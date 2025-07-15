//! A test demonstrating a multi-stage pipeline using `tokio::mpsc` channels.

use std::sync::Arc;
use tokio::sync::mpsc;
use eeg_types::data::{Packet, PacketHeader, SensorMeta};
use pipeline::control::PipelineEvent;
use pipeline::stage::{Stage, StageContext};
use pipeline::stages::to_voltage::ToVoltage;
use pipeline::error::StageError;

/// A simple stage that multiplies every sample by a given factor.
struct MultiplierStage {
    factor: f32,
}

#[async_trait::async_trait]
impl Stage<f32, f32> for MultiplierStage {
    fn id(&self) -> &str {
        "MultiplierStage"
    }

    async fn process(&mut self, packet: Packet<f32>, _ctx: &mut StageContext) -> Result<Option<Packet<f32>>, StageError> {
        let samples = packet.samples.into_iter().map(|s| s * self.factor).collect();
        Ok(Some(Packet {
            header: packet.header,
            samples,
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Full Pipeline Test with Channels");
    println!("================================");

    // 1. Create channels for communication between stages
    let (tx1, mut rx1) = mpsc::channel::<Packet<f32>>(10);
    let (tx2, mut rx2) = mpsc::channel::<Packet<f32>>(10);
    let (tx3, mut rx3) = mpsc::channel::<Packet<f32>>(10);
    let (event_tx, mut _event_rx) = mpsc::channel::<PipelineEvent>(10);

    // 2. Spawn a task to generate input data
    tokio::spawn(async move {
        let sensor_meta = Arc::new(SensorMeta {
            schema_ver: 2,
            source_type: "mock".to_string(),
            v_ref: 2.5,
            adc_bits: 24,
            gain: 1.0,
            sample_rate: 250,
            offset_code: 0,
            is_twos_complement: true,
        });
        let packet = Packet {
            header: PacketHeader { ts_ns: 0, batch_size: 4, meta: sensor_meta },
            samples: vec![1000.0, 2000.0, -1000.0, -2000.0],
        };
        println!("Input -> {:?}", packet.samples);
        tx1.send(packet).await.unwrap();
    });

    // 3. Spawn tasks for each stage in the pipeline
    // Stage 1: ToVoltage
    tokio::spawn({
        let event_tx = event_tx.clone();
        async move {
            let mut stage = ToVoltage::default();
            let mut ctx = StageContext::new(event_tx);
            while let Some(packet) = rx1.recv().await {
                if let Ok(Some(out_packet)) = stage.process(packet, &mut ctx).await {
                    println!("ToVoltage -> {:?}", out_packet.samples);
                    tx2.send(out_packet).await.unwrap();
                }
            }
        }
    });

    // Stage 2: Multiplier
    tokio::spawn(async move {
        let mut stage = MultiplierStage { factor: 10.0 };
        let mut ctx = StageContext::new(event_tx);
        while let Some(packet) = rx2.recv().await {
            if let Ok(Some(out_packet)) = stage.process(packet, &mut ctx).await {
                println!("Multiplier -> {:?}", out_packet.samples);
                tx3.send(out_packet).await.unwrap();
            }
        }
    });

    // 4. Receive the final output
    if let Some(final_packet) = rx3.recv().await {
        println!("\nFinal Output: {:?}", final_packet.samples);
        // Expected: (1000 * scale * 10), (2000 * scale * 10), etc.
        // This just verifies the pipeline ran.
        assert_eq!(final_packet.samples.len(), 4);
    }

    println!("\nTest completed successfully!");
    Ok(())
}