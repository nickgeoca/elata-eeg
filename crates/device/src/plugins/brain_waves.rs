//! Brain Waves FFT Analysis Plugin
//!
//! This plugin receives raw EEG data and performs FFT analysis to extract
//! brain wave frequencies (Delta, Theta, Alpha, Beta, Gamma).

use std::collections::VecDeque;
use std::sync::Arc;
use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use anyhow::Result;
use tracing::{info, warn, debug};
use rustfft::{FftPlanner, num_complex::Complex};

use eeg_types::{
    SensorEvent, EegPacket, FftPacket, BrainWaves, FftConfig, EventFilter,
    plugin::{EegPlugin, PluginConfig}
};

/// Configuration for the Brain Waves FFT plugin
#[derive(Debug, Clone)]
pub struct BrainWavesConfig {
    /// FFT window size (must be power of 2)
    pub fft_size: usize,
    /// Sample rate in Hz
    pub sample_rate: f32,
    /// Number of channels to process
    pub num_channels: usize,
    /// Window function to apply
    pub window_function: String,
}

impl Default for BrainWavesConfig {
    fn default() -> Self {
        Self {
            fft_size: 512,
            sample_rate: 500.0,
            num_channels: 8,
            window_function: "hanning".to_string(),
        }
    }
}

impl PluginConfig for BrainWavesConfig {
    fn validate(&self) -> Result<()> {
        if !self.fft_size.is_power_of_two() {
            return Err(anyhow::anyhow!("FFT size must be a power of 2"));
        }
        if self.sample_rate <= 0.0 {
            return Err(anyhow::anyhow!("Sample rate must be positive"));
        }
        if self.num_channels == 0 {
            return Err(anyhow::anyhow!("Number of channels must be positive"));
        }
        Ok(())
    }
    
    fn config_name(&self) -> &str {
        "brain_waves_fft"
    }
}

/// Brain Waves FFT Analysis Plugin
#[derive(Clone)]
pub struct BrainWavesPlugin {
    config: BrainWavesConfig,
    analyzer: Option<BrainWaveAnalyzer>,
}

impl BrainWavesPlugin {
    pub fn new() -> Self {
        Self {
            config: BrainWavesConfig::default(),
            analyzer: None,
        }
    }
}

#[async_trait]
impl EegPlugin for BrainWavesPlugin {
    fn name(&self) -> &'static str {
        "brain_waves_fft"
    }

    fn clone_box(&self) -> Box<dyn EegPlugin> {
        Box::new(self.clone())
    }
    
    fn version(&self) -> &'static str {
        "1.0.0"
    }
    
    fn description(&self) -> &'static str {
        "FFT analysis plugin for extracting brain wave frequencies"
    }
    
    fn event_filter(&self) -> Vec<EventFilter> {
        vec![EventFilter::RawEegOnly]
    }
    
    async fn initialize(&mut self) -> Result<()> {
        info!("Initializing Brain Waves FFT plugin");
        self.config.validate()?;
        self.analyzer = Some(BrainWaveAnalyzer::new(
            self.config.fft_size,
            self.config.sample_rate,
            self.config.num_channels,
        ));
        Ok(())
    }
    
    async fn run(
        &mut self,
        bus: Arc<dyn eeg_types::plugin::EventBus>,
        mut receiver: broadcast::Receiver<SensorEvent>,
        shutdown_token: CancellationToken,
    ) -> Result<()> {
        info!("Brain Waves FFT plugin starting");
        
        let analyzer = self.analyzer.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Analyzer not initialized"))?;
        
        let mut events_processed = 0u64;
        
        loop {
            tokio::select! {
                biased; // Prioritize shutdown
                _ = shutdown_token.cancelled() => {
                    info!("Brain Waves FFT plugin received shutdown signal");
                    break;
                }
                event_result = receiver.recv() => {
                    match event_result {
                        Ok(SensorEvent::RawEeg(eeg_packet)) => {
                            debug!("Processing EEG packet with frame_id: {}", eeg_packet.frame_id);
                            
                            // Process the EEG data through FFT analysis
                            if let Some(fft_result) = analyzer.process_eeg_packet(&eeg_packet).await {
                                // Create FFT packet and broadcast it
                                let fft_event = SensorEvent::Fft(Arc::new(fft_result));
                                bus.broadcast(fft_event).await;
                                
                                events_processed += 1;
                                if events_processed % 100 == 0 {
                                    debug!("Brain Waves FFT plugin processed {} events", events_processed);
                                }
                            }
                        }
                        Ok(_) => {
                            // Ignore other event types (shouldn't happen due to filter)
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("Brain Waves FFT plugin lagged by {} messages", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            warn!("Brain Waves FFT plugin receiver channel closed");
                            break;
                        }
                    }
                }
            }
        }
        
        info!("Brain Waves FFT plugin stopped after processing {} events", events_processed);
        Ok(())
    }
}

/// Brain wave analyzer with FFT processing
struct BrainWaveAnalyzer {
    fft_size: usize,
    sample_rate: f32,
    channel_buffers: Vec<VecDeque<f32>>,
    fft_planner: FftPlanner<f32>,
    fft_config: FftConfig,
}

impl Clone for BrainWaveAnalyzer {
    fn clone(&self) -> Self {
        Self {
            fft_size: self.fft_size,
            sample_rate: self.sample_rate,
            channel_buffers: self.channel_buffers.clone(),
            fft_planner: FftPlanner::new(),
            fft_config: self.fft_config.clone(),
        }
    }
}

