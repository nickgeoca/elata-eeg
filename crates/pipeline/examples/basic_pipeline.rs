//! Basic pipeline example demonstrating the new, simplified architecture.

use std::sync::Arc;
use eeg_types::data::{Packet, PacketHeader, SensorMeta};
use pipeline::control::PipelineEvent;
use pipeline::stage::{Stage, StageContext};
use pipeline::stages::to_voltage::ToVoltage;
use pipeline::error::StageError;
use tokio::sync::mpsc;

// A simple stage that doubles each sample.
struct DoublerStage;

#[async_trait::async_trait]
impl Stage<f32, f32> for DoublerStage {
    fn id(&self) -> &str {
        "DoublerStage"
    }

    async fn process(&mut self, packet: Packet<f32>, _ctx: &mut StageContext) -> Result<Option<Packet<f32>>, StageError> {
        let samples = packet.samples.into_iter().map(|s| s * 2.0).collect();
        Ok(Some(Packet {
            header: packet.header,
            samples,
        }))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Basic Pipeline Example");
    println!("======================");

    // 1. Create a test packet of raw ADC counts
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

    let input_packet = Packet {
        header: PacketHeader {
            ts_ns: 0,
            batch_size: 4,
            meta: sensor_meta,
        },
        samples: vec![1000.0, 2000.0, -1000.0, -2000.0],
    };
    println!("Input Samples: {:?}", input_packet.samples);


    // 2. Instantiate the stages
    let mut to_voltage_stage = ToVoltage::default();
    let mut doubler_stage = DoublerStage;
    let (event_tx, _) = mpsc::channel::<PipelineEvent>(10);
    let mut ctx = StageContext::new(event_tx);

    // 3. Manually process the packet through the pipeline
    // Stage 1: Convert ADC counts to voltage
    let voltage_packet = to_voltage_stage.process(input_packet, &mut ctx).await?.unwrap();
    println!("After ToVoltage: {:?}", voltage_packet.samples);

    // Stage 2: Double the voltage values
    let final_packet = doubler_stage.process(voltage_packet, &mut ctx).await?.unwrap();
    println!("After Doubler:   {:?}", final_packet.samples);

    println!("\nExample completed successfully!");

    Ok(())
}