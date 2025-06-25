//! Plugin for calculating Power Spectral Density (PSD) from EEG data using FFT.

use std::sync::Arc;
use async_trait::async_trait;
use tokio::sync::{broadcast, Mutex};
use tokio_util::sync::CancellationToken;
use anyhow::Result;
use rustfft::{Fft, FftPlanner};
use num_complex::Complex;

use eeg_types::plugin::{EegPlugin, EventBus};
use eeg_types::event::{EventFilter, SensorEvent, FftPacket, PsdPacket, FftConfig};

const FFT_SIZE: usize = 512;
const FFT_OVERLAP: usize = 256; // 50% overlap

/// Applies a Hann window to a slice of f32.
/// The window reduces spectral leakage in the FFT.
fn hann_window(samples: &[f32]) -> Vec<f32> {
    let n = samples.len();
    if n == 0 {
        return Vec::new();
    }
    let n_minus_1 = (n - 1) as f32;
    samples
        .iter()
        .enumerate()
        .map(|(i, &sample)| {
            let multiplier = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / n_minus_1).cos());
            sample * multiplier
        })
        .collect()
}

/// Processes EEG data for a single channel.
struct ChannelProcessor {
    channel_id: usize,
    buffer: Vec<f32>,
    fft_planner: Arc<dyn Fft<f32> + Send + Sync>,
    fft_config: FftConfig,
}

impl ChannelProcessor {
    pub fn new(channel_id: usize, sample_rate: f32) -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let fft_config = FftConfig {
            fft_size: FFT_SIZE,
            sample_rate,
            window_function: "Hann".to_string(),
        };

        Self {
            channel_id,
            buffer: Vec::with_capacity(FFT_SIZE * 2),
            fft_planner: fft,
            fft_config,
        }
    }

    /// Processes a chunk of samples and returns a PsdPacket if enough data is available.
    pub fn process_chunk(&mut self, samples: &[f32]) -> Option<PsdPacket> {
        self.buffer.extend_from_slice(samples);

        if self.buffer.len() >= FFT_SIZE {
            let mut fft_buffer: Vec<Complex<f32>> = self.buffer[..FFT_SIZE]
                .iter()
                .map(|&s| Complex::new(s, 0.0))
                .collect();

            let windowed_samples = hann_window(&self.buffer[..FFT_SIZE]);
            for (i, sample) in windowed_samples.iter().enumerate() {
                fft_buffer[i] = Complex::new(*sample, 0.0);
            }

            self.fft_planner.process(&mut fft_buffer);

            let window_norm_factor: f32 = hann_window(&vec![1.0; FFT_SIZE]).iter().map(|&v| v * v).sum::<f32>();
            let psd_norm_factor = 2.0 / (self.fft_config.sample_rate * window_norm_factor);

            let psd: Vec<f32> = fft_buffer[..FFT_SIZE / 2]
                .iter()
                .map(|c| c.norm_sqr() * psd_norm_factor)
                .collect();

            self.buffer.drain(..FFT_OVERLAP);

            return Some(PsdPacket {
                channel: self.channel_id,
                psd,
            });
        }
        None
    }
}

/// A plugin that performs FFT analysis on EEG data.
#[derive(Clone, Default)]
pub struct BrainWavesPlugin {
    state: Arc<Mutex<PluginState>>,
}

#[derive(Default)]
struct PluginState {
    processors: Vec<ChannelProcessor>,
    initialized: bool,
}

impl BrainWavesPlugin {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl EegPlugin for BrainWavesPlugin {
    fn clone_box(&self) -> Box<dyn EegPlugin> {
        Box::new(self.clone())
    }

    fn name(&self) -> &'static str { "brain_waves" }
    fn version(&self) -> &'static str { "0.3.0" }
    fn description(&self) -> &'static str { "Calculates Power Spectral Density (PSD) from EEG data." }
    fn event_filter(&self) -> Vec<EventFilter> { vec![EventFilter::FilteredEegOnly] }

    async fn run(
        &mut self,
        bus: Arc<dyn EventBus>,
        mut receiver: broadcast::Receiver<SensorEvent>,
        shutdown_token: CancellationToken,
    ) -> Result<()> {
        log::info!("Starting brain_waves plugin");

        loop {
            tokio::select! {
                _ = shutdown_token.cancelled() => {
                    log::info!("brain_waves plugin shutting down");
                    break;
                }
                event = receiver.recv() => {
                    match event {
                        Ok(SensorEvent::FilteredEeg(packet)) => {
                            let mut state = self.state.lock().await;

                            if !state.initialized {
                                log::info!(
                                    "Initializing brain_waves processors: count={}, rate={}",
                                    packet.channel_count,
                                    packet.sample_rate
                                );
                                state.processors = (0..packet.channel_count)
                                    .map(|i| ChannelProcessor::new(i, packet.sample_rate))
                                    .collect();
                                state.initialized = true;
                            }

                            if state.processors.len() != packet.channel_count {
                                log::error!("Channel count mismatch. Expected {}, got {}. Re-initializing.",
                                    state.processors.len(), packet.channel_count);
                                state.initialized = false;
                                continue;
                            }

                            let samples_per_channel = packet.samples.len() / packet.channel_count;
                            let mut psd_packets: Vec<PsdPacket> = Vec::with_capacity(packet.channel_count);

                            for i in 0..packet.channel_count {
                                let start = i * samples_per_channel;
                                let end = start + samples_per_channel;
                                let channel_samples = &packet.samples[start..end];

                                if let Some(processor) = state.processors.get_mut(i) {
                                    if let Some(psd_packet) = processor.process_chunk(channel_samples) {
                                        psd_packets.push(psd_packet);
                                    }
                                }
                            }

                            if !psd_packets.is_empty() {
                                let fft_packet = {
                                    let fft_config = state.processors[0].fft_config.clone();
                                    FftPacket::new(
                                        packet.timestamps.first().cloned().unwrap_or(0),
                                        packet.frame_id,
                                        psd_packets,
                                        fft_config,
                                    )
                                };
                                bus.broadcast(SensorEvent::Fft(Arc::new(fft_packet))).await;
                            }
                        }
                        Ok(_) => {} // Ignore other event types
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("brain_waves plugin lagged by {} messages", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            log::info!("Event bus closed, brain_waves plugin shutting down.");
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}