impl BrainWaveAnalyzer {
    fn new(fft_size: usize, sample_rate: f32, num_channels: usize) -> Self {
        let mut channel_buffers = Vec::new();
        for _ in 0..num_channels {
            channel_buffers.push(VecDeque::with_capacity(fft_size * 2));
        }
        
        let fft_config = FftConfig::new(fft_size, sample_rate, "hanning".to_string());
        
        Self {
            fft_size,
            sample_rate,
            channel_buffers,
            fft_planner: FftPlanner::new(),
            fft_config,
        }
    }
    
    /// Process new EEG data and return FFT analysis if enough data is available
    async fn process_eeg_packet(&mut self, eeg_packet: &EegPacket) -> Option<FftPacket> {
        // Extract samples per channel
        let samples_per_channel = eeg_packet.voltage_samples.len() / eeg_packet.channel_count;
        
        // Add new samples to channel buffers
        for channel_idx in 0..eeg_packet.channel_count.min(self.channel_buffers.len()) {
            let channel_buffer = &mut self.channel_buffers[channel_idx];
            
            // Extract samples for this channel
            for sample_idx in 0..samples_per_channel {
                let data_idx = sample_idx * eeg_packet.channel_count + channel_idx;
                if data_idx < eeg_packet.voltage_samples.len() {
                    channel_buffer.push_back(eeg_packet.voltage_samples[data_idx]);
                    
                    // Keep buffer size manageable
                    if channel_buffer.len() > self.fft_size * 2 {
                        channel_buffer.pop_front();
                    }
                }
            }
        }
        
        // Check if we have enough data for FFT analysis
        let min_buffer_size = self.channel_buffers.iter()
            .map(|buf| buf.len())
            .min()
            .unwrap_or(0);
            
        if min_buffer_size >= self.fft_size {
            Some(self.analyze_brain_waves(eeg_packet.timestamps.first().cloned().unwrap_or(0), eeg_packet.frame_id))
        } else {
            None
        }
    }
    
    /// Perform FFT analysis and extract brain wave frequencies
    fn analyze_brain_waves(&mut self, timestamp: u64, source_frame_id: u64) -> FftPacket {
        let mut brain_waves = Vec::new();
        
        // Process each channel individually to avoid borrowing conflicts
        for channel_idx in 0..self.channel_buffers.len() {
            if self.channel_buffers[channel_idx].len() >= self.fft_size {
                let brain_wave = self.analyze_channel(channel_idx);
                brain_waves.push(brain_wave);
            }
        }
        
        FftPacket::new(
            timestamp,
            source_frame_id,
            brain_waves,
            self.fft_config.clone(),
        )
    }
    
    /// Analyze a single channel's data
    fn analyze_channel(&mut self, channel_idx: usize) -> BrainWaves {
        let buffer = &self.channel_buffers[channel_idx];
        // Extract the most recent fft_size samples
        let start_idx = buffer.len() - self.fft_size;
        let samples: Vec<f32> = buffer.iter().skip(start_idx).cloned().collect();
        
        // Apply Hanning window to reduce spectral leakage
        let windowed_samples: Vec<Complex<f32>> = samples
            .iter()
            .enumerate()
            .map(|(i, &sample)| {
                let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (self.fft_size - 1) as f32).cos());
                Complex::new(sample * window, 0.0)
            })
            .collect();
        
        // Perform FFT
        let fft = self.fft_planner.plan_fft_forward(self.fft_size);
        let mut fft_buffer = windowed_samples;
        fft.process(&mut fft_buffer);
        
        // Calculate power spectral density
        let psd: Vec<f32> = fft_buffer
            .iter()
            .take(self.fft_size / 2) // Only use positive frequencies
            .map(|c| c.norm_sqr())
            .collect();
        
        // Extract brain wave bands
        let freq_resolution = self.sample_rate / self.fft_size as f32;
        
        let delta = self.extract_band_power(&psd, 0.5, 4.0, freq_resolution);
        let theta = self.extract_band_power(&psd, 4.0, 8.0, freq_resolution);
        let alpha = self.extract_band_power(&psd, 8.0, 13.0, freq_resolution);
        let beta = self.extract_band_power(&psd, 13.0, 30.0, freq_resolution);
        let gamma = self.extract_band_power(&psd, 30.0, 100.0, freq_resolution);
        
        BrainWaves::new(channel_idx, delta, theta, alpha, beta, gamma)
    }
    
    /// Extract power in a specific frequency band
    fn extract_band_power(&self, psd: &[f32], low_freq: f32, high_freq: f32, freq_resolution: f32) -> f32 {
        let low_bin = (low_freq / freq_resolution) as usize;
        let high_bin = ((high_freq / freq_resolution) as usize).min(psd.len() - 1);
        
        if low_bin >= high_bin {
            return 0.0;
        }
        
        psd[low_bin..=high_bin].iter().sum::<f32>() / (high_bin - low_bin + 1) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_brain_waves_config_validation() {
        let mut config = BrainWavesConfig::default();
        assert!(config.validate().is_ok());
        
        config.fft_size = 500; // Not a power of 2
        assert!(config.validate().is_err());
        
        config.fft_size = 512;
        config.sample_rate = -1.0;
        assert!(config.validate().is_err());
    }
    
    #[tokio::test]
    async fn test_brain_wave_analyzer() {
        let mut analyzer = BrainWaveAnalyzer::new(256, 500.0, 2);
        
        // Create test EEG packet
        let raw_samples = vec![10, 20, 30, 40]; // 2 channels, 2 samples each
        let voltage_samples: Vec<f32> = raw_samples.iter().map(|&s| s as f32 * 0.1).collect();
        let timestamps = vec![1000, 1002, 1000, 1002];
        let eeg_packet = EegPacket::new(timestamps, 1, raw_samples, voltage_samples, 2, 500.0);
        
        // Should return None initially (not enough data)
        let result = analyzer.process_eeg_packet(&eeg_packet).await;
        assert!(result.is_none());
    }
}