//! Converts raw i32 ADC samples into f32 voltage values.

use crate::allocator::RecycledI32F32TupleVec;
use crate::config::StageConfig;
use crate::data::{PacketData, PacketView, RtPacket};
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext, StageInitCtx};
use flume::Receiver;
use sensors::ads1299::helpers::ch_raw_to_voltage;
use std::sync::Arc;

/// A factory for creating `ToVoltage` stages.
#[derive(Default)]
pub struct ToVoltageFactory;

impl StageFactory for ToVoltageFactory {
    fn create(
        &self,
        config: &StageConfig,
        _: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        let v_ref = config.params["vref"]
            .as_f64()
            .ok_or_else(|| StageError::BadConfig("Missing vref".to_string()))? as f32;

        Ok((
            Box::new(ToVoltage::new(
                config.name.clone(),
                v_ref,
                config.outputs.clone(),
            )),
            None,
        ))
    }
}

pub struct ToVoltage {
    id: String,
    v_ref: f32,
    output_name: String,
}

impl ToVoltage {
    pub fn new(id: String, v_ref: f32, outputs: Vec<String>) -> Self {
        let output_name =
            format!("{}.{}", id, outputs.get(0).cloned().unwrap_or_else(|| "0".to_string()));
        Self {
            id,
            v_ref,
            output_name,
        }
    }
}

impl Stage for ToVoltage {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        pkt: Arc<RtPacket>,
        ctx: &mut StageContext,
    ) -> Result<Option<Arc<RtPacket>>, StageError> {
        let source_id = match &*pkt {
            RtPacket::RawI32(d) => &d.header.source_id,
            RtPacket::Voltage(d) => &d.header.source_id,
            RtPacket::RawAndVoltage(d) => &d.header.source_id,
        };
        tracing::info!("to_voltage received packet with source_id: {}", source_id);
        let view = PacketView::from(&*pkt);

        if let PacketView::RawI32 { header, data } = view {
            // Extract gain from packet metadata instead of using self.gain
            let gain = header.meta.gain;
            let v_ref = header.meta.v_ref;

            let mut samples_both =
                RecycledI32F32TupleVec::with_capacity(ctx.allocator.clone(), data.len());

            for &raw_sample in data.iter() {
                let voltage = ch_raw_to_voltage(raw_sample, v_ref, gain);
                samples_both.push((raw_sample, voltage));
            }

            let mut output_packet = PacketData {
                header: header.clone(),
                samples: samples_both,
            };
            output_packet.header.source_id = self.output_name.clone();

            return Ok(Some(Arc::new(RtPacket::RawAndVoltage(output_packet))));
        }

        // If the packet is not RawI32, pass it through.
        Ok(Some(pkt))
    }
}
