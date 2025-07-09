//! Voltage conversion stage for converting raw ADC values to voltages

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, error};

use eeg_types::EegPacket;
use crate::data::PipelineData;
use crate::error::{PipelineError, PipelineResult};
use crate::stage::{PipelineStage, StageFactory, StageParams, StageMetric};

/// Stage that converts raw ADC values to voltage values
pub struct ToVoltageStage {
    /// Reference voltage for conversion
    vref: f32,
    /// ADC resolution (bits)
    adc_bits: u8,
    /// Metrics
    packets_processed: u64,
}

impl ToVoltageStage {
    /// Create a new to_voltage stage
    pub fn new(vref: f32, adc_bits: u8) -> Self {
        Self {
            vref,
            adc_bits,
            packets_processed: 0,
        }
    }

    /// Convert raw ADC value to voltage
    fn raw_to_voltage(&self, raw_value: i32) -> f32 {
        let max_value = (1 << (self.adc_bits - 1)) - 1; // For signed values
        (raw_value as f32 / max_value as f32) * self.vref
    }
}

#[async_trait]
impl PipelineStage for ToVoltageStage {
    async fn process(&mut self, input: PipelineData) -> PipelineResult<PipelineData> {
        // Extract EegPacket from PipelineData
        let packet = match input {
            PipelineData::RawEeg(packet) => packet,
            _ => {
                return Err(PipelineError::RuntimeError {
                    stage_name: "to_voltage".to_string(),
                    message: "Input is not RawEeg data".to_string(),
                });
            }
        };

        // Convert raw samples to voltages
        let voltage_samples: Vec<f32> = packet.raw_samples
            .iter()
            .map(|&raw| self.raw_to_voltage(raw))
            .collect();

        // Create new packet with converted voltages
        let converted_packet = EegPacket::new(
            packet.timestamps.to_vec(),
            packet.frame_id,
            packet.raw_samples.to_vec(),
            voltage_samples,
            packet.channel_count,
            packet.sample_rate,
        );

        self.packets_processed += 1;

        Ok(PipelineData::RawEeg(Arc::new(converted_packet)))
    }

    fn stage_type(&self) -> &'static str {
        "to_voltage"
    }

    fn description(&self) -> &'static str {
        "Converts raw ADC values to voltage values"
    }

    async fn initialize(&mut self) -> PipelineResult<()> {
        info!("Initializing to_voltage stage: vref={}V, adc_bits={}", 
               self.vref, self.adc_bits);
        Ok(())
    }

    async fn cleanup(&mut self) -> PipelineResult<()> {
        info!("Cleaning up to_voltage stage, processed {} packets", self.packets_processed);
        Ok(())
    }

    fn get_metrics(&self) -> Vec<StageMetric> {
        vec![
            StageMetric::new(
                "packets_processed".to_string(),
                self.packets_processed as f64,
                "count".to_string(),
            ),
            StageMetric::new(
                "vref".to_string(),
                self.vref as f64,
                "V".to_string(),
            ),
            StageMetric::new(
                "adc_bits".to_string(),
                self.adc_bits as f64,
                "bits".to_string(),
            ),
        ]
    }

    fn validate_params(&self, params: &StageParams) -> PipelineResult<()> {
        // Validate reference voltage
        if let Some(vref) = params.get("vref") {
            let vref = vref.as_f64().ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "vref parameter must be a number".to_string(),
            })?;
            
            if vref <= 0.0 || vref > 10.0 {
                return Err(PipelineError::InvalidConfiguration {
                    message: "vref must be between 0 and 10 volts".to_string(),
                });
            }
        }

        // Validate ADC bits
        if let Some(adc_bits) = params.get("adc_bits") {
            let adc_bits = adc_bits.as_u64().ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "adc_bits parameter must be a positive integer".to_string(),
            })?;
            
            if adc_bits < 8 || adc_bits > 32 {
                return Err(PipelineError::InvalidConfiguration {
                    message: "adc_bits must be between 8 and 32".to_string(),
                });
            }
        }

        Ok(())
    }
}

/// Factory for creating to_voltage stages
pub struct ToVoltageStageFactory;

impl ToVoltageStageFactory {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StageFactory for ToVoltageStageFactory {
    async fn create_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn PipelineStage>> {
        let vref = params.get("vref")
            .and_then(|v| v.as_f64())
            .unwrap_or(4.5) as f32;

        let adc_bits = params.get("adc_bits")
            .and_then(|v| v.as_u64())
            .unwrap_or(24) as u8;

        let stage = ToVoltageStage::new(vref, adc_bits);
        
        // Validate parameters
        stage.validate_params(params)?;

        Ok(Box::new(stage))
    }

    fn stage_type(&self) -> &'static str {
        "to_voltage"
    }

    fn parameter_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "vref": {
                    "type": "number",
                    "description": "Reference voltage for ADC conversion",
                    "minimum": 0.1,
                    "maximum": 10.0,
                    "default": 4.5
                },
                "adc_bits": {
                    "type": "integer",
                    "description": "ADC resolution in bits",
                    "minimum": 8,
                    "maximum": 32,
                    "default": 24
                }
            }
        })
    }
}

impl Default for ToVoltageStageFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_to_voltage_stage_creation() {
        let factory = ToVoltageStageFactory::new();
        let mut params = HashMap::new();
        params.insert("vref".to_string(), json!(3.3));
        params.insert("adc_bits".to_string(), json!(16));

        let stage = factory.create_stage(&params).await.unwrap();
        assert_eq!(stage.stage_type(), "to_voltage");
    }

    #[tokio::test]
    async fn test_voltage_conversion() {
        let mut stage = ToVoltageStage::new(4.5, 24);
        
        // Create test EEG packet
        let timestamps = vec![1000, 1001, 1002];
        let raw_samples = vec![8388607, 0, -8388607]; // Max, zero, min for 24-bit
        let voltage_samples = vec![0.0, 0.0, 0.0]; // Will be overwritten
        
        let packet = EegPacket::new(
            timestamps,
            1,
            raw_samples,
            voltage_samples,
            1,
            250.0,
        );

        let result = stage.process(Box::new(packet)).await.unwrap();
        let converted = result.downcast::<EegPacket>().unwrap();
        
        // Check that voltages were converted
        assert!((converted.voltage_samples[0] - 4.5).abs() < 0.001); // Max value -> vref
        assert!(converted.voltage_samples[1].abs() < 0.001); // Zero -> zero
        assert!((converted.voltage_samples[2] + 4.5).abs() < 0.001); // Min value -> -vref
    }

    #[tokio::test]
    async fn test_to_voltage_stage_validation() {
        let factory = ToVoltageStageFactory::new();
        
        // Test invalid vref
        let mut params = HashMap::new();
        params.insert("vref".to_string(), json!(-1.0));
        assert!(factory.create_stage(&params).await.is_err());

        // Test invalid adc_bits
        params.clear();
        params.insert("adc_bits".to_string(), json!(7));
        assert!(factory.create_stage(&params).await.is_err());
    }

    #[test]
    fn test_parameter_schema() {
        let factory = ToVoltageStageFactory::new();
        let schema = factory.parameter_schema();
        assert!(schema.is_object());
        assert!(schema["properties"]["vref"].is_object());
        assert!(schema["properties"]["adc_bits"].is_object());
    }
}