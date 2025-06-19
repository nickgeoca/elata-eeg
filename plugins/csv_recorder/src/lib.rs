use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::fs::File;
use std::io;
use chrono::{Local, DateTime};
use csv::Writer;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use async_trait::async_trait;
use anyhow::Result;
use tracing::{info, warn, error, debug};

use eeg_types::{
    event::{SensorEvent, EegPacket},
    plugin::{EegPlugin, PluginConfig},
    event::EventFilter,
    config::DaemonConfig,
};
use eeg_sensor::AdcConfig;

/// Configuration for the CSV Recorder Plugin
#[derive(Clone, Debug)]
pub struct CsvRecorderConfig {
    pub daemon_config: Arc<DaemonConfig>,
    pub adc_config: AdcConfig,
    pub is_recording_shared: Arc<AtomicBool>,
}

impl PluginConfig for CsvRecorderConfig {
    fn validate(&self) -> anyhow::Result<()> {
        if self.adc_config.channels.is_empty() {
            return Err(anyhow::anyhow!("ADC config must have at least one channel"));
        }
        if self.adc_config.sample_rate == 0 {
            return Err(anyhow::anyhow!("Sample rate must be greater than 0"));
        }
        Ok(())
    }
    
    fn config_name(&self) -> &str {
        "csv_recorder_config"
    }
}

/// CSV Recorder Plugin - handles recording EEG data to CSV files
pub struct CsvRecorderPlugin {
    config: CsvRecorderConfig,
    writer: Option<Writer<File>>,
    file_path: Option<String>,
    is_recording: bool,
    start_timestamp: Option<u64>,
    last_flush_time: Instant,
    recording_start_time: Option<Instant>,
}

impl CsvRecorderPlugin {
    pub fn new(config: CsvRecorderConfig) -> Self {
        Self {
            config,
            writer: None,
            file_path: None,
            is_recording: false,
            start_timestamp: None,
            last_flush_time: Instant::now(),
            recording_start_time: None,
        }
    }

    /// Start recording to a new CSV file
    pub async fn start_recording(&mut self) -> io::Result<String> {
        if self.is_recording {
            return Ok(format!("Already recording to {}", self.file_path.clone().unwrap_or_default()));
        }
        
        // Update shared recording state
        self.config.is_recording_shared.store(true, Ordering::Relaxed);
        
        debug!("Recordings directory from config: {}", self.config.daemon_config.recordings_directory);
        
        // Get absolute path of the current directory
        let current_dir = match std::env::current_dir() {
            Ok(dir) => dir,
            Err(e) => {
                error!("Failed to get current directory: {}", e);
                return Err(io::Error::new(io::ErrorKind::Other, "Failed to get current directory"));
            }
        };
        debug!("Current directory: {:?}", current_dir);
        
        // Determine if we're running from the daemon directory
        let is_in_daemon_dir = current_dir.to_string_lossy().ends_with("/daemon");
        debug!("Running from daemon directory: {}", is_in_daemon_dir);
        
        // Adjust the path if needed to ensure it's relative to the repo root
        let recordings_dir = if is_in_daemon_dir && self.config.daemon_config.recordings_directory.starts_with("./") {
            // Convert "./recordings/" to "../recordings/" when running from daemon directory
            let adjusted_path = format!("..{}", &self.config.daemon_config.recordings_directory[1..]);
            debug!("Adjusted recordings path: {}", adjusted_path);
            adjusted_path
        } else {
            self.config.daemon_config.recordings_directory.clone()
        };
        
        // Debug: Print the absolute path of the recordings directory
        let absolute_recordings_path = current_dir.join(&recordings_dir);
        debug!("Absolute recordings path: {:?}", absolute_recordings_path);
        
        // Create recordings directory if it doesn't exist
        std::fs::create_dir_all(&recordings_dir)?;
        
        // Create filename with current timestamp and parameters
        let now: DateTime<Local> = Local::now();
        let driver = format!("{:?}", self.config.adc_config.board_driver);
        
        // Get session field from config
        let session_prefix = if self.config.daemon_config.session.is_empty() {
            "".to_string()
        } else {
            format!("session{}_", self.config.daemon_config.session)
        };

        let filename = format!(
            "{}/{}{}_board{}.csv",
            recordings_dir,
            session_prefix,
            now.format("%Y-%m-%d_%H-%M"),
            driver,
        );
        debug!("Creating recording file at: {}", filename);
        
        // Create CSV writer
        let file = File::create(&filename)?;
        let mut writer = csv::Writer::from_writer(file);
        
        // Write header row with both voltage and raw samples
        let mut header = vec!["timestamp".to_string()];
        
        // Add voltage channel headers using the actual channel indices
        for &channel_idx in &self.config.adc_config.channels {
            header.push(format!("ch{}_voltage", channel_idx));
        }
        
        // Add raw channel headers using the actual channel indices
        for &channel_idx in &self.config.adc_config.channels {
            header.push(format!("ch{}_raw_sample", channel_idx));
        }
        
        writer.write_record(&header)?;
        writer.flush()?;
        
        self.writer = Some(writer);
        self.file_path = Some(filename.clone());
        self.is_recording = true;
        self.start_timestamp = None; // Will be set when first data arrives
        self.recording_start_time = Some(Instant::now());
        
        info!("Started recording to {}", filename);
        Ok(format!("Started recording to {}", filename))
    }
    
