//! Data acquisition stage for EEG sensors
//!
//! This stage serves as the bridge between sensor drivers and the data plane.
//! It demonstrates several key architectural patterns:
//! - **Source stage pattern:** Generates data without requiring input packets
//! - **Memory pool integration:** Acquires packets from configured pools
//! - **Hardware abstraction:** Bridges sensor drivers to the unified data plane
//! - **Configurable timing:** Supports different sample rates and batch sizes

use async_trait::async_trait;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::error::TrySendError;
use tokio::time::{interval, Duration, Instant};
use tracing::{debug, error, info, trace, warn};

use crate::ctrl_loop;
use crate::data::{MemoryPool, Packet, PacketHeader, RawEegPacket};
use crate::AnyMemoryPool;
use crate::error::{PipelineResult, StageError};
use crate::stage::{
    ControlMsg, DataPlaneStage, DataPlaneStageFactory, DataPlaneStageErased, ErasedDataPlaneStageFactory,
    ErasedStageContext, StageContext, StageParams, StaticStageRegistrar, Output
};

/// A high-performance data acquisition stage for the data plane.
/// 
/// This stage acts as a source, generating raw EEG data packets at a configured
/// sample rate. In a real implementation, this would interface with sensor drivers
/// like ADS1299, but for now it generates mock data for testing.
pub struct AcquisitionStage {
    /// Sample rate in Hz
    sample_rate: AtomicU32,
    /// Number of channels to generate
    channel_count: AtomicU32,
    /// Samples per packet (batch size)
    samples_per_packet: AtomicU32,
    /// A flag to enable or disable data generation on-the-fly
    enabled: AtomicBool,
    /// Packet counter for frame IDs
    packet_counter: AtomicU64,
    /// Last packet generation time for rate limiting
    last_packet_time: std::sync::Mutex<Option<Instant>>,
    /// The number of packets to process before yielding to the scheduler
    yield_threshold: u32,
    // Cached handles to avoid HashMap lookups in the hot path
    output_tx: Option<Box<dyn Output<RawEegPacket>>>,
    memory_pool: Option<Arc<Mutex<dyn AnyMemoryPool>>>,
}

#[async_trait]
impl DataPlaneStage<RawEegPacket, RawEegPacket> for AcquisitionStage {
    /// The main execution loop for the acquisition stage.
    /// 
    /// Unlike other stages, this is a source stage that generates data
    /// rather than processing input packets.
    async fn run(&mut self, ctx: &mut StageContext<RawEegPacket, RawEegPacket>) -> Result<(), StageError> {
        // First, handle any incoming control messages
        ctrl_loop!(self, ctx);

        // Then, generate data packets at the configured rate
        self.generate_packets(ctx).await
    }
}

#[async_trait]
impl DataPlaneStageErased for AcquisitionStage {
    async fn run_erased(&mut self, context: &mut dyn ErasedStageContext) -> Result<(), StageError> {
        // Downcast the erased context back to the concrete type
        let concrete_context = context
            .as_any_mut()
            .downcast_mut::<StageContext<RawEegPacket, RawEegPacket>>()
            .ok_or_else(|| StageError::Fatal("Context type mismatch for AcquisitionStage".into()))?;
        
        self.run(concrete_context).await
    }
}

impl AcquisitionStage {
    /// Creates a new `AcquisitionStage`.
    pub fn new(sample_rate: f32, channel_count: u32, samples_per_packet: u32, enabled: bool, yield_threshold: u32) -> Self {
        Self {
            sample_rate: AtomicU32::new(sample_rate.to_bits()),
            channel_count: AtomicU32::new(channel_count),
            samples_per_packet: AtomicU32::new(samples_per_packet),
            enabled: AtomicBool::new(enabled),
            packet_counter: AtomicU64::new(0),
            last_packet_time: std::sync::Mutex::new(None),
            yield_threshold,
            output_tx: None,
            memory_pool: None,
        }
    }

    /// Safely sets the sample rate using atomic operations.
    fn set_sample_rate(&self, sample_rate: f32) {
        self.sample_rate.store(sample_rate.to_bits(), Ordering::Release);
    }

    /// Safely gets the sample rate using atomic operations.
    fn get_sample_rate(&self) -> f32 {
        f32::from_bits(self.sample_rate.load(Ordering::Acquire))
    }

