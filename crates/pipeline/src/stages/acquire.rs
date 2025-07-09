//! Data acquisition stage for EEG sensors

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, error};

use eeg_types::EegPacket;
use crate::data::PipelineData;
use crate::error::{PipelineError, PipelineResult};
use crate::stage::{PipelineStage, StageFactory, StageParams, StageMetric};

/// Data acquisition stage that reads from EEG sensors
pub struct AcquireStage {
    /// Sample rate in Hz
    sample_rate: f32,
    /// Gain setting
    gain: u32,
    /// Number of channels
    channel_count: usize,
    /// Metrics
    packets_generated: u64,
}

impl AcquireStage {
    /// Create a new acquire stage
    pub fn new(sample_rate: f32, gain: u32, channel_count: usize) -> Self {
        Self {
            sample_rate,
            gain,
            channel_count,
            packets_generated: 0,
        }
    }
}

#[async_trait]
impl PipelineStage for AcquireStage {
    async fn process(&mut self, input: PipelineData) -> PipelineResult<PipelineData> {
        // Source stages should only process Trigger inputs
        match input {
            PipelineData::Trigger => {
                info!("Acquire stage received trigger, generating data...");
                // Generate data in response to trigger
            }
            _ => {
                return Err(PipelineError::InvalidInput {
                    message: "Acquire stage only accepts Trigger inputs".to_string(),
                });
            }
        }
        
        // In a real implementation, this would interface with the actual sensor hardware
        // For now, we'll generate mock data at the specified sample rate
        
        // Sleep to simulate real-time data acquisition
        let packet_interval = tokio::time::Duration::from_millis(100); // 100ms packets
        tokio::time::sleep(packet_interval).await;
        
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;

        // Generate mock EEG data
        let samples_per_packet = (self.sample_rate / 10.0) as usize; // 100ms worth of data
        let total_samples = samples_per_packet * self.channel_count;
        
        let mut timestamps = Vec::with_capacity(total_samples);
        let mut raw_samples = Vec::with_capacity(total_samples);
        let mut voltage_samples = Vec::with_capacity(total_samples);

        for i in 0..total_samples {
            let sample_time = timestamp + (i as u64 * 1000000 / self.sample_rate as u64);
            timestamps.push(sample_time);
            
            // Generate mock raw ADC values with some variation
            let base_value = (self.packets_generated as i32 * 1000 + i as i32 * 100) % 8388607;
            let noise = ((i as f32 * 0.1).sin() * 1000.0) as i32;
            let raw_value = base_value + noise;
            raw_samples.push(raw_value);
            
            // Convert to voltage (simplified)
            let voltage = (raw_value as f32 / 8388607.0) * 4.5; // Assuming 4.5V reference
            voltage_samples.push(voltage);
        }

        let packet = EegPacket::new(
            timestamps,
            self.packets_generated,
            raw_samples,
            voltage_samples,
            self.channel_count,
            self.sample_rate,
        );

        self.packets_generated += 1;
        
        info!("Generated EEG packet #{} with {} samples",
              self.packets_generated, total_samples);

        Ok(PipelineData::RawEeg(Arc::new(packet)))
    }

    fn stage_type(&self) -> &'static str {
        "acquire"
    }

    fn description(&self) -> &'static str {
        "Data acquisition stage for EEG sensors"
    }

    async fn initialize(&mut self) -> PipelineResult<()> {
        info!("Initializing acquire stage: {}Hz, gain={}, channels={}", 
               self.sample_rate, self.gain, self.channel_count);
        Ok(())
    }

    async fn cleanup(&mut self) -> PipelineResult<()> {
        info!("Cleaning up acquire stage, generated {} packets", self.packets_generated);
        Ok(())
    }

    fn get_metrics(&self) -> Vec<StageMetric> {
        vec![
            StageMetric::new(
                "packets_generated".to_string(),
                self.packets_generated as f64,
                "count".to_string(),
            ),
            StageMetric::new(
                "sample_rate".to_string(),
                self.sample_rate as f64,
                "Hz".to_string(),
            ),
            StageMetric::new(
                "channel_count".to_string(),
                self.channel_count as f64,
                "count".to_string(),
            ),
        ]
    }

    fn validate_params(&self, params: &StageParams) -> PipelineResult<()> {
        // Validate sample rate
        if let Some(sps) = params.get("sps") {
            let sps = sps.as_f64().ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "sps parameter must be a number".to_string(),
            })?;
            
            if sps <= 0.0 || sps > 10000.0 {
                return Err(PipelineError::InvalidConfiguration {
                    message: "sps must be between 0 and 10000".to_string(),
                });
            }
        }

        // Validate gain
        if let Some(gain) = params.get("gain") {
            let gain = gain.as_u64().ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "gain parameter must be a positive integer".to_string(),
            })?;
            
            if gain == 0 || gain > 24 {
                return Err(PipelineError::InvalidConfiguration {
                    message: "gain must be between 1 and 24".to_string(),
                });
            }
        }

        Ok(())
    }
}

/// Factory for creating acquire stages
pub struct AcquireStageFactory;

impl AcquireStageFactory {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StageFactory for AcquireStageFactory {
    async fn create_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn PipelineStage>> {
        let sample_rate = params.get("sps")
            .and_then(|v| v.as_f64())
            .unwrap_or(500.0) as f32;

        let gain = params.get("gain")
            .and_then(|v| v.as_u64())
            .unwrap_or(24) as u32;

        let channel_count = params.get("channels")
            .and_then(|v| v.as_u64())
            .unwrap_or(8) as usize;

        let stage = AcquireStage::new(sample_rate, gain, channel_count);
        
        // Validate parameters
        stage.validate_params(params)?;

        Ok(Box::new(stage))
    }

    fn stage_type(&self) -> &'static str {
        "acquire"
    }

    fn parameter_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "sps": {
                    "type": "number",
                    "description": "Sample rate in Hz",
                    "minimum": 1,
                    "maximum": 10000,
                    "default": 500
                },
                "gain": {
                    "type": "integer",
                    "description": "Amplifier gain setting",
                    "minimum": 1,
                    "maximum": 24,
                    "default": 24
                },
                "channels": {
                    "type": "integer",
                    "description": "Number of EEG channels",
                    "minimum": 1,
                    "maximum": 32,
                    "default": 8
                }
            }
        })
    }
}

impl Default for AcquireStageFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_acquire_stage_creation() {
        let factory = AcquireStageFactory::new();
        let mut params = HashMap::new();
        params.insert("sps".to_string(), json!(250.0));
        params.insert("gain".to_string(), json!(12));
        params.insert("channels".to_string(), json!(4));

        let stage = factory.create_stage(&params).await.unwrap();
        assert_eq!(stage.stage_type(), "acquire");
    }

    #[tokio::test]
    async fn test_acquire_stage_validation() {
        let factory = AcquireStageFactory::new();
        
        // Test invalid sample rate
        let mut params = HashMap::new();
        params.insert("sps".to_string(), json!(-1.0));
        assert!(factory.create_stage(&params).await.is_err());

        // Test invalid gain
        params.clear();
        params.insert("gain".to_string(), json!(0));
        assert!(factory.create_stage(&params).await.is_err());
    }

    #[test]
    fn test_parameter_schema() {
        let factory = AcquireStageFactory::new();
        let schema = factory.parameter_schema();
        assert!(schema.is_object());
        assert!(schema["properties"]["sps"].is_object());
        assert!(schema["properties"]["gain"].is_object());
        assert!(schema["properties"]["channels"].is_object());
    }
}