    /// Stop recording and close the CSV file
    pub async fn stop_recording(&mut self) -> io::Result<String> {
        if !self.is_recording {
            return Ok("Not currently recording".to_string());
        }
        
        if let Some(mut writer) = self.writer.take() {
            writer.flush()?;
        }
        
        let file_path = self.file_path.take().unwrap_or_default();
        self.is_recording = false;
        self.start_timestamp = None;
        
        // Update shared recording state
        self.config.is_recording_shared.store(false, Ordering::Relaxed);
        
        info!("Stopped recording to {}", file_path);
        Ok(format!("Stopped recording to {}", file_path))
    }
    
    /// Write EEG packet data to the CSV file
    async fn write_eeg_packet(&mut self, packet: &EegPacket) -> io::Result<String> {
        if !self.is_recording || self.writer.is_none() {
            return Ok("Not recording".to_string());
        }
        
        // Set start timestamp if this is the first data
        if self.start_timestamp.is_none() {
            self.start_timestamp = Some(packet.timestamp);
        }
        
        let writer = self.writer.as_mut().unwrap();
        let samples_per_channel = packet.samples.len() / self.config.adc_config.channels.len();
        let num_channels = self.config.adc_config.channels.len();
        
        // Calculate sample rate from ADC config
        let sample_rate = self.config.adc_config.sample_rate;
        let us_per_sample = 1_000_000 / sample_rate as u64;
        
        // Write each sample as a row
        for i in 0..samples_per_channel {
            let sample_timestamp = packet.timestamp + (i as u64 * us_per_sample);
            
            // Create a record with timestamp, voltage values, and raw values
            let mut record = Vec::with_capacity(1 + num_channels * 2);
            record.push(sample_timestamp.to_string());
            
            // Add voltage values for each configured channel
            for channel_idx in 0..num_channels {
                let sample_idx = channel_idx * samples_per_channel + i;
                if sample_idx < packet.samples.len() {
                    record.push(packet.samples[sample_idx].to_string());
                } else {
                    record.push("0.0".to_string());
                }
            }
            
            // Add raw values (for now, convert voltage back to approximate raw)
            // TODO: This should be actual raw samples when available in EegPacket
            for channel_idx in 0..num_channels {
                let sample_idx = channel_idx * samples_per_channel + i;
                if sample_idx < packet.samples.len() {
                    // Approximate raw value (this is a placeholder)
                    let raw_approx = (packet.samples[sample_idx] * 1000.0) as i32;
                    record.push(raw_approx.to_string());
                } else {
                    record.push("0".to_string());
                }
            }
            
            writer.write_record(&record)?;
        }
        
        // Check if it's time to flush (every 5 seconds)
        let now = Instant::now();
        if now.duration_since(self.last_flush_time).as_secs() >= 5 {
            writer.flush()?;
            self.last_flush_time = now;
        }
        
        // Check if we've exceeded the maximum recording length
        if let Some(start_time) = self.recording_start_time {
            if now.duration_since(start_time).as_secs() >= (self.config.daemon_config.max_recording_length_minutes * 60) as u64 {
                let old_file = self.stop_recording().await?;
                let new_file = self.start_recording().await?;
                return Ok(format!("Maximum recording length reached. {} and {}", old_file, new_file));
            }
        }
        
        Ok("Data written successfully".to_string())
    }
}

#[async_trait]
impl EegPlugin for CsvRecorderPlugin {
    fn name(&self) -> &'static str {
        "csv_recorder"
    }
    
    fn description(&self) -> &'static str {
        "Records EEG data to CSV files with automatic rotation"
    }
    
    fn event_filter(&self) -> Vec<EventFilter> {
        vec![EventFilter::RawEegOnly]
    }

    async fn run(
        &self,
        _bus: Arc<dyn std::any::Any + Send + Sync>,
        mut receiver: tokio::sync::mpsc::Receiver<SensorEvent>,
        shutdown_token: CancellationToken,
    ) -> Result<()> {
        info!("[{}] Starting CSV recorder plugin", self.name());
        
        // Create a mutable copy of self for state management
        let mut recorder = CsvRecorderPlugin::new(self.config.clone());
        
        loop {
            tokio::select! {
                biased; // Prioritize shutdown
                _ = shutdown_token.cancelled() => {
                    info!("[{}] Received shutdown signal", self.name());
                    if recorder.is_recording {
                        if let Err(e) = recorder.stop_recording().await {
                            error!("[{}] Error stopping recording during shutdown: {}", self.name(), e);
                        }
                    }
                    break;
                }
                Some(event) = receiver.recv() => {
                    match event {
                        SensorEvent::RawEeg(packet) => {
                            // Only record if recording is enabled
                            if recorder.config.is_recording_shared.load(Ordering::Relaxed) {
                                if !recorder.is_recording {
                                    if let Err(e) = recorder.start_recording().await {
                                        error!("[{}] Failed to start recording: {}", self.name(), e);
                                        continue;
                                    }
                                }
                                
                                match recorder.write_eeg_packet(&packet).await {
                                    Ok(msg) => {
                                        if msg != "Data written successfully" && msg != "Not recording" {
                                            info!("[{}] {}", self.name(), msg);
                                        }
                                    },
                                    Err(e) => {
                                        warn!("[{}] Failed to write data: {}", self.name(), e);
                                    }
                                }
                            } else if recorder.is_recording {
                                // Stop recording if it was disabled
                                if let Err(e) = recorder.stop_recording().await {
                                    error!("[{}] Error stopping recording: {}", self.name(), e);
                                }
                            }
                        }
                        _ => {
                            // Ignore other event types
                        }
                    }
                }
            }
        }
        
        info!("[{}] CSV recorder plugin stopped", self.name());
        Ok(())
    }
}