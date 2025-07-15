//! A test demonstrating a multi-stage synchronous pipeline.

use pipeline::data::{Packet, PacketHeader, SensorMeta};
use pipeline::config::{StageConfig, SystemConfig};
use pipeline::control::{ControlCommand, PipelineEvent};
use pipeline::error::StageError;
use pipeline::graph::PipelineGraph;
use pipeline::registry::{StageFactory, StageRegistry};
use pipeline::runtime::{run, RuntimeMsg};
use pipeline::stage::{Stage, StageContext};
use serde_json::json;
use std::any::Any;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

// A simple stage that multiplies every sample by a given factor.
struct MultiplierStage {
    id: String,
    factor: f32,
}

#[derive(Default)]
struct MultiplierStageFactory;

impl StageFactory for MultiplierStageFactory {
    fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage>, StageError> {
        let factor = config
            .params
            .get("factor")
            .and_then(|v| v.as_f64())
            .map(|f| f as f32)
            .ok_or_else(|| StageError::BadConfig("Missing 'factor' param".to_string()))?;

        Ok(Box::new(MultiplierStage {
            id: config.name.clone(),
            factor,
        }))
    }
}

impl Stage for MultiplierStage {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Box<dyn Any + Send>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Box<dyn Any + Send>>, StageError> {
        info!("MultiplierStage processing packet");
        let mut packet = packet
            .downcast::<Packet<f32>>()
            .map_err(|_| StageError::BadConfig("Expected Packet<f32>".to_string()))?;

        for sample in &mut packet.samples {
            *sample *= self.factor;
        }
        Ok(Some(packet as Box<dyn Any + Send>))
    }
}

// A sink stage to capture the final output for verification.
struct TestSink {
    id: String,
    output_tx: Sender<usize>, // Send back the number of samples received
}

#[derive(Default)]
struct TestSinkFactory {
    // The factory will be configured with the sender at runtime.
    output_tx: Arc<std::sync::Mutex<Option<Sender<usize>>>>,
}

impl StageFactory for TestSinkFactory {
    fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage>, StageError> {
        let tx = self
            .output_tx
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| StageError::BadConfig("output_tx already taken".to_string()))?;

        Ok(Box::new(TestSink {
            id: config.name.clone(),
            output_tx: tx,
        }))
    }
}

impl Stage for TestSink {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Box<dyn Any + Send>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Box<dyn Any + Send>>, StageError> {
        if let Some(packet) = packet.downcast_ref::<Packet<f32>>() {
            self.output_tx.send(packet.samples.len()).unwrap();
        }
        Ok(None) // Sinks consume the packet
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Full Synchronous Pipeline Test");
    info!("==============================");

    // 1. Set up the stage registry
    let mut registry = StageRegistry::new();
    registry.register(
        "Acquire",
        Box::new(pipeline::stages::acquire::AcquireFactory::default()),
    );
    registry.register(
        "ToVoltage",
        Box::new(pipeline::stages::to_voltage::ToVoltageFactory::default()),
    );
    registry.register("Multiplier", Box::new(MultiplierStageFactory::default()));

    // The TestSink needs to communicate the result count back to the main thread.
    let (output_tx, output_rx) = mpsc::channel::<usize>();
    let sink_factory = TestSinkFactory {
        output_tx: Arc::new(std::sync::Mutex::new(Some(output_tx))),
    };
    registry.register("TestSink", Box::new(sink_factory));

    // 2. Define the pipeline configuration
    let config: SystemConfig = serde_json::from_value(json!({
        "version": "1.0",
        "stages": [
            {
                "name": "acquire",
                "type": "Acquire",
                "params": { "samples_per_packet": 4 }
            },
            {
                "name": "to_voltage",
                "type": "ToVoltage",
                "inputs": ["acquire"]
            },
            {
                "name": "multiplier",
                "type": "Multiplier",
                "inputs": ["to_voltage"],
                "params": { "factor": 10.0 }
            },
            {
                "name": "test_sink",
                "type": "TestSink",
                "inputs": ["multiplier"]
            }
        ]
    }))?;

    // 3. Create channels for the pipeline runtime
    let (runtime_tx, runtime_rx) = mpsc::channel::<RuntimeMsg>();
    let (event_tx, event_rx) = mpsc::channel::<PipelineEvent>();
    let context = StageContext::new(event_tx.clone());

    // 4. Build the pipeline graph
    let graph = PipelineGraph::build(&config, &registry, context)?;

    // 5. Spawn the pipeline runtime in a dedicated thread
    let pipeline_handle = thread::spawn(move || run(runtime_rx, event_tx, graph));

    // 6. Send a large number of packets into the pipeline
    let num_packets = 1_000;
    let samples_per_packet = 4;
    let total_samples_sent = num_packets * samples_per_packet;

    info!(
        "Sending {} packets with {} samples each (total {} samples)...",
        num_packets, samples_per_packet, total_samples_sent
    );

    for i in 0..num_packets {
        let packet: Box<dyn Any + Send> = Box::new(Packet {
            header: PacketHeader {
                ts_ns: i as u64,
                batch_size: samples_per_packet as u32,
                meta: Arc::new(SensorMeta::default()),
            },
            samples: vec![1000i32, 2000, -1000, -2000],
        });
        runtime_tx.send(RuntimeMsg::Data(packet))?;
    }

    // 7. Wait for the final output from the TestSink
    let mut total_samples_received = 0;
    for _ in 0..num_packets {
        total_samples_received += output_rx.recv()?;
    }

    info!("\nTotal samples received: {}", total_samples_received);
    assert_eq!(total_samples_sent, total_samples_received);

    // 8. Shut down the pipeline
    info!("Shutting down pipeline...");
    runtime_tx.send(RuntimeMsg::Ctrl(ControlCommand::Shutdown))?;

    // 9. Wait for shutdown confirmation
    loop {
        if let Ok(PipelineEvent::ShutdownAck) = event_rx.recv() {
            info!("Shutdown acknowledged.");
            break;
        }
    }

    // 10. Join the pipeline thread
    pipeline_handle.join().unwrap()?;

    info!("\nTest completed successfully!");
    Ok(())
}