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

/// FFT analysis result packet containing brain wave frequency data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FftPacket {
    /// Timestamp when the FFT analysis was completed
    pub timestamp: u64,
    /// Reference to the original frame that was analyzed
    pub source_frame_id: u64,
    /// Brain wave frequency bands for each channel
    #[serde(with = "serde_arc_slice")]
    pub brain_waves: Arc<[BrainWaves]>,
    /// FFT configuration used for this analysis
    pub fft_config: FftConfig,
}

/// Brain wave frequency bands for a single channel
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BrainWaves {
    /// Channel index
    pub channel: usize,
    /// Delta band power (0.5-4 Hz)
    pub delta: f32,
    /// Theta band power (4-8 Hz)
    pub theta: f32,
    /// Alpha band power (8-13 Hz)
    pub alpha: f32,
    /// Beta band power (13-30 Hz)
    pub beta: f32,
    /// Gamma band power (30-100 Hz)
    pub gamma: f32,
}

/// FFT analysis configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FftConfig {
    /// FFT window size (must be power of 2)
    pub fft_size: usize,
    /// Sample rate used for analysis
    pub sample_rate: f32,
    /// Window function applied
    pub window_function: String,
}

// Helper module for serializing Arc<[T]>
mod serde_arc_slice {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<S, T>(arc_slice: &Arc<[T]>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        arc_slice.as_ref().serialize(serializer)
    }

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Arc<[T]>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        let vec = Vec::<T>::deserialize(deserializer)?;
        Ok(vec.into())
    }
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
    /// FFT analysis results with brain wave data
    Fft(Arc<FftPacket>),
    /// System status and control events
    System(Arc<SystemEvent>),
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    // Sensor-level events
    RawEeg(Arc<EegPacket>),
    FilteredEeg(Arc<FilteredEegPacket>),
    Fft(Arc<FftPacket>),
    System(Arc<SystemEvent>),
    // Add other app-level events here if needed
}

/// Event filter types for plugins to specify what events they want to receive
#[derive(Debug, Clone, PartialEq)]
pub enum EventFilter {
    /// Receive all events
    All,
    /// Only raw EEG events
    RawEegOnly,
    /// Only filtered EEG events
    FilteredEegOnly,
    /// Only FFT analysis events
    FftOnly,
    /// Only system events
    SystemOnly,
}

/// Helper function to check if an event matches a filter
pub fn event_matches_filter(event: &SensorEvent, filter: &EventFilter) -> bool {
    match filter {
        EventFilter::All => true,
        EventFilter::RawEegOnly => matches!(event, SensorEvent::RawEeg(_)),
        EventFilter::FilteredEegOnly => matches!(event, SensorEvent::FilteredEeg(_)),
        EventFilter::FftOnly => matches!(event, SensorEvent::Fft(_)),
        EventFilter::SystemOnly => matches!(event, SensorEvent::System(_)),
    }
}

impl SensorEvent {
    /// Get the timestamp of any event type
    pub fn timestamp(&self) -> u64 {
        match self {
            SensorEvent::RawEeg(packet) => packet.timestamp,
            SensorEvent::FilteredEeg(packet) => packet.timestamp,
            SensorEvent::Fft(packet) => packet.timestamp,
            SensorEvent::System(event) => event.timestamp,
        }
    }

    /// Get a human-readable description of the event type
    pub fn event_type_name(&self) -> &'static str {
        match self {
            SensorEvent::RawEeg(_) => "RawEeg",
            SensorEvent::FilteredEeg(_) => "FilteredEeg",
            SensorEvent::Fft(_) => "Fft",
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

    pub fn to_binary(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        // Simplified binary format: [timestamp_u64_le][error_flag_u8][data_payload]
        buffer.extend_from_slice(&self.timestamp.to_le_bytes());
        buffer.push(0); // No error
        for &sample in self.samples.iter() {
            buffer.extend_from_slice(&sample.to_le_bytes());
        }
        buffer
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

    pub fn to_binary(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        // Simplified binary format: [timestamp_u64_le][error_flag_u8][data_payload]
        buffer.extend_from_slice(&self.timestamp.to_le_bytes());
        buffer.push(0); // No error
        for &sample in self.filtered_samples.iter() {
            buffer.extend_from_slice(&sample.to_le_bytes());
        }
        buffer
    }
}

impl FftPacket {
    /// Create a new FFT packet
    pub fn new(
        timestamp: u64,
        source_frame_id: u64,
        brain_waves: Vec<BrainWaves>,
        fft_config: FftConfig,
    ) -> Self {
        Self {
            timestamp,
            source_frame_id,
            brain_waves: brain_waves.into(),
            fft_config,
        }
    }

    /// Get brain waves for a specific channel
    pub fn channel_brain_waves(&self, channel: usize) -> Option<&BrainWaves> {
        self.brain_waves.iter().find(|bw| bw.channel == channel)
    }


    /// Convert to binary format for efficient transmission
    pub fn to_binary(&self) -> Vec<u8> {
        let mut buffer = Vec::new();
        // Binary format: [timestamp_u64_le][source_frame_id_u64_le][num_channels_u32_le][brain_wave_data]
        buffer.extend_from_slice(&self.timestamp.to_le_bytes());
        buffer.extend_from_slice(&self.source_frame_id.to_le_bytes());
        buffer.extend_from_slice(&(self.brain_waves.len() as u32).to_le_bytes());
        
        for brain_wave in self.brain_waves.iter() {
            buffer.extend_from_slice(&(brain_wave.channel as u32).to_le_bytes());
            buffer.extend_from_slice(&brain_wave.delta.to_le_bytes());
            buffer.extend_from_slice(&brain_wave.theta.to_le_bytes());
            buffer.extend_from_slice(&brain_wave.alpha.to_le_bytes());
            buffer.extend_from_slice(&brain_wave.beta.to_le_bytes());
            buffer.extend_from_slice(&brain_wave.gamma.to_le_bytes());
        }
        
        buffer
    }
}

impl BrainWaves {
    /// Create new brain waves data for a channel
    pub fn new(channel: usize, delta: f32, theta: f32, alpha: f32, beta: f32, gamma: f32) -> Self {
        Self {
            channel,
            delta,
            theta,
            alpha,
            beta,
            gamma,
        }
    }

    /// Get the dominant frequency band
    pub fn dominant_band(&self) -> (&'static str, f32) {
        let bands = [
            ("delta", self.delta),
            ("theta", self.theta),
            ("alpha", self.alpha),
            ("beta", self.beta),
            ("gamma", self.gamma),
        ];
        
        bands.iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(name, value)| (*name, *value))
            .unwrap_or(("unknown", 0.0))
    }
}

impl FftConfig {
    /// Create a new FFT configuration
    pub fn new(fft_size: usize, sample_rate: f32, window_function: String) -> Self {
        Self {
            fft_size,
            sample_rate,
            window_function,
        }
    }

    /// Get frequency resolution for this configuration
    pub fn frequency_resolution(&self) -> f32 {
        self.sample_rate / self.fft_size as f32
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