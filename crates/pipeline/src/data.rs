//! Data types for pipeline communication
//! 
//! This module defines the type-safe data structures used for communication
//! between pipeline stages, replacing the unsafe `Box<dyn Any>` approach.

use std::sync::Arc;
use serde::{Serialize, Deserialize};
use eeg_types::{EegPacket, FilteredEegPacket, FftPacket};

/// Pipeline data that can flow between stages
#[derive(Debug, Clone)]
pub enum PipelineData {
    /// Raw EEG data from acquisition
    RawEeg(Arc<EegPacket>),
    /// Filtered EEG data
    FilteredEeg(Arc<FilteredEegPacket>),
    /// FFT analysis results
    Fft(Arc<FftPacket>),
    /// Trigger signal for source stages (no data payload)
    Trigger,
    /// CSV record command with data
    CsvRecord {
        data: CsvData,
        file_path: String,
    },
    /// WebSocket broadcast command with data
    WebSocketBroadcast {
        data: WebSocketData,
        endpoint: String,
    },
}

/// Data for CSV recording
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvData {
    /// Timestamp
    pub timestamp: u64,
    /// Frame ID for tracking
    pub frame_id: u64,
    /// Channel data (can be raw, voltage, or filtered)
    pub channels: Vec<ChannelData>,
    /// Sample rate
    pub sample_rate: f32,
}

/// Data for a single channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelData {
    /// Channel index
    pub channel: usize,
    /// Sample values
    pub samples: Vec<f32>,
}

/// Data for WebSocket broadcasting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketData {
    /// Timestamp
    pub timestamp: u64,
    /// Frame ID for tracking
    pub frame_id: u64,
    /// Data format (json, binary)
    pub format: String,
    /// Serialized payload
    pub payload: Vec<u8>,
}

impl PipelineData {
    /// Get the timestamp of the data (if applicable)
    pub fn timestamp(&self) -> Option<u64> {
        match self {
            PipelineData::RawEeg(packet) => packet.timestamps.first().copied(),
            PipelineData::FilteredEeg(packet) => packet.timestamps.first().copied(),
            PipelineData::Fft(packet) => Some(packet.timestamp),
            PipelineData::CsvRecord { data, .. } => Some(data.timestamp),
            PipelineData::WebSocketBroadcast { data, .. } => Some(data.timestamp),
            PipelineData::Trigger => None,
        }
    }

    /// Get the frame ID of the data (if applicable)
    pub fn frame_id(&self) -> Option<u64> {
        match self {
            PipelineData::RawEeg(packet) => Some(packet.frame_id),
            PipelineData::FilteredEeg(packet) => Some(packet.frame_id),
            PipelineData::Fft(packet) => Some(packet.source_frame_id),
            PipelineData::CsvRecord { data, .. } => Some(data.frame_id),
            PipelineData::WebSocketBroadcast { data, .. } => Some(data.frame_id),
            PipelineData::Trigger => None,
        }
    }

    /// Get a human-readable description of the data type
    pub fn data_type(&self) -> &'static str {
        match self {
            PipelineData::RawEeg(_) => "RawEeg",
            PipelineData::FilteredEeg(_) => "FilteredEeg",
            PipelineData::Fft(_) => "Fft",
            PipelineData::CsvRecord { .. } => "CsvRecord",
            PipelineData::WebSocketBroadcast { .. } => "WebSocketBroadcast",
            PipelineData::Trigger => "Trigger",
        }
    }
}

impl CsvData {
    /// Create CSV data from an EEG packet
    pub fn from_eeg_packet(packet: &EegPacket, fields: &[String]) -> Self {
        let mut channels = Vec::new();
        
        // Extract requested fields
        for field in fields {
            match field.as_str() {
                "raw_channels" => {
                    for ch in 0..packet.channel_count {
                        if let Some(samples) = packet.channel_raw_samples(ch) {
                            channels.push(ChannelData {
                                channel: ch,
                                samples: samples.iter().map(|&s| s as f32).collect(),
                            });
                        }
                    }
                }
                "voltage_channels" => {
                    for ch in 0..packet.channel_count {
                        if let Some(samples) = packet.channel_voltage_samples(ch) {
                            channels.push(ChannelData {
                                channel: ch,
                                samples: samples.to_vec(),
                            });
                        }
                    }
                }
                _ => {
                    // Skip unknown fields
                }
            }
        }

        Self {
            timestamp: packet.timestamps.first().copied().unwrap_or(0),
            frame_id: packet.frame_id,
            channels,
            sample_rate: packet.sample_rate,
        }
    }

