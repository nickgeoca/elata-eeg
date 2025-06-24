use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, warn, error, debug};

use eeg_types::{
    event::{SensorEvent, EegPacket, FilteredEegPacket},
    plugin::{EegPlugin, PluginConfig},
    event::EventFilter,
    config::DaemonConfig,
};
use basic_voltage_filter::SignalProcessor;

/// Configuration for the Basic Voltage Filter Plugin
#[derive(Clone, Debug)]
pub struct BasicVoltageFilterConfig {
    pub daemon_config: Arc<DaemonConfig>,
    pub sample_rate: u32,
    pub num_channels: usize,
}

impl PluginConfig for BasicVoltageFilterConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.num_channels == 0 {
            return Err(anyhow::anyhow!("Number of channels must be greater than 0"));
        }
        if self.sample_rate == 0 {
            return Err(anyhow::anyhow!("Sample rate must be greater than 0"));
        }
        Ok(())
    }
    
    fn config_name(&self) -> &str {
        "basic_voltage_filter_config"
    }
}

/// Basic Voltage Filter Plugin - applies DSP filtering to raw EEG data
pub struct BasicVoltageFilterPlugin {
    config: BasicVoltageFilterConfig,
    signal_processor: SignalProcessor,
}

impl BasicVoltageFilterPlugin {
    pub fn new(config: BasicVoltageFilterConfig) -> Self {
        let signal_processor = SignalProcessor::new(
            config.sample_rate,
            config.num_channels,
            config.daemon_config.filter_config.dsp_high_pass_cutoff_hz,
            config.daemon_config.filter_config.dsp_low_pass_cutoff_hz,
            config.daemon_config.filter_config.powerline_filter_hz,
        );

        Self {
            config,
            signal_processor,
        }
    }

    /// Process EEG packet through the signal processor
    async fn process_eeg_packet(&mut self, packet: &EegPacket) -> Result<FilteredEegPacket> {
        // Convert Arc<[f32]> to Vec<Vec<f32>> format expected by SignalProcessor
        let samples_per_channel = packet.samples.len() / self.config.num_channels;
        let mut channel_samples = vec![vec![0.0; samples_per_channel]; self.config.num_channels];
        
        // Reshape flat samples array into per-channel format
        for (sample_idx, &sample) in packet.samples.iter().enumerate() {
            let channel_idx = sample_idx / samples_per_channel;
            let sample_in_channel = sample_idx % samples_per_channel;
            if channel_idx < self.config.num_channels && sample_in_channel < samples_per_channel {
                channel_samples[channel_idx][sample_in_channel] = sample;
            }
        }

        // Process each channel through the signal processor
        for (channel_idx, channel_data) in channel_samples.iter_mut().enumerate() {
            if channel_idx < self.config.num_channels {
                // Create a copy of input for processing
                let input_samples = channel_data.clone();
                match self.signal_processor.process_chunk(
                    channel_idx, 
                    &input_samples, 
                    channel_data.as_mut_slice()
                ) {
                    Ok(_) => {
                        debug!("[basic_voltage_filter] Successfully processed channel {}", channel_idx);
                    }
                    Err(e) => {
                        error!("[basic_voltage_filter] Error processing channel {}: {}", channel_idx, e);
                        // Keep original data on error
                        *channel_data = input_samples;
                    }
                }
            }
        }

        // Convert back to flat Arc<[f32]> format
        let mut filtered_samples = Vec::with_capacity(packet.samples.len());
        for sample_idx in 0..samples_per_channel {
            for channel_idx in 0..self.config.num_channels {
                if sample_idx < channel_samples[channel_idx].len() {
                    filtered_samples.push(channel_samples[channel_idx][sample_idx]);
                } else {
                    filtered_samples.push(0.0);
                }
            }
        }

        Ok(FilteredEegPacket {
            timestamp: packet.timestamps.first().cloned().unwrap_or(0),
            source_frame_id: packet.frame_id,
            filtered_samples: filtered_samples.into(),
            channel_count: self.config.num_channels,
            filter_type: "basic_voltage".to_string(),
        })
    }
}

#[async_trait]
impl EegPlugin for BasicVoltageFilterPlugin {
    fn name(&self) -> &'static str {
        "basic_voltage_filter"
    }
    
    fn description(&self) -> &'static str {
        "Applies DSP filtering (high-pass, low-pass, powerline) to raw EEG data"
    }
    
    fn event_filter(&self) -> Vec<EventFilter> {
        vec![EventFilter::RawEegOnly]
    }

    async fn run(
        &self,
        bus: Arc<dyn std::any::Any + Send + Sync>,
        mut receiver: tokio::sync::mpsc::Receiver<SensorEvent>,
        shutdown_token: CancellationToken,
    ) -> Result<()> {
        info!("[{}] Starting basic voltage filter plugin", self.name());
        
        // Create a mutable copy for processing state
        let mut filter = BasicVoltageFilterPlugin::new(self.config.clone());
        
        loop {
            tokio::select! {
                biased; // Prioritize shutdown
                _ = shutdown_token.cancelled() => {
                    info!("[{}] Received shutdown signal", self.name());
                    break;
                }
                Some(event) = receiver.recv() => {
                    match event {
                        SensorEvent::RawEeg(packet) => {
                            debug!("[{}] Processing raw EEG packet with frame_id: {}", 
                                   self.name(), packet.frame_id);
                            
                            match filter.process_eeg_packet(&packet).await {
                                Ok(filtered_packet) => {
                                    // Publish filtered data back to the event bus
                                    let filtered_event = SensorEvent::FilteredEeg(Arc::new(filtered_packet));
                                    // TODO: Cast bus back to EventBus trait and broadcast
                                    // For now, just log that we would publish
                                    debug!("[{}] Successfully processed filtered EEG data (would publish to bus)",
                                           self.name());
                                }
                                Err(e) => {
                                    error!("[{}] Failed to process EEG packet: {}", self.name(), e);
                                }
                            }
                        }
                        _ => {
                            // Ignore other event types
                            debug!("[{}] Ignoring non-RawEeg event", self.name());
                        }
                    }
                }
            }
        }
        
        info!("[{}] Basic voltage filter plugin stopped", self.name());
        Ok(())
    }
}