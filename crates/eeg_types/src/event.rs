//! Event system types for the EEG daemon
//! 
//! This module defines the core data structures and events used in the event-driven
//! architecture. All large data payloads use Arc<[T]> for zero-copy sharing between
//! plugins while maintaining data integrity through frame IDs.

use std::sync::Arc;

/// Raw EEG data packet from the sensor hardware
#[derive(Debug, Clone)]
pub struct EegPacket {
    /// Timestamp when the data was acquired (milliseconds since Unix epoch)
    pub timestamp: u64,
    /// Monotonic frame counter for detecting data gaps
    pub frame_id: u64,
    /// EEG voltage samples for all channels - using Arc<[f32]> for zero-copy sharing
    pub samples: Arc<[f32]>,
    /// Number of channels in this packet
    pub channel_count: usize,
    /// Sample rate in Hz
    pub sample_rate: f32,
}

/// Filtered EEG data packet after processing
#[derive(Debug, Clone)]
pub struct FilteredEegPacket {
    /// Timestamp when the filtering was completed
    pub timestamp: u64,
    /// Reference to the original frame that was filtered
    pub source_frame_id: u64,
    /// Filtered voltage samples - using Arc<[f32]> for zero-copy sharing
    pub filtered_samples: Arc<[f32]>,
    /// Number of channels in this packet
    pub channel_count: usize,
    /// Filter type applied (e.g., "basic_voltage", "bandpass_8_30hz")
    pub filter_type: String,
}

/// System status and control events
#[derive(Debug, Clone)]
pub struct SystemEvent {
    /// Timestamp of the event
    pub timestamp: u64,
    /// Event type and data
    pub event_type: SystemEventType,
}

/// Types of system events
#[derive(Debug, Clone)]
pub enum SystemEventType {
    /// Recording started with filename
    RecordingStarted(String),
    /// Recording stopped
    RecordingStopped,
    /// Configuration changed
    ConfigurationChanged,
    /// Plugin error occurred
    PluginError { plugin_name: String, error: String },
    /// System shutdown initiated
    ShutdownInitiated,
}

/// Main event enum that wraps all event types in Arc for efficient sharing
#[derive(Debug, Clone)]
pub enum SensorEvent {
    /// Raw EEG data from hardware
    RawEeg(Arc<EegPacket>),
    /// Filtered EEG data from processing plugins
    FilteredEeg(Arc<FilteredEegPacket>),
    /// System status and control events
    System(Arc<SystemEvent>),
}

impl SensorEvent {
    /// Get the timestamp of any event type
    pub fn timestamp(&self) -> u64 {
        match self {
            SensorEvent::RawEeg(packet) => packet.timestamp,
            SensorEvent::FilteredEeg(packet) => packet.timestamp,
            SensorEvent::System(event) => event.timestamp,
        }
    }

    /// Get a human-readable description of the event type
    pub fn event_type_name(&self) -> &'static str {
        match self {
            SensorEvent::RawEeg(_) => "RawEeg",
            SensorEvent::FilteredEeg(_) => "FilteredEeg",
            SensorEvent::System(_) => "System",
        }
    }
}

impl EegPacket {
    /// Create a new EEG packet with the given parameters
    pub fn new(
        timestamp: u64,
        frame_id: u64,
        samples: Vec<f32>,
        channel_count: usize,
        sample_rate: f32,
    ) -> Self {
        Self {
            timestamp,
            frame_id,
            samples: samples.into(),
            channel_count,
            sample_rate,
        }
    }

    /// Get samples for a specific channel
    pub fn channel_samples(&self, channel: usize) -> Option<&[f32]> {
        if channel >= self.channel_count {
            return None;
        }
        
        let samples_per_channel = self.samples.len() / self.channel_count;
        let start = channel * samples_per_channel;
        let end = start + samples_per_channel;
        
        self.samples.get(start..end)
    }
}

impl FilteredEegPacket {
    /// Create a new filtered EEG packet
    pub fn new(
        timestamp: u64,
        source_frame_id: u64,
        filtered_samples: Vec<f32>,
        channel_count: usize,
        filter_type: String,
    ) -> Self {
        Self {
            timestamp,
            source_frame_id,
            filtered_samples: filtered_samples.into(),
            channel_count,
            filter_type,
        }
    }

    /// Get filtered samples for a specific channel
    pub fn channel_samples(&self, channel: usize) -> Option<&[f32]> {
        if channel >= self.channel_count {
            return None;
        }
        
        let samples_per_channel = self.filtered_samples.len() / self.channel_count;
        let start = channel * samples_per_channel;
        let end = start + samples_per_channel;
        
        self.filtered_samples.get(start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eeg_packet_creation() {
        let samples = vec![1.0, 2.0, 3.0, 4.0]; // 2 channels, 2 samples each
        let packet = EegPacket::new(1000, 1, samples, 2, 250.0);
        
        assert_eq!(packet.timestamp, 1000);
        assert_eq!(packet.frame_id, 1);
        assert_eq!(packet.channel_count, 2);
        assert_eq!(packet.sample_rate, 250.0);
        assert_eq!(packet.samples.len(), 4);
    }

    #[test]
    fn test_channel_samples() {
        let samples = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]; // 3 channels, 2 samples each
        let packet = EegPacket::new(1000, 1, samples, 3, 250.0);
        
        assert_eq!(packet.channel_samples(0), Some([1.0, 2.0].as_slice()));
        assert_eq!(packet.channel_samples(1), Some([3.0, 4.0].as_slice()));
        assert_eq!(packet.channel_samples(2), Some([5.0, 6.0].as_slice()));
        assert_eq!(packet.channel_samples(3), None);
    }

    #[test]
    fn test_sensor_event_timestamp() {
        let eeg_packet = Arc::new(EegPacket::new(1000, 1, vec![1.0], 1, 250.0));
        let event = SensorEvent::RawEeg(eeg_packet);
        
        assert_eq!(event.timestamp(), 1000);
        assert_eq!(event.event_type_name(), "RawEeg");
    }
}