//! Digital filtering stage for EEG data

use async_trait::async_trait;
use serde_json::json;
use std::any::Any;
use std::collections::HashMap;
use tracing::{info, error};

use eeg_types::{EegPacket, FilteredEegPacket};
use crate::data::PipelineData;
use crate::error::{PipelineError, PipelineResult};
use crate::stage::{PipelineStage, StageFactory, StageParams, StageMetric};

/// Digital filter stage for EEG data
pub struct FilterStage {
    /// Low-pass cutoff frequency in Hz
    lowpass: Option<f32>,
    /// High-pass cutoff frequency in Hz
    highpass: Option<f32>,
    /// Notch filter frequency in Hz (e.g., 50Hz or 60Hz for power line noise)
    notch: Option<f32>,
    /// Filter order
    order: u8,
    /// Metrics
    packets_processed: u64,
}

impl FilterStage {
    /// Create a new filter stage
    pub fn new(lowpass: Option<f32>, highpass: Option<f32>, notch: Option<f32>, order: u8) -> Self {
        Self {
            lowpass,
            highpass,
            notch,
            order,
            packets_processed: 0,
        }
    }

    /// Apply filtering to voltage samples (simplified implementation)
    fn apply_filter(&self, samples: &[f32], sample_rate: f32) -> Vec<f32> {
        // This is a very simplified filter implementation
        // In a real system, you would use proper DSP libraries like rustfft or similar
        
        let mut filtered = samples.to_vec();
        
        // Simple low-pass filter (moving average approximation)
        if let Some(cutoff) = self.lowpass {
            let window_size = (sample_rate / cutoff / 2.0) as usize;
            if window_size > 1 {
                filtered = self.moving_average(&filtered, window_size);
            }
        }
        
        // Simple high-pass filter (difference from low-pass)
        if let Some(cutoff) = self.highpass {
            let window_size = (sample_rate / cutoff / 2.0) as usize;
            if window_size > 1 {
                let lowpass_result = self.moving_average(&filtered, window_size);
                let original_filtered = filtered.clone();
                for (i, &original) in original_filtered.iter().enumerate() {
                    filtered[i] = original - lowpass_result[i];
                }
            }
        }
        
        // Simple notch filter (placeholder - would need proper implementation)
        if let Some(_notch_freq) = self.notch {
            // TODO: Implement proper notch filter
            // For now, just apply a small attenuation
            for sample in &mut filtered {
                *sample *= 0.95;
            }
        }
        
        filtered
    }
    
    /// Simple moving average filter
    fn moving_average(&self, samples: &[f32], window_size: usize) -> Vec<f32> {
        if window_size <= 1 {
            return samples.to_vec();
        }
        
        let mut result = Vec::with_capacity(samples.len());
        let half_window = window_size / 2;
        
        for i in 0..samples.len() {
            let start = if i >= half_window { i - half_window } else { 0 };
            let end = std::cmp::min(i + half_window + 1, samples.len());
            
            let sum: f32 = samples[start..end].iter().sum();
            let avg = sum / (end - start) as f32;
            result.push(avg);
        }
        
        result
    }
}

#[async_trait]
impl PipelineStage for FilterStage {
    async fn process(&mut self, input: PipelineData) -> PipelineResult<PipelineData> {
        // Extract EEG packet from pipeline data
        let packet = match input {
            PipelineData::RawEeg(packet) => packet,
            _ => return Err(PipelineError::RuntimeError {
                stage_name: "filter".to_string(),
                message: "Filter stage expects RawEeg input".to_string(),
            }),
        };

        // Apply filtering to voltage samples
        let filtered_samples = self.apply_filter(&packet.voltage_samples, packet.sample_rate);

        // Create filtered EEG packet
        let filtered_packet = FilteredEegPacket::new(
            packet.timestamps.clone(),
            packet.frame_id,
            filtered_samples,
            packet.channel_count,
            packet.sample_rate,
        );

        self.packets_processed += 1;

        Ok(PipelineData::FilteredEeg(std::sync::Arc::new(filtered_packet)))
    }

    fn stage_type(&self) -> &'static str {
        "filter"
    }

    fn description(&self) -> &'static str {
        "Digital filtering stage for EEG data"
    }

    async fn initialize(&mut self) -> PipelineResult<()> {
        info!("Initializing filter stage: lowpass={:?}Hz, highpass={:?}Hz, notch={:?}Hz, order={}", 
               self.lowpass, self.highpass, self.notch, self.order);
        Ok(())
    }

    async fn cleanup(&mut self) -> PipelineResult<()> {
        info!("Cleaning up filter stage, processed {} packets", self.packets_processed);
        Ok(())
    }

    fn get_metrics(&self) -> Vec<StageMetric> {
        let mut metrics = vec![
            StageMetric::new(
                "packets_processed".to_string(),
                self.packets_processed as f64,
                "count".to_string(),
            ),
            StageMetric::new(
                "filter_order".to_string(),
                self.order as f64,
                "order".to_string(),
            ),
        ];

        if let Some(lowpass) = self.lowpass {
            metrics.push(StageMetric::new(
                "lowpass_cutoff".to_string(),
                lowpass as f64,
                "Hz".to_string(),
            ));
        }

        if let Some(highpass) = self.highpass {
            metrics.push(StageMetric::new(
                "highpass_cutoff".to_string(),
                highpass as f64,
                "Hz".to_string(),
            ));
        }

        if let Some(notch) = self.notch {
            metrics.push(StageMetric::new(
                "notch_frequency".to_string(),
                notch as f64,
                "Hz".to_string(),
            ));
        }

        metrics
    }

    fn validate_params(&self, params: &StageParams) -> PipelineResult<()> {
        // Validate lowpass frequency
        if let Some(lowpass) = params.get("lowpass") {
            let lowpass = lowpass.as_f64().ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "lowpass parameter must be a number".to_string(),
            })?;
            
            if lowpass <= 0.0 || lowpass > 1000.0 {
                return Err(PipelineError::InvalidConfiguration {
                    message: "lowpass frequency must be between 0 and 1000 Hz".to_string(),
                });
            }
        }

        // Validate highpass frequency
        if let Some(highpass) = params.get("highpass") {
            let highpass = highpass.as_f64().ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "highpass parameter must be a number".to_string(),
            })?;
            
            if highpass <= 0.0 || highpass > 1000.0 {
                return Err(PipelineError::InvalidConfiguration {
                    message: "highpass frequency must be between 0 and 1000 Hz".to_string(),
                });
            }
        }

        // Validate notch frequency
        if let Some(notch) = params.get("notch") {
            let notch = notch.as_f64().ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "notch parameter must be a number".to_string(),
            })?;
            
            if notch <= 0.0 || notch > 1000.0 {
                return Err(PipelineError::InvalidConfiguration {
                    message: "notch frequency must be between 0 and 1000 Hz".to_string(),
                });
            }
        }

        // Validate filter order
        if let Some(order) = params.get("order") {
            let order = order.as_u64().ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "order parameter must be a positive integer".to_string(),
            })?;
            
            if order == 0 || order > 10 {
                return Err(PipelineError::InvalidConfiguration {
                    message: "filter order must be between 1 and 10".to_string(),
                });
            }
        }

        Ok(())
    }
}

/// Factory for creating filter stages
pub struct FilterStageFactory;

impl FilterStageFactory {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StageFactory for FilterStageFactory {
    async fn create_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn PipelineStage>> {
        let lowpass = params.get("lowpass").and_then(|v| v.as_f64()).map(|v| v as f32);
        let highpass = params.get("highpass").and_then(|v| v.as_f64()).map(|v| v as f32);
        let notch = params.get("notch").and_then(|v| v.as_f64()).map(|v| v as f32);
        let order = params.get("order")
            .and_then(|v| v.as_u64())
            .unwrap_or(4) as u8;

        let stage = FilterStage::new(lowpass, highpass, notch, order);
        
        // Validate parameters
        stage.validate_params(params)?;

        Ok(Box::new(stage))
    }

    fn stage_type(&self) -> &'static str {
        "filter"
    }

    fn parameter_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "lowpass": {
                    "type": "number",
                    "description": "Low-pass cutoff frequency in Hz",
                    "minimum": 0.1,
                    "maximum": 1000.0
                },
                "highpass": {
                    "type": "number",
                    "description": "High-pass cutoff frequency in Hz",
                    "minimum": 0.1,
                    "maximum": 1000.0
                },
                "notch": {
                    "type": "number",
                    "description": "Notch filter frequency in Hz (e.g., 50 or 60 for power line noise)",
                    "minimum": 1.0,
                    "maximum": 1000.0
                },
                "order": {
                    "type": "integer",
                    "description": "Filter order",
                    "minimum": 1,
                    "maximum": 10,
                    "default": 4
                }
            }
        })
    }
}

impl Default for FilterStageFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_filter_stage_creation() {
        let factory = FilterStageFactory::new();
        let mut params = HashMap::new();
        params.insert("lowpass".to_string(), json!(40.0));
        params.insert("highpass".to_string(), json!(0.5));
        params.insert("order".to_string(), json!(4));

        let stage = factory.create_stage(&params).await.unwrap();
        assert_eq!(stage.stage_type(), "filter");
    }

    #[tokio::test]
    async fn test_filter_processing() {
        let mut stage = FilterStage::new(Some(40.0), Some(0.5), None, 4);
        
        // Create test EEG packet
        let timestamps = vec![1000, 1001, 1002, 1003, 1004];
        let raw_samples = vec![100, 200, 300, 400, 500];
        let voltage_samples = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        
        let packet = EegPacket::new(
            timestamps,
            1,
            raw_samples,
            voltage_samples,
            1,
            250.0,
        );

        let input = PipelineData::RawEeg(std::sync::Arc::new(packet));
        let result = stage.process(input).await.unwrap();
        
        // Check that we got a filtered packet
        match result {
            PipelineData::FilteredEeg(filtered) => {
                assert_eq!(filtered.samples.len(), 5);
                assert_eq!(filtered.frame_id, 1);
                assert_eq!(filtered.sample_rate, 250.0);
            }
            _ => panic!("Expected FilteredEeg output"),
        }
    }

    #[tokio::test]
    async fn test_filter_stage_validation() {
        let factory = FilterStageFactory::new();
        
        // Test invalid lowpass frequency
        let mut params = HashMap::new();
        params.insert("lowpass".to_string(), json!(-1.0));
        assert!(factory.create_stage(&params).await.is_err());

        // Test invalid order
        params.clear();
        params.insert("order".to_string(), json!(0));
        assert!(factory.create_stage(&params).await.is_err());
    }

    #[test]
    fn test_moving_average() {
        let stage = FilterStage::new(None, None, None, 1);
        let samples = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = stage.moving_average(&samples, 3);
        
        // Check that the result has the same length
        assert_eq!(result.len(), samples.len());
        
        // Check that the middle value is approximately correct
        assert!((result[2] - 2.0).abs() < 0.1); // Should be close to average of 1,2,3
    }

    #[test]
    fn test_parameter_schema() {
        let factory = FilterStageFactory::new();
        let schema = factory.parameter_schema();
        assert!(schema.is_object());
        assert!(schema["properties"]["lowpass"].is_object());
        assert!(schema["properties"]["highpass"].is_object());
        assert!(schema["properties"]["notch"].is_object());
        assert!(schema["properties"]["order"].is_object());
    }
}