//! Brain Waves Display Plugin Backend
//!
//! This plugin receives raw ADC data from the device daemon and performs
//! FFT analysis to extract brain wave frequencies (Delta, Theta, Alpha, Beta, Gamma).

use clap::Parser;
use log::{info, warn, error, debug};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use tokio::sync::broadcast;
use warp::Filter;
use rustfft::{FftPlanner, num_complex::Complex};

/// Command line arguments
#[derive(Parser, Debug)]
#[command(name = "brain_waves_backend")]
#[command(about = "Brain waves analysis plugin backend")]
struct Args {
    /// Port for data input from device daemon
    #[arg(long, default_value = "8081")]
    data_port: u16,
    
    /// Port for WebSocket output to kiosk
    #[arg(long, default_value = "8082")]
    output_port: u16,
    
    /// FFT window size (must be power of 2)
    #[arg(long, default_value = "512")]
    fft_size: usize,
    
    /// Sample rate (Hz)
    #[arg(long, default_value = "500")]
    sample_rate: u32,
}

/// Raw ADC data structure (matches the device daemon's AdcData)
#[derive(Debug, Clone, Deserialize, Serialize)]
struct AdcData {
    timestamp: u64,
    raw_samples: Vec<Vec<i32>>,
    voltage_samples: Vec<Vec<f32>>,
}

/// Brain wave frequency bands
#[derive(Debug, Clone, Serialize)]
struct BrainWaves {
    timestamp: u64,
    channel: usize,
    delta: f32,   // 0.5-4 Hz
    theta: f32,   // 4-8 Hz
    alpha: f32,   // 8-13 Hz
    beta: f32,    // 13-30 Hz
    gamma: f32,   // 30-100 Hz
}

/// FFT analysis result for all channels
#[derive(Debug, Clone, Serialize)]
struct AnalysisResult {
    timestamp: u64,
    brain_waves: Vec<BrainWaves>,
}

/// Brain wave analyzer with FFT processing
struct BrainWaveAnalyzer {
    fft_size: usize,
    sample_rate: u32,
    channel_buffers: Vec<VecDeque<f32>>,
    fft_planner: FftPlanner<f32>,
}

impl BrainWaveAnalyzer {
    fn new(fft_size: usize, sample_rate: u32, num_channels: usize) -> Self {
        let mut channel_buffers = Vec::new();
        for _ in 0..num_channels {
            channel_buffers.push(VecDeque::with_capacity(fft_size * 2));
        }
        
        Self {
            fft_size,
            sample_rate,
            channel_buffers,
            fft_planner: FftPlanner::new(),
        }
    }
    
    /// Process new ADC data and return brain wave analysis if enough data is available
    fn process_data(&mut self, adc_data: AdcData) -> Option<AnalysisResult> {
        // Add new samples to channel buffers
        for (channel_idx, channel_data) in adc_data.voltage_samples.iter().enumerate() {
            if channel_idx < self.channel_buffers.len() {
                for &sample in channel_data {
                    self.channel_buffers[channel_idx].push_back(sample);
                    
                    // Keep buffer size manageable
                    if self.channel_buffers[channel_idx].len() > self.fft_size * 2 {
                        self.channel_buffers[channel_idx].pop_front();
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
            Some(self.analyze_brain_waves(adc_data.timestamp))
        } else {
            None
        }
    }
    
    /// Perform FFT analysis and extract brain wave frequencies
    fn analyze_brain_waves(&mut self, timestamp: u64) -> AnalysisResult {
        let mut brain_waves = Vec::new();
        
        for (channel_idx, buffer) in self.channel_buffers.iter().enumerate() {
            if buffer.len() >= self.fft_size {
                let brain_wave = self.analyze_channel(channel_idx, buffer, timestamp);
                brain_waves.push(brain_wave);
            }
        }
        
        AnalysisResult {
            timestamp,
            brain_waves,
        }
    }
    
    /// Analyze a single channel's data
    fn analyze_channel(&mut self, channel_idx: usize, buffer: &VecDeque<f32>, timestamp: u64) -> BrainWaves {
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
        let freq_resolution = self.sample_rate as f32 / self.fft_size as f32;
        
        let delta = self.extract_band_power(&psd, 0.5, 4.0, freq_resolution);
        let theta = self.extract_band_power(&psd, 4.0, 8.0, freq_resolution);
        let alpha = self.extract_band_power(&psd, 8.0, 13.0, freq_resolution);
        let beta = self.extract_band_power(&psd, 13.0, 30.0, freq_resolution);
        let gamma = self.extract_band_power(&psd, 30.0, 100.0, freq_resolution);
        
        BrainWaves {
            timestamp,
            channel: channel_idx,
            delta,
            theta,
            alpha,
            beta,
            gamma,
        }
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    env_logger::init();
    let args = Args::parse();
    
    info!("Brain Waves Backend starting...");
    info!("Data input port: {}", args.data_port);
    info!("WebSocket output port: {}", args.output_port);
    info!("FFT size: {}", args.fft_size);
    info!("Sample rate: {} Hz", args.sample_rate);
    
    // Create broadcast channel for analysis results
    let (analysis_tx, _) = broadcast::channel::<AnalysisResult>(100);
    let analysis_tx_clone = analysis_tx.clone();
    
    // Initialize brain wave analyzer
    let mut analyzer = BrainWaveAnalyzer::new(args.fft_size, args.sample_rate, 8); // Assume max 8 channels
    
    // Data input endpoint (receives AdcData from device daemon)
    let data_input = warp::path("data")
        .and(warp::post())
        .and(warp::body::json())
        .map(move |adc_data: AdcData| {
            debug!("Received AdcData with timestamp: {}", adc_data.timestamp);
            
            // For now, just log the data reception
            // In a full implementation, this would process the data through the analyzer
            info!("Processing AdcData: channels={}, samples_per_channel={}", 
                  adc_data.voltage_samples.len(),
                  adc_data.voltage_samples.first().map(|ch| ch.len()).unwrap_or(0));
            
            warp::reply::with_status("OK", warp::http::StatusCode::OK)
        });
    
    // WebSocket endpoint for sending analysis results to kiosk
    let analysis_rx = analysis_tx.subscribe();
    let websocket = warp::path("analysis")
        .and(warp::ws())
        .map(move |ws: warp::ws::Ws| {
            let mut rx = analysis_tx.subscribe();
            ws.on_upgrade(move |websocket| async move {
                info!("WebSocket client connected for analysis data");
                
                let (ws_tx, mut ws_rx) = websocket.split();
                let (tx, mut rx_internal) = tokio::sync::mpsc::unbounded_channel();
                
                // Forward analysis results to WebSocket
                tokio::spawn(async move {
                    while let Ok(result) = rx.recv().await {
                        if let Ok(json) = serde_json::to_string(&result) {
                            if tx.send(Ok(warp::ws::Message::text(json))).is_err() {
                                break;
                            }
                        }
                    }
                });
                
                // Handle WebSocket messages
                let ws_send_task = tokio::spawn(async move {
                    use futures_util::SinkExt;
                    let mut ws_tx = ws_tx;
                    while let Some(msg) = rx_internal.recv().await {
                        if ws_tx.send(msg.unwrap()).await.is_err() {
                            break;
                        }
                    }
                });
                
                ws_send_task.await.ok();
                info!("WebSocket client disconnected");
            })
        });
    
    let routes = data_input.or(websocket);
    
    info!("Starting HTTP server on port {}", args.data_port);
    warp::serve(routes)
        .run(([127, 0, 0, 1], args.data_port))
        .await;
    
    Ok(())
}