    /// Gets mutable references to the output handle and memory pool, initializing them
    /// on the first call. This avoids HashMap lookups in the hot path.
    #[cold]
    #[inline(always)]
    fn lazy_io<'a>(
        output_tx: &'a mut Option<Box<dyn Output<RawEegPacket>>>,
        memory_pool: &'a mut Option<Arc<Mutex<dyn AnyMemoryPool>>>,
        ctx: &'a mut StageContext<RawEegPacket, RawEegPacket>,
    ) -> Result<(&'a mut dyn Output<RawEegPacket>, &'a Arc<Mutex<dyn AnyMemoryPool>>), StageError> {
        if output_tx.is_none() {
            *output_tx = Some(
                ctx.outputs
                    .remove("out")
                    .unwrap_or_else(|| panic!("Output 'out' not found for acquisition stage")),
            );
            *memory_pool = Some(
                ctx.memory_pools
                    .get("raw_eeg")
                    .cloned()
                    .unwrap_or_else(|| panic!("Memory pool 'raw_eeg' not found for acquisition stage")),
            );
        }
        Ok((
            output_tx.as_mut().unwrap().as_mut(),
            memory_pool.as_ref().unwrap(),
        ))
    }

    /// Generates data packets at the configured sample rate.
    async fn generate_packets(&mut self, ctx: &mut StageContext<RawEegPacket, RawEegPacket>) -> Result<(), StageError> {
        // Check if generation is enabled first
        if !self.enabled.load(Ordering::Acquire) {
            // If disabled, just yield and return
            tokio::task::yield_now().await;
            return Ok(());
        }

        // Get all values from atomic fields and other self fields before any mutable borrows
        let sample_rate = self.get_sample_rate();
        let channel_count = self.channel_count.load(Ordering::Acquire);
        let samples_per_packet = self.samples_per_packet.load(Ordering::Acquire);
        let yield_threshold = self.yield_threshold;
        
        // Calculate packet interval based on sample rate and batch size
        let packet_interval_ms = (samples_per_packet as f32 / sample_rate * 1000.0) as u64;
        let packet_interval = Duration::from_millis(packet_interval_ms.max(1));

        // Check if it's time to generate a new packet
        let now = Instant::now();
        let should_generate = {
            let mut last_time = self.last_packet_time.lock().unwrap();
            match *last_time {
                Some(last) if now.duration_since(last) < packet_interval => false,
                _ => {
                    *last_time = Some(now);
                    true
                }
            }
        };

        if !should_generate {
            // Not time yet, yield and return
            tokio::task::yield_now().await;
            return Ok(());
        }

        // Generate a new packet
        let frame_id = self.packet_counter.fetch_add(1, Ordering::Relaxed);
        let timestamp = now.elapsed().as_micros() as u64;

        // Now get the output handle after we've read all the values we need from self
        let (output, _pool_arc) = Self::lazy_io(&mut self.output_tx, &mut self.memory_pool, ctx)?;

        // Try to acquire a packet from the memory pool
        let header = PacketHeader {
            batch_size: samples_per_packet as usize,
            timestamp,
        };
        
        // Create a mock packet for now - in real implementation this would come from the pool
        let dummy_pool = std::sync::Weak::new(); // Temporary: empty weak reference
        let mut packet = Packet::new(header, RawEegPacket {
            samples: vec![0; samples_per_packet as usize * 8], // 8 channels
        }, dummy_pool);

        // Generate mock raw EEG data
        let total_samples = (samples_per_packet * channel_count) as usize;
        packet.samples.samples.clear();
        packet.samples.samples.reserve(total_samples);

        for i in 0..total_samples {
            // Generate mock raw ADC values with some variation
            let base_value = (frame_id as i32 * 1000 + i as i32 * 100) % 8388607;
            let noise = ((i as f32 * 0.1).sin() * 1000.0) as i32;
            let raw_value = base_value + noise;
            packet.samples.samples.push(raw_value);
        }

        trace!("Generated raw EEG packet #{} with {} samples", frame_id, total_samples);

        // Send the packet downstream
        if let Err(e) = output.send(packet).await {
            error!("Failed to send acquisition packet: {}", e);
            return Err(StageError::Fatal(format!("Failed to send packet: {}", e)));
        }

        debug!("Acquisition stage generated packet #{}", frame_id);
        Ok(())
    }

    /// Updates a stage parameter based on a key-value pair from the control plane.
    fn update_param(&mut self, key: &str, val: Value) -> Result<(), StageError> {
        match key {
            "enabled" => {
                let is_enabled = val.as_bool().unwrap_or(true);
                self.enabled.store(is_enabled, Ordering::Release);
                trace!("Acquisition 'enabled' set to {}", is_enabled);
            }
            "sample_rate" => {
                let new_rate = val
                    .as_f64()
                    .map(|v| v as f32)
                    .ok_or_else(|| StageError::BadParam(key.into()))?;

                if new_rate <= 0.0 || new_rate > 10000.0 {
                    return Err(StageError::BadParam("sample_rate must be between 0 and 10000".into()));
                }

                self.set_sample_rate(new_rate);
                trace!("Updating sample rate to {}", new_rate);
            }
            "channel_count" => {
                let new_count = val
                    .as_u64()
                    .ok_or_else(|| StageError::BadParam(key.into()))?;

                if new_count == 0 || new_count > 32 {
                    return Err(StageError::BadParam("channel_count must be between 1 and 32".into()));
                }

                self.channel_count.store(new_count as u32, Ordering::Release);
                trace!("Updating channel count to {}", new_count);
            }
            "samples_per_packet" => {
                let new_batch = val
                    .as_u64()
                    .ok_or_else(|| StageError::BadParam(key.into()))?;

                if new_batch == 0 || new_batch > 1024 {
                    return Err(StageError::BadParam("samples_per_packet must be between 1 and 1024".into()));
                }

                self.samples_per_packet.store(new_batch as u32, Ordering::Release);
                trace!("Updating samples per packet to {}", new_batch);
            }
            _ => return Err(StageError::BadParam(key.into())),
        }
        Ok(())
    }
}

