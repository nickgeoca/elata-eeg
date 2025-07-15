//! Converts raw i32 ADC samples into f32 voltage values.

use crate::config::StageConfig;
use crate::data::{Packet, PacketData};
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext};
use std::sync::Arc;

/// A factory for creating `ToVoltage` stages.
#[derive(Default)]
pub struct ToVoltageFactory;

impl StageFactory for ToVoltageFactory {
    fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage>, StageError> {
        Ok(Box::new(ToVoltage {
            id: config.name.clone(),
            ..Default::default()
        }))
    }
}

/// A pipeline stage that converts raw integer samples to voltage values.
pub struct ToVoltage {
    id: String,
    cached_meta_ptr: usize,
    cached_scale_factor: f32,
    cached_offset: i32,
}

impl Default for ToVoltage {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            cached_meta_ptr: 0,
            cached_scale_factor: 1.0,
            cached_offset: 0,
        }
    }
}

impl Stage for ToVoltage {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Packet,
        _ctx: &mut StageContext,
    ) -> Result<Option<Packet>, StageError> {
        if let Packet::RawI32(packet) = packet {
            let meta_ptr = Arc::as_ptr(&packet.header.meta) as usize;

            if self.cached_meta_ptr != meta_ptr {
                let meta = &packet.header.meta;
                let full_scale_range = if meta.is_twos_complement {
                    1i32 << (meta.adc_bits - 1)
                } else {
                    1i32 << meta.adc_bits
                };
                self.cached_scale_factor = (meta.v_ref / meta.gain) / full_scale_range as f32;
                self.cached_offset = meta.offset_code;
                self.cached_meta_ptr = meta_ptr;
            }

            let samples_f32: Vec<f32> = packet
                .samples
                .iter()
                .map(|&raw_sample| (raw_sample - self.cached_offset) as f32 * self.cached_scale_factor)
                .collect();

            let output_packet = PacketData {
                header: packet.header.clone(),
                samples: samples_f32,
            };

            return Ok(Some(Packet::Voltage(output_packet)));
        }
        Ok(None)
    }
}
