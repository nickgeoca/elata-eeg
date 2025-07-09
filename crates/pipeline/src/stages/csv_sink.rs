//! CSV sink stage for recording data to files

use async_trait::async_trait;
use serde_json::json;
use std::any::Any;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, error, warn};

use eeg_types::{EegPacket, FilteredEegPacket};
use crate::data::{PipelineData, CsvData};
use crate::error::{PipelineError, PipelineResult};
use crate::stage::{PipelineStage, StageFactory, StageParams, StageMetric};

/// CSV sink stage for recording data to files
pub struct CsvSink {
    /// Output file path
    path: PathBuf,
    /// Data fields to include in CSV
    fields: Vec<String>,
    /// Whether to include headers
    include_headers: bool,
    /// Metrics
    packets_written: u64,
    bytes_written: u64,
    /// File handle (placeholder - would be actual file in real implementation)
    file_initialized: bool,
}

impl CsvSink {
    /// Create a new CSV sink
    pub fn new(path: PathBuf, fields: Vec<String>, include_headers: bool) -> Self {
        Self {
            path,
            fields,
            include_headers,
            packets_written: 0,
            bytes_written: 0,
            file_initialized: false,
        }
    }

    /// Initialize the CSV file (placeholder implementation)
    async fn initialize_file(&mut self) -> PipelineResult<()> {
        if self.file_initialized {
            return Ok(());
        }

        // TODO: Implement actual file creation and header writing
        // For now, just log what we would do
        info!("Would create CSV file: {:?}", self.path);
        
        if self.include_headers {
            let header_line = self.fields.join(",");
            info!("Would write CSV headers: {}", header_line);
            self.bytes_written += header_line.len() as u64 + 1; // +1 for newline
        }

        self.file_initialized = true;
        Ok(())
    }

    /// Format CSV data into a row string
    fn format_csv_row(&self, csv_data: &crate::data::CsvData) -> PipelineResult<String> {
        let mut values = Vec::new();
        
        for field in &self.fields {
            match field.as_str() {
                "timestamp" => {
                    values.push(csv_data.timestamp.to_string());
                }
                "frame_id" => {
                    values.push(csv_data.frame_id.to_string());
                }
                "raw_channels" | "voltage_channels" | "filtered_channels" => {
                    // Convert channel data to comma-separated values in brackets
                    let channel_str = format!("[{}]",
                        csv_data.channels.iter()
                            .flat_map(|ch| &ch.samples)
                            .map(|v| format!("{:.6}", v))
                            .collect::<Vec<_>>()
                            .join(";"));
                    values.push(format!("\"{}\"", channel_str)); // Quote to handle commas
                }
                "sample_rate" => {
                    values.push(csv_data.sample_rate.to_string());
                }
                _ => {
                    values.push("".to_string()); // Unknown field
                }
            }
        }
        
        Ok(values.join(","))
    }