/// Parameters for configuring an `AcquisitionStage`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionStageParams {
    /// Sample rate in Hz
    #[serde(default = "default_sample_rate")]
    sample_rate: f32,
    /// Number of EEG channels
    #[serde(default = "default_channel_count")]
    channel_count: u32,
    /// Number of samples per packet (batch size)
    #[serde(default = "default_samples_per_packet")]
    samples_per_packet: u32,
    /// The number of packets to process before yielding to the scheduler
    #[serde(default = "default_yield_threshold")]
    yield_threshold: u32,
}

fn default_sample_rate() -> f32 {
    500.0
}

fn default_channel_count() -> u32 {
    8
}

fn default_samples_per_packet() -> u32 {
    50 // 100ms at 500Hz
}

fn default_yield_threshold() -> u32 {
    10
}

pub struct AcquisitionStageFactory;

impl AcquisitionStageFactory {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DataPlaneStageFactory<RawEegPacket, RawEegPacket> for AcquisitionStageFactory {
    /// Creates a new `AcquisitionStage` instance from JSON parameters.
    async fn create_stage(
        &self,
        params: &StageParams,
    ) -> PipelineResult<Box<dyn DataPlaneStage<RawEegPacket, RawEegPacket>>> {
        // Convert StageParams (HashMap) to serde_json::Value before deserializing
        let params_value = serde_json::to_value(params.clone())?;
        let params: AcquisitionStageParams = serde_json::from_value(params_value)?;
        
        let stage = AcquisitionStage::new(
            params.sample_rate,
            params.channel_count,
            params.samples_per_packet,
            true, // enabled by default
            params.yield_threshold,
        );
        Ok(Box::new(stage))
    }

    fn stage_type(&self) -> &'static str {
        "acquisition"
    }

    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::to_value(schema_for!(AcquisitionStageParams)).unwrap_or_default()
    }
}

#[async_trait]
impl ErasedDataPlaneStageFactory for AcquisitionStageFactory {
    async fn create_erased_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn DataPlaneStageErased>> {
        // Extract parameters with defaults
        let sample_rate = params.get("sample_rate")
            .and_then(|v| v.as_f64())
            .unwrap_or(500.0) as f32;
        
        let channel_count = params.get("channel_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(8) as u32;
            
        let samples_per_packet = params.get("samples_per_packet")
            .and_then(|v| v.as_u64())
            .unwrap_or(50) as u32;
        
        let stage = AcquisitionStage::new(
            sample_rate,
            channel_count,
            samples_per_packet,
            true, // enabled by default
            10,   // default yield threshold
        );
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
        factory_fn: || Box::new(AcquisitionStageFactory::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::MemoryPool;
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    // Mock output for testing
    struct MockOutput {
        tx: mpsc::UnboundedSender<Packet<RawEegPacket>>,
    }

    #[async_trait]
    impl Output<RawEegPacket> for MockOutput {
        async fn send(&mut self, packet: Packet<RawEegPacket>) -> Result<(), TrySendError<Packet<RawEegPacket>>> {
            self.tx.send(packet).map_err(|e| TrySendError::Closed(e.0))
        }
        fn try_send(&mut self, packet: Packet<RawEegPacket>) -> Result<(), TrySendError<Packet<RawEegPacket>>> {
            self.tx.send(packet).map_err(|e| TrySendError::Closed(e.0))
        }
    }

    #[tokio::test]
    async fn test_acquisition_stage_creation() {
        let stage = AcquisitionStage::new(250.0, 4, 25, true, 10);
        assert_eq!(stage.get_sample_rate(), 250.0);
        assert_eq!(stage.channel_count.load(Ordering::Acquire), 4);
        assert_eq!(stage.samples_per_packet.load(Ordering::Acquire), 25);
        assert!(stage.enabled.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn test_acquisition_stage_factory() {
        let factory = AcquisitionStageFactory::new();
        assert_eq!(crate::stage::DataPlaneStageFactory::stage_type(&factory), "acquisition");

        let mut params = HashMap::new();
        params.insert("sample_rate".to_string(), serde_json::json!(125.0));
        params.insert("channel_count".to_string(), serde_json::json!(2));
        params.insert("samples_per_packet".to_string(), serde_json::json!(12));

        let stage = factory.create_stage(&params).await.unwrap();
        // We can't easily test the internal state without more complex setup
        // but we can verify the stage was created successfully
    }

    #[test]
    fn test_parameter_schema() {
        let factory = AcquisitionStageFactory::new();
        let schema = crate::stage::DataPlaneStageFactory::parameter_schema(&factory);
        assert!(schema.is_object());
        // The schema should contain our parameter definitions
    }
}