    /// Create CSV data from a filtered EEG packet
    pub fn from_filtered_packet(packet: &FilteredEegPacket, fields: &[String]) -> Self {
        let mut channels = Vec::new();
        
        // Extract requested fields
        for field in fields {
            match field.as_str() {
                "filtered_channels" => {
                    for ch in 0..packet.channel_count {
                        if let Some(samples) = packet.channel_samples(ch) {
                            channels.push(ChannelData {
                                channel: ch,
                                samples: samples.to_vec(),
                            });
                        }
                    }
                }
                _ => {
                    // Skip unknown fields
                }
            }
        }

        Self {
            timestamp: packet.timestamps.first().copied().unwrap_or(0),
            frame_id: packet.frame_id,
            channels,
            sample_rate: packet.sample_rate,
        }
    }
}

impl WebSocketData {
    /// Create WebSocket data from an EEG packet
    pub fn from_eeg_packet(packet: &EegPacket, format: &str) -> Self {
        let payload = match format {
            "binary" => packet.to_binary(),
            "json" => {
                // Create a JSON representation
                let json_data = serde_json::json!({
                    "timestamp": packet.timestamps.first().copied().unwrap_or(0),
                    "frame_id": packet.frame_id,
                    "channel_count": packet.channel_count,
                    "sample_rate": packet.sample_rate,
                    "voltage_samples": packet.voltage_samples.as_ref(),
                });
                serde_json::to_vec(&json_data).unwrap_or_default()
            }
            _ => Vec::new(),
        };

        Self {
            timestamp: packet.timestamps.first().copied().unwrap_or(0),
            frame_id: packet.frame_id,
            format: format.to_string(),
            payload,
        }
    }

    /// Create WebSocket data from a filtered EEG packet
    pub fn from_filtered_packet(packet: &FilteredEegPacket, format: &str) -> Self {
        let payload = match format {
            "binary" => packet.to_binary(),
            "json" => {
                // Create a JSON representation
                let json_data = serde_json::json!({
                    "timestamp": packet.timestamps.first().copied().unwrap_or(0),
                    "frame_id": packet.frame_id,
                    "channel_count": packet.channel_count,
                    "sample_rate": packet.sample_rate,
                    "samples": packet.samples.as_ref(),
                });
                serde_json::to_vec(&json_data).unwrap_or_default()
            }
            _ => Vec::new(),
        };

        Self {
            timestamp: packet.timestamps.first().copied().unwrap_or(0),
            frame_id: packet.frame_id,
            format: format.to_string(),
            payload,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eeg_types::EegPacket;

    #[test]
    fn test_pipeline_data_timestamp() {
        let packet = Arc::new(EegPacket::new(
            vec![1000, 1001],
            42,
            vec![100, 200],
            vec![1.0, 2.0],
            1,
            250.0,
        ));
        
        let data = PipelineData::RawEeg(packet);
        assert_eq!(data.timestamp(), Some(1000));
        assert_eq!(data.frame_id(), Some(42));
        assert_eq!(data.data_type(), "RawEeg");
    }

    #[test]
    fn test_csv_data_from_eeg_packet() {
        let packet = EegPacket::new(
            vec![1000, 1001],
            42,
            vec![100, 200],
            vec![1.0, 2.0],
            1,
            250.0,
        );
        
        let csv_data = CsvData::from_eeg_packet(&packet, &["voltage_channels".to_string()]);
        assert_eq!(csv_data.timestamp, 1000);
        assert_eq!(csv_data.frame_id, 42);
        assert_eq!(csv_data.channels.len(), 1);
        assert_eq!(csv_data.channels[0].samples, vec![1.0, 2.0]);
    }
}