    /// Convert packet to CSV row (legacy method - kept for compatibility)
    fn packet_to_csv_row(&self, packet: &dyn Any) -> PipelineResult<String> {
        let mut values = Vec::new();

        // Try to downcast to different packet types
        if let Some(eeg_packet) = packet.downcast_ref::<EegPacket>() {
            for field in &self.fields {
                match field.as_str() {
                    "timestamp" => {
                        if let Some(&first_ts) = eeg_packet.timestamps.first() {
                            values.push(first_ts.to_string());
                        } else {
                            values.push("".to_string());
                        }
                    }
                    "frame_id" => {
                        values.push(eeg_packet.frame_id.to_string());
                    }
                    "raw_channels" => {
                        // Convert array to comma-separated values in brackets
                        let raw_str = format!("[{}]", 
                            eeg_packet.raw_samples.iter()
                                .map(|v| v.to_string())
                                .collect::<Vec<_>>()
                                .join(";"));
                        values.push(format!("\"{}\"", raw_str)); // Quote to handle commas
                    }
                    "voltage_channels" => {
                        let voltage_str = format!("[{}]", 
                            eeg_packet.voltage_samples.iter()
                                .map(|v| format!("{:.6}", v))
                                .collect::<Vec<_>>()
                                .join(";"));
                        values.push(format!("\"{}\"", voltage_str));
                    }
                    "sample_rate" => {
                        values.push(eeg_packet.sample_rate.to_string());
                    }
                    "channel_count" => {
                        values.push(eeg_packet.channel_count.to_string());
                    }
                    _ => {
                        values.push("".to_string()); // Unknown field
                    }
                }
            }
        } else if let Some(filtered_packet) = packet.downcast_ref::<FilteredEegPacket>() {
            for field in &self.fields {
                match field.as_str() {
                    "timestamp" => {
                        if let Some(&first_ts) = filtered_packet.timestamps.first() {
                            values.push(first_ts.to_string());
                        } else {
                            values.push("".to_string());
                        }
                    }
                    "frame_id" => {
                        values.push(filtered_packet.frame_id.to_string());
                    }
                    "filtered_channels" => {
                        let filtered_str = format!("[{}]", 
                            filtered_packet.samples.iter()
                                .map(|v| format!("{:.6}", v))
                                .collect::<Vec<_>>()
                                .join(";"));
                        values.push(format!("\"{}\"", filtered_str));
                    }
                    "sample_rate" => {
                        values.push(filtered_packet.sample_rate.to_string());
                    }
                    "channel_count" => {
                        values.push(filtered_packet.channel_count.to_string());
                    }
                    _ => {
                        values.push("".to_string()); // Unknown field
                    }
                }
            }
        } else {
            return Err(PipelineError::RuntimeError {
                stage_name: "csv_sink".to_string(),
                message: "Unsupported packet type for CSV formatting".to_string(),
            });
        }

        Ok(values.join(","))
    }

    /// Write CSV row to file (placeholder implementation)
    async fn write_csv_row(&mut self, row: String) -> PipelineResult<()> {
        // TODO: Implement actual file writing
        // For now, just log what we would write
        info!("Would write CSV row to {:?}: {}", self.path, 
              if row.len() > 100 { format!("{}...", &row[..100]) } else { row.clone() });
        
        self.packets_written += 1;
        self.bytes_written += row.len() as u64 + 1; // +1 for newline
        
        Ok(())
    }
}

#[async_trait]
impl PipelineStage for CsvSink {
    async fn process(&mut self, input: PipelineData) -> PipelineResult<PipelineData> {
        // Initialize file if needed
        self.initialize_file().await?;
        
        // Convert packet to CSV row based on data type
        let csv_row = match &input {
            PipelineData::RawEeg(packet) => {
                let csv_data = crate::data::CsvData::from_eeg_packet(packet, &self.fields);
                self.format_csv_row(&csv_data)?
            }
            PipelineData::FilteredEeg(packet) => {
                let csv_data = crate::data::CsvData::from_filtered_packet(packet, &self.fields);
                self.format_csv_row(&csv_data)?
            }
            _ => return Err(PipelineError::RuntimeError {
                stage_name: "csv_sink".to_string(),
                message: "CSV sink only supports RawEeg and FilteredEeg data".to_string(),
            }),
        };
        
        // Write to file
        self.write_csv_row(csv_row).await?;
        
        // CSV sinks are terminal nodes, so we return the input unchanged
        // (though in practice, nothing should be connected to the output)
        Ok(input)
    }

