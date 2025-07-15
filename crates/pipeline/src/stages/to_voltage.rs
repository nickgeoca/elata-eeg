//! Converts raw i32 ADC samples into f32 voltage values.

use crate::config::StageConfig;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext};
use async_trait::async_trait;
use eeg_types::Packet;
use std::sync::Arc;

/// A factory for creating `ToVoltage` stages.
#[derive(Default)]
pub struct ToVoltageFactory;

#[async_trait]
impl StageFactory<f32, f32> for ToVoltageFactory {
    async fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage<f32, f32>>, StageError> {
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

#[async_trait]
impl Stage<f32, f32> for ToVoltage {
    fn id(&self) -> &str {
        &self.id
    }

    async fn process(
        &mut self,
        packet: Packet<f32>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Packet<f32>>, StageError> {
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
            .into_iter()
            .map(|raw_sample| (raw_sample as i32 - self.cached_offset) as f32 * self.cached_scale_factor)
            .collect();

        let output_packet = Packet {
            header: packet.header,
            samples: samples_f32,
        };

        Ok(Some(output_packet))
    }
}
