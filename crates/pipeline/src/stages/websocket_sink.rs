//! WebSocket sink stage for streaming data to clients

use async_trait::async_trait;
use serde_json::json;
use std::any::Any;
use std::collections::HashMap;
use tracing::{info, error, warn};

use eeg_types::{EegPacket, FilteredEegPacket, WebSocketTopic};
use crate::data::{PipelineData, WebSocketData};
use crate::error::{PipelineError, PipelineResult};
use crate::stage::{PipelineStage, StageFactory, StageParams, StageMetric};

/// WebSocket sink stage for streaming data to clients
pub struct WebSocketSink {
    /// WebSocket endpoint/topic
    endpoint: String,
    /// Data fields to include in output
    fields: Vec<String>,
    /// Output format (json, binary)
    format: String,
    /// Metrics
    packets_sent: u64,
    bytes_sent: u64,
}

impl WebSocketSink {
    /// Create a new WebSocket sink
    pub fn new(endpoint: String, fields: Vec<String>, format: String) -> Self {
        Self {
            endpoint,
            fields,
            format,
            packets_sent: 0,
            bytes_sent: 0,
        }
    }

    /// Convert packet to output format
    fn format_packet(&self, packet: &dyn Any) -> PipelineResult<Vec<u8>> {
        match self.format.as_str() {
            "json" => self.format_as_json(packet),
            "binary" => self.format_as_binary(packet),
            _ => Err(PipelineError::InvalidConfiguration {
                message: format!("Unsupported format: {}", self.format),
            }),
        }
    }

    /// Format packet as JSON
    fn format_as_json(&self, packet: &dyn Any) -> PipelineResult<Vec<u8>> {
        // Try to downcast to different packet types
        if let Some(eeg_packet) = packet.downcast_ref::<EegPacket>() {
            let mut json_obj = serde_json::Map::new();
            
            for field in &self.fields {
                match field.as_str() {
                    "timestamp" => {
                        if let Some(&first_ts) = eeg_packet.timestamps.first() {
                            json_obj.insert("timestamp".to_string(), json!(first_ts));
                        }
                    }
                    "frame_id" => {
                        json_obj.insert("frame_id".to_string(), json!(eeg_packet.frame_id));
                    }
                    "raw_channels" => {
                        json_obj.insert("raw_channels".to_string(), json!(eeg_packet.raw_samples.as_ref()));
                    }
                    "voltage_channels" => {
                        json_obj.insert("voltage_channels".to_string(), json!(eeg_packet.voltage_samples.as_ref()));
                    }
                    "sample_rate" => {
                        json_obj.insert("sample_rate".to_string(), json!(eeg_packet.sample_rate));
                    }
                    "channel_count" => {
                        json_obj.insert("channel_count".to_string(), json!(eeg_packet.channel_count));
                    }
                    _ => {
                        // Unknown field, skip
                    }
                }
            }
            
            let json_value = serde_json::Value::Object(json_obj);
            Ok(serde_json::to_vec(&json_value)?)
        } else if let Some(filtered_packet) = packet.downcast_ref::<FilteredEegPacket>() {
            let mut json_obj = serde_json::Map::new();
            
            for field in &self.fields {
                match field.as_str() {
                    "timestamp" => {
                        if let Some(&first_ts) = filtered_packet.timestamps.first() {
                            json_obj.insert("timestamp".to_string(), json!(first_ts));
                        }
                    }
                    "frame_id" => {
                        json_obj.insert("frame_id".to_string(), json!(filtered_packet.frame_id));
                    }
                    "filtered_channels" => {
                        json_obj.insert("filtered_channels".to_string(), json!(filtered_packet.samples.as_ref()));
                    }
                    "sample_rate" => {
                        json_obj.insert("sample_rate".to_string(), json!(filtered_packet.sample_rate));
                    }
                    "channel_count" => {
                        json_obj.insert("channel_count".to_string(), json!(filtered_packet.channel_count));
                    }
                    _ => {
                        // Unknown field, skip
                    }
                }
            }
            
            let json_value = serde_json::Value::Object(json_obj);
            Ok(serde_json::to_vec(&json_value)?)
        } else {
            Err(PipelineError::RuntimeError {
                stage_name: "websocket_sink".to_string(),
                message: "Unsupported packet type for JSON formatting".to_string(),
            })
        }
    }

    /// Format packet as binary
    fn format_as_binary(&self, packet: &dyn Any) -> PipelineResult<Vec<u8>> {
        // Try to downcast to different packet types
        if let Some(eeg_packet) = packet.downcast_ref::<EegPacket>() {
            Ok(eeg_packet.to_binary())
        } else if let Some(filtered_packet) = packet.downcast_ref::<FilteredEegPacket>() {
            Ok(filtered_packet.to_binary())
        } else {
            Err(PipelineError::RuntimeError {
                stage_name: "websocket_sink".to_string(),
                message: "Unsupported packet type for binary formatting".to_string(),
            })
        }
    }

    /// Send data via WebSocket (placeholder implementation)
    async fn send_websocket_data(&mut self, data: Vec<u8>) -> PipelineResult<()> {
        // TODO: Implement actual WebSocket sending
        // This would integrate with the existing WebSocket infrastructure
        
        self.packets_sent += 1;
        self.bytes_sent += data.len() as u64;
        
        // For now, just log that we would send the data
        info!("WebSocket sink would send {} bytes to endpoint: {}", data.len(), self.endpoint);
        
        Ok(())
    }
}

#[async_trait]
impl PipelineStage for WebSocketSink {
    async fn process(&mut self, input: PipelineData) -> PipelineResult<PipelineData> {
        // Format the packet based on data type
        let formatted_data = match &input {
            PipelineData::RawEeg(packet) => {
                let ws_data = crate::data::WebSocketData::from_eeg_packet(packet, &self.format);
                ws_data.payload
            }
            PipelineData::FilteredEeg(packet) => {
                let ws_data = crate::data::WebSocketData::from_filtered_packet(packet, &self.format);
                ws_data.payload
            }
            _ => return Err(PipelineError::RuntimeError {
                stage_name: "websocket_sink".to_string(),
                message: "WebSocket sink only supports RawEeg and FilteredEeg data".to_string(),
            }),
        };
        
        // Send via WebSocket
        self.send_websocket_data(formatted_data).await?;
        
        // WebSocket sinks are terminal nodes, so we return the input unchanged
        // (though in practice, nothing should be connected to the output)
        Ok(input)
    }

    fn stage_type(&self) -> &'static str {
        "websocket_sink"
    }

    fn description(&self) -> &'static str {
        "WebSocket sink for streaming data to clients"
    }

    async fn initialize(&mut self) -> PipelineResult<()> {
        info!("Initializing WebSocket sink: endpoint={}, format={}, fields={:?}", 
               self.endpoint, self.format, self.fields);
        Ok(())
    }

    async fn cleanup(&mut self) -> PipelineResult<()> {
        info!("Cleaning up WebSocket sink, sent {} packets ({} bytes)", 
               self.packets_sent, self.bytes_sent);
        Ok(())
    }

    fn get_metrics(&self) -> Vec<StageMetric> {
        vec![
            StageMetric::new(
                "packets_sent".to_string(),
                self.packets_sent as f64,
                "count".to_string(),
            ),
            StageMetric::new(
                "bytes_sent".to_string(),
                self.bytes_sent as f64,
                "bytes".to_string(),
            ),
        ]
    }

    fn validate_params(&self, params: &StageParams) -> PipelineResult<()> {
        // Validate endpoint
        if let Some(endpoint) = params.get("endpoint") {
            let endpoint = endpoint.as_str().ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "endpoint parameter must be a string".to_string(),
            })?;
            
            if endpoint.is_empty() {
                return Err(PipelineError::InvalidConfiguration {
                    message: "endpoint cannot be empty".to_string(),
                });
            }
        }

        // Validate format
        if let Some(format) = params.get("format") {
            let format = format.as_str().ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "format parameter must be a string".to_string(),
            })?;
            
            if !["json", "binary"].contains(&format) {
                return Err(PipelineError::InvalidConfiguration {
                    message: "format must be 'json' or 'binary'".to_string(),
                });
            }
        }

        // Validate fields
        if let Some(fields) = params.get("fields") {
            let fields = fields.as_array().ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "fields parameter must be an array".to_string(),
            })?;
            
            if fields.is_empty() {
                return Err(PipelineError::InvalidConfiguration {
                    message: "fields array cannot be empty".to_string(),
                });
            }
        }

        Ok(())
    }
}

/// Factory for creating WebSocket sink stages
pub struct WebSocketSinkFactory;

impl WebSocketSinkFactory {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StageFactory for WebSocketSinkFactory {
    async fn create_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn PipelineStage>> {
        let endpoint = params.get("endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or("ws://filtered_data")
            .to_string();

        let fields = params.get("fields")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_else(|| vec!["timestamp".to_string(), "filtered_channels".to_string()]);

        let format = params.get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("json")
            .to_string();

        let stage = WebSocketSink::new(endpoint, fields, format);
        
        // Validate parameters
        stage.validate_params(params)?;

        Ok(Box::new(stage))
    }

    fn stage_type(&self) -> &'static str {
        "websocket_sink"
    }

    fn parameter_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "endpoint": {
                    "type": "string",
                    "description": "WebSocket endpoint/topic",
                    "default": "ws://filtered_data"
                },
                "fields": {
                    "type": "array",
                    "description": "Data fields to include in output",
                    "items": {
                        "type": "string",
                        "enum": ["timestamp", "frame_id", "raw_channels", "voltage_channels", "filtered_channels", "sample_rate", "channel_count"]
                    },
                    "default": ["timestamp", "filtered_channels"]
                },
                "format": {
                    "type": "string",
                    "description": "Output format",
                    "enum": ["json", "binary"],
                    "default": "json"
                }
            },
            "required": ["endpoint", "fields", "format"]
        })
    }
}

impl Default for WebSocketSinkFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_websocket_sink_creation() {
        let factory = WebSocketSinkFactory::new();
        let mut params = HashMap::new();
        params.insert("endpoint".to_string(), json!("ws://test"));
        params.insert("fields".to_string(), json!(["timestamp", "filtered_channels"]));
        params.insert("format".to_string(), json!("json"));

        let stage = factory.create_stage(&params).await.unwrap();
        assert_eq!(stage.stage_type(), "websocket_sink");
    }

    #[tokio::test]
    async fn test_websocket_sink_validation() {
        let factory = WebSocketSinkFactory::new();
        
        // Test invalid format
        let mut params = HashMap::new();
        params.insert("format".to_string(), json!("invalid"));
        assert!(factory.create_stage(&params).await.is_err());

        // Test empty fields
        params.clear();
        params.insert("fields".to_string(), json!([]));
        assert!(factory.create_stage(&params).await.is_err());
    }

    #[test]
    fn test_parameter_schema() {
        let factory = WebSocketSinkFactory::new();
        let schema = factory.parameter_schema();
        assert!(schema.is_object());
        assert!(schema["properties"]["endpoint"].is_object());
        assert!(schema["properties"]["fields"].is_object());
        assert!(schema["properties"]["format"].is_object());
    }
}