    fn stage_type(&self) -> &'static str {
        "csv_sink"
    }

    fn description(&self) -> &'static str {
        "CSV sink for recording data to files"
    }

    async fn initialize(&mut self) -> PipelineResult<()> {
        info!("Initializing CSV sink: path={:?}, fields={:?}, headers={}", 
               self.path, self.fields, self.include_headers);
        Ok(())
    }

    async fn cleanup(&mut self) -> PipelineResult<()> {
        info!("Cleaning up CSV sink, wrote {} packets ({} bytes) to {:?}", 
               self.packets_written, self.bytes_written, self.path);
        // TODO: Close file handle
        Ok(())
    }

    fn get_metrics(&self) -> Vec<StageMetric> {
        vec![
            StageMetric::new(
                "packets_written".to_string(),
                self.packets_written as f64,
                "count".to_string(),
            ),
            StageMetric::new(
                "bytes_written".to_string(),
                self.bytes_written as f64,
                "bytes".to_string(),
            ),
        ]
    }

    fn validate_params(&self, params: &StageParams) -> PipelineResult<()> {
        // Validate path
        if let Some(path) = params.get("path") {
            let path = path.as_str().ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "path parameter must be a string".to_string(),
            })?;
            
            if path.is_empty() {
                return Err(PipelineError::InvalidConfiguration {
                    message: "path cannot be empty".to_string(),
                });
            }

            // Check if path has valid extension
            if !path.ends_with(".csv") {
                return Err(PipelineError::InvalidConfiguration {
                    message: "path must end with .csv extension".to_string(),
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

/// Factory for creating CSV sink stages
pub struct CsvSinkFactory;

impl CsvSinkFactory {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StageFactory for CsvSinkFactory {
    async fn create_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn PipelineStage>> {
        let path = params.get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("output.csv")
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

        let include_headers = params.get("include_headers")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let stage = CsvSink::new(PathBuf::from(path), fields, include_headers);
        
        // Validate parameters
        stage.validate_params(params)?;

        Ok(Box::new(stage))
    }

    fn stage_type(&self) -> &'static str {
        "csv_sink"
    }

    fn parameter_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Output CSV file path",
                    "pattern": ".*\\.csv$",
                    "default": "output.csv"
                },
                "fields": {
                    "type": "array",
                    "description": "Data fields to include in CSV",
                    "items": {
                        "type": "string",
                        "enum": ["timestamp", "frame_id", "raw_channels", "voltage_channels", "filtered_channels", "sample_rate", "channel_count"]
                    },
                    "default": ["timestamp", "filtered_channels"]
                },
                "include_headers": {
                    "type": "boolean",
                    "description": "Whether to include column headers",
                    "default": true
                }
            },
            "required": ["path", "fields"]
        })
    }
}

impl Default for CsvSinkFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_csv_sink_creation() {
        let factory = CsvSinkFactory::new();
        let mut params = HashMap::new();
        params.insert("path".to_string(), json!("test.csv"));
        params.insert("fields".to_string(), json!(["timestamp", "filtered_channels"]));
        params.insert("include_headers".to_string(), json!(true));

        let stage = factory.create_stage(&params).await.unwrap();
        assert_eq!(stage.stage_type(), "csv_sink");
    }

    #[tokio::test]
    async fn test_csv_sink_validation() {
        let factory = CsvSinkFactory::new();
        
        // Test invalid path extension
        let mut params = HashMap::new();
        params.insert("path".to_string(), json!("test.txt"));
        assert!(factory.create_stage(&params).await.is_err());

        // Test empty fields
        params.clear();
        params.insert("path".to_string(), json!("test.csv"));
        params.insert("fields".to_string(), json!([]));
        assert!(factory.create_stage(&params).await.is_err());
    }

    #[tokio::test]
    async fn test_csv_row_formatting() {
        let sink = CsvSink::new(
            PathBuf::from("test.csv"),
            vec!["timestamp".to_string(), "frame_id".to_string()],
            true,
        );
        
        // Create test EEG packet
        let timestamps = vec![1000, 1001, 1002];
        let raw_samples = vec![100, 200, 300];
        let voltage_samples = vec![1.0, 2.0, 3.0];
        
        let packet = EegPacket::new(
            timestamps,
            42,
            raw_samples,
            voltage_samples,
            1,
            250.0,
        );

        let csv_row = sink.packet_to_csv_row(&packet).unwrap();
        assert!(csv_row.contains("1000")); // timestamp
        assert!(csv_row.contains("42"));   // frame_id
    }

    #[test]
    fn test_parameter_schema() {
        let factory = CsvSinkFactory::new();
        let schema = factory.parameter_schema();
        assert!(schema.is_object());
        assert!(schema["properties"]["path"].is_object());
        assert!(schema["properties"]["fields"].is_object());
        assert!(schema["properties"]["include_headers"].is_object());
    }
}