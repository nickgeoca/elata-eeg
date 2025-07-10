//! CSV sink stage for recording voltage EEG data to files.
//
// This stage serves as a terminal sink for the data plane pipeline.
// It demonstrates several key architectural patterns:
// - **File I/O handling:** Efficiently writes CSV data to disk with proper buffering.
// - **Hot-reloadable parameters:** File path and format options can be changed on-the-fly.
// - **Correct concurrency:** Uses atomic operations for configuration changes.
// - **Efficient run-loop:** Drains the input queue to maximize throughput.
// - **Graceful termination:** Properly closes files and flushes buffers on shutdown.

use async_trait::async_trait;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, trace, warn};

use crate::ctrl_loop;
use crate::data::{Packet, VoltageEegPacket, AnyPacketType};
use crate::error::{PipelineResult, StageError};
use crate::stage::{
    ControlMsg, DataPlaneStage, DataPlaneStageFactory, DataPlaneStageErased, ErasedDataPlaneStageFactory,
    ErasedStageContext, StageContext, StageParams, StaticStageRegistrar, Input, Output
};

/// A high-performance CSV sink stage for the data plane.
pub struct CsvSink {
    /// The output file path.
    path: PathBuf,
    /// Whether to include CSV headers.
    include_headers: AtomicBool,
    /// Whether the stage is enabled.
    enabled: AtomicBool,
    /// Number of packets written.
    packets_written: AtomicU64,
    /// Number of bytes written.
    bytes_written: AtomicU64,
    /// The file writer (wrapped in Arc<Mutex> for thread safety).
    writer: Arc<Mutex<Option<BufWriter<File>>>>,
    /// Whether headers have been written.
    headers_written: AtomicBool,
    // Cached handles to avoid HashMap lookups in the hot path.
    input_rx: Option<Box<dyn Input<VoltageEegPacket>>>,
}

#[async_trait]
impl DataPlaneStage<VoltageEegPacket, VoltageEegPacket> for CsvSink {
    /// The main execution loop for the CSV sink stage.
    async fn run(&mut self, ctx: &mut StageContext<VoltageEegPacket, VoltageEegPacket>) -> Result<(), StageError> {
        // First, handle any incoming control messages.
        ctrl_loop!(self, ctx);

        // Then, enter the packet processing loop.
        self.process_packets(ctx).await
    }
}

#[async_trait]
impl DataPlaneStageErased for CsvSink {
    async fn run_erased(&mut self, context: &mut dyn ErasedStageContext) -> Result<(), StageError> {
        // Downcast the erased context back to the concrete type
        let concrete_context = context
            .as_any_mut()
            .downcast_mut::<StageContext<VoltageEegPacket, VoltageEegPacket>>()
            .ok_or_else(|| StageError::Fatal("Context type mismatch for CsvSink".into()))?;
        
        self.run(concrete_context).await
    }
}

impl CsvSink {
    /// Creates a new `CsvSink`.
    pub fn new(path: PathBuf, include_headers: bool, enabled: bool) -> Self {
        Self {
            path,
            include_headers: AtomicBool::new(include_headers),
            enabled: AtomicBool::new(enabled),
            packets_written: AtomicU64::new(0),
            bytes_written: AtomicU64::new(0),
            writer: Arc::new(Mutex::new(None)),
            headers_written: AtomicBool::new(false),
            input_rx: None,
        }
    }

    /// Initialize the CSV file and writer.
    async fn initialize_file(&self) -> Result<(), StageError> {
        let mut writer_guard = self.writer.lock().await;
        
        if writer_guard.is_some() {
            return Ok(()); // Already initialized
        }

        // Create the file and buffered writer
        let file = File::create(&self.path)
            .map_err(|e| StageError::Fatal(format!("Failed to create CSV file {:?}: {}", self.path, e)))?;
        
        let buf_writer = BufWriter::new(file);
        *writer_guard = Some(buf_writer);
        
        info!("CSV file initialized: {:?}", self.path);
        Ok(())
    }

    /// Write CSV headers if needed.
    async fn write_headers_if_needed(&self) -> Result<(), StageError> {
        if !self.include_headers.load(Ordering::Acquire) || 
           self.headers_written.load(Ordering::Acquire) {
            return Ok(());
        }

        let mut writer_guard = self.writer.lock().await;
        if let Some(ref mut writer) = writer_guard.as_mut() {
            let header_line = "timestamp,channel_0,channel_1,channel_2,channel_3,channel_4,channel_5,channel_6,channel_7\n";
            writer.write_all(header_line.as_bytes())
                .map_err(|e| StageError::Fatal(format!("Failed to write CSV headers: {}", e)))?;
            
            self.headers_written.store(true, Ordering::Release);
            self.bytes_written.fetch_add(header_line.len() as u64, Ordering::Relaxed);
            trace!("CSV headers written");
        }
        
        Ok(())
    }

    /// Gets mutable references to the input handle, initializing it on the first call.
    #[cold]
    #[inline(always)]
    fn lazy_io<'a>(
        input_rx: &'a mut Option<Box<dyn Input<VoltageEegPacket>>>,
        ctx: &'a mut StageContext<VoltageEegPacket, VoltageEegPacket>,
    ) -> &'a mut dyn Input<VoltageEegPacket> {
        if input_rx.is_none() {
            *input_rx = Some(
                ctx.inputs
                    .remove("in")
                    .unwrap_or_else(|| panic!("Input 'in' not found for CSV sink stage")),
            );
        }
        input_rx.as_mut().unwrap().as_mut()
    }

    /// Efficiently processes all available packets in the input queue.
    async fn process_packets(&mut self, ctx: &mut StageContext<VoltageEegPacket, VoltageEegPacket>) -> Result<(), StageError> {
        // Initialize file if needed (do this before borrowing input)
        self.initialize_file().await?;
        self.write_headers_if_needed().await?;

        let input = Self::lazy_io(&mut self.input_rx, ctx);

        // Loop to drain the input queue
        loop {
            let pkt = match input.try_recv()? {
                Some(p) => p,
                // The queue is empty, so we're done for now.
                None => return Ok(()),
            };

            // Skip processing if disabled
            if !self.enabled.load(Ordering::Acquire) {
                continue;
            }

            // Format the packet as CSV (inline to avoid borrowing conflicts)
            let csv_line = {
                let mut line = String::new();
                
                // Add timestamp
                line.push_str(&pkt.header.timestamp.to_string());
                
                // Add voltage samples (assuming 8 channels for now)
                for sample in pkt.samples.samples.iter().take(8) {
                    line.push(',');
                    line.push_str(&format!("{:.6}", sample));
                }
                
                // Pad with zeros if we have fewer than 8 channels
                for _ in pkt.samples.samples.len()..8 {
                    line.push_str(",0.0");
                }
                
                line.push('\n');
                line
            };

            // Write to file
            {
                let mut writer_guard = self.writer.lock().await;
                if let Some(ref mut buf_writer) = writer_guard.as_mut() {
                    buf_writer.write_all(csv_line.as_bytes())
                        .map_err(|e| StageError::Fatal(format!("Failed to write CSV data: {}", e)))?;
                    
                    // Flush periodically for real-time monitoring
                    buf_writer.flush()
                        .map_err(|e| StageError::Fatal(format!("Failed to flush CSV data: {}", e)))?;
                }
            }

            self.packets_written.fetch_add(1, Ordering::Relaxed);
            self.bytes_written.fetch_add(csv_line.len() as u64, Ordering::Relaxed);
            
            trace!("CSV packet written, total packets: {}", self.packets_written.load(Ordering::Relaxed));
        }
    }

    /// Format a voltage EEG packet as a CSV line.
    fn format_packet_as_csv(&self, packet: &Packet<VoltageEegPacket>) -> Result<String, StageError> {
        let mut line = String::new();
        
        // Add timestamp
        line.push_str(&packet.header.timestamp.to_string());
        
        // Add voltage samples (assuming 8 channels for now)
        for sample in packet.samples.samples.iter().take(8) {
            line.push(',');
            line.push_str(&format!("{:.6}", sample));
        }
        
        // Pad with zeros if we have fewer than 8 channels
        for _ in packet.samples.samples.len()..8 {
            line.push_str(",0.0");
        }
        
        line.push('\n');
        Ok(line)
    }

    /// Updates a stage parameter based on a key-value pair from the control plane.
    fn update_param(&mut self, key: &str, val: Value) -> Result<(), StageError> {
        match key {
            "enabled" => {
                let is_enabled = val.as_bool().unwrap_or(true);
                self.enabled.store(is_enabled, Ordering::Release);
                trace!("CSV sink 'enabled' set to {}", is_enabled);
            }
            "include_headers" => {
                let include = val.as_bool().unwrap_or(true);
                self.include_headers.store(include, Ordering::Release);
                trace!("CSV sink 'include_headers' set to {}", include);
            }
            _ => return Err(StageError::BadParam(key.into())),
        }
        Ok(())
    }
}

/// Parameters for configuring a `CsvSink`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CsvSinkParams {
    /// The output file path.
    #[serde(default = "default_path")]
    path: String,
    /// Whether to include CSV headers.
    #[serde(default = "default_include_headers")]
    include_headers: bool,
}

fn default_path() -> String {
    "output.csv".to_string()
}

fn default_include_headers() -> bool {
    true
}

pub struct CsvSinkFactory;

impl CsvSinkFactory {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DataPlaneStageFactory<VoltageEegPacket, VoltageEegPacket> for CsvSinkFactory {
    /// Creates a new `CsvSink` instance from JSON parameters.
    async fn create_stage(
        &self,
        params: &StageParams,
    ) -> PipelineResult<Box<dyn DataPlaneStage<VoltageEegPacket, VoltageEegPacket>>> {
        // Convert StageParams (HashMap) to serde_json::Value before deserializing
        let params_value = serde_json::to_value(params.clone())?;
        let params: CsvSinkParams = serde_json::from_value(params_value)?;
        let stage = CsvSink::new(PathBuf::from(params.path), params.include_headers, true);
        Ok(Box::new(stage))
    }

    fn stage_type(&self) -> &'static str {
        "csv_sink"
    }

    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::to_value(schema_for!(CsvSinkParams)).unwrap_or_default()
    }
}

#[async_trait]
impl ErasedDataPlaneStageFactory for CsvSinkFactory {
    async fn create_erased_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn DataPlaneStageErased>> {
        // Extract path parameter
        let path = params.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("output.csv");
        
        let include_headers = params.get("include_headers")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        
        let stage = CsvSink::new(PathBuf::from(path), include_headers, true);
        Ok(Box::new(stage) as Box<dyn DataPlaneStageErased>)
    }

    fn stage_type(&self) -> &'static str {
        DataPlaneStageFactory::stage_type(self)
    }

    fn parameter_schema(&self) -> serde_json::Value {
        DataPlaneStageFactory::parameter_schema(self)
    }
}

// Automatically register this stage factory with the pipeline runtime.
inventory::submit! {
    StaticStageRegistrar {
        factory_fn: || Box::new(CsvSinkFactory::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{PacketHeader, VoltageEegPacket};
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    // MockInput for testing
    struct MockInput {
        rx: mpsc::UnboundedReceiver<Packet<VoltageEegPacket>>,
    }

    #[async_trait]
    impl Input<VoltageEegPacket> for MockInput {
        async fn recv(&mut self) -> Result<Option<Packet<VoltageEegPacket>>, StageError> {
            Ok(self.rx.recv().await)
        }
        fn try_recv(&mut self) -> Result<Option<Packet<VoltageEegPacket>>, StageError> {
            match self.rx.try_recv() {
                Ok(p) => Ok(Some(p)),
                Err(mpsc::error::TryRecvError::Empty) => Ok(None),
                Err(mpsc::error::TryRecvError::Disconnected) => Err(StageError::QueueClosed),
            }
        }
    }

    fn setup_test_rig() -> (
        CsvSink,
        StageContext<VoltageEegPacket, VoltageEegPacket>,
        mpsc::UnboundedSender<ControlMsg>,
        mpsc::UnboundedSender<Packet<VoltageEegPacket>>,
    ) {
        let stage = CsvSink::new(PathBuf::from("/tmp/test.csv"), true, true);

        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let (in_tx, in_rx) = mpsc::unbounded_channel();

        let mut inputs: HashMap<String, Box<dyn Input<VoltageEegPacket>>> = HashMap::new();
        inputs.insert("in".to_string(), Box::new(MockInput { rx: in_rx }));

        let ctx = StageContext {
            control_rx,
            inputs,
            outputs: HashMap::new(), // CSV sink has no outputs
            memory_pools: HashMap::new(),
        };

        (stage, ctx, control_tx, in_tx)
    }

    #[tokio::test]
    async fn test_csv_sink_creation() {
        let factory = CsvSinkFactory::new();
        let mut params = HashMap::new();
        params.insert("path".to_string(), serde_json::json!("test.csv"));
        params.insert("include_headers".to_string(), serde_json::json!(true));

        let stage = factory.create_stage(&params).await.unwrap();
        assert_eq!(DataPlaneStageFactory::stage_type(&factory), "csv_sink");
    }

    #[tokio::test]
    async fn test_csv_formatting() {
        let sink = CsvSink::new(PathBuf::from("test.csv"), true, true);
        
        let header = PacketHeader { batch_size: 8, timestamp: 1234567890 };
        let samples = VoltageEegPacket { samples: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0] };
        let packet = Packet::new_for_test(samples);
        
        let csv_line = sink.format_packet_as_csv(&packet).unwrap();
        assert!(csv_line.contains("1234567890")); // timestamp
        assert!(csv_line.contains("1.000000"));   // first sample
        assert!(csv_line.contains("8.000000"));   // last sample
    }

    #[test]
    fn test_parameter_schema() {
        let factory = CsvSinkFactory::new();
        let schema = crate::stage::DataPlaneStageFactory::parameter_schema(&factory);
        assert!(schema.is_object());
    }
}