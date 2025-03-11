use eeg_driver::{AdcConfig, ProcessedData};
use std::sync::Arc;
use std::fs::File;
use std::io::{self, Write};
use chrono::{Local, DateTime};
use csv::Writer;
use std::time::Instant;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::config::DaemonConfig;

#[derive(Clone, Serialize)]
pub struct EegData {
    pub channels: Vec<f32>,
    pub timestamp: u64,
}

#[derive(Clone, Serialize)]
pub struct EegBatchData {
    pub channels: Vec<Vec<f32>>,  // Each inner Vec represents a channel's data for the batch
    pub timestamp: u64,           // Timestamp for the start of the batch
}

/// Structure to hold CSV recording state
pub struct CsvRecorder {
    writer: Option<Writer<File>>,
    pub file_path: Option<String>,
    pub is_recording: bool,
    start_timestamp: Option<u64>,
    sample_rate: u32,
    last_flush_time: Instant,
    recording_start_time: Option<Instant>,
    config: Arc<DaemonConfig>,
    current_adc_config: AdcConfig,
}

impl CsvRecorder {
    pub fn new(sample_rate: u32, config: Arc<DaemonConfig>, adc_config: AdcConfig) -> Self {
        Self {
            writer: None,
            file_path: None,
            is_recording: false,
            start_timestamp: None,
            sample_rate,
            last_flush_time: Instant::now(),
            recording_start_time: None,
            config,
            current_adc_config: adc_config,
        }
    }
    
    /// Start recording to a new CSV file
    pub fn start_recording(&mut self) -> io::Result<String> {
        if self.is_recording {
            return Ok(format!("Already recording to {}", self.file_path.clone().unwrap_or_default()));
        }
        
        // Create recordings directory if it doesn't exist
        std::fs::create_dir_all(&self.config.recordings_directory)?;
        
        // Create filename with current timestamp and parameters
        let now: DateTime<Local> = Local::now();
        let gain = self.current_adc_config.gain;
        let driver = format!("{:?}", self.current_adc_config.board_driver);
        
        let filename = format!(
            "{}/{}_gain{}_board{}.csv",
            self.config.recordings_directory,
            now.format("%Y-%m-%d_%H-%M"),
            gain,
            driver
        );
        
        // Create CSV writer
        let file = File::create(&filename)?;
        let mut writer = csv::Writer::from_writer(file);
        
        // Write header row
        writer.write_record(&["timestamp", "channel_1", "channel_2", "channel_3", "channel_4"])?;
        writer.flush()?;
        
        self.writer = Some(writer);
        self.file_path = Some(filename.clone());
        self.is_recording = true;
        self.start_timestamp = None; // Will be set when first data arrives
        self.recording_start_time = Some(Instant::now());
        
        Ok(format!("Started recording to {}", filename))
    }
    
    /// Stop recording and close the CSV file
    pub fn stop_recording(&mut self) -> io::Result<String> {
        if !self.is_recording {
            return Ok("Not currently recording".to_string());
        }
        
        if let Some(mut writer) = self.writer.take() {
            writer.flush()?;
        }
        
        let file_path = self.file_path.take().unwrap_or_default();
        self.is_recording = false;
        self.start_timestamp = None;
        
        Ok(format!("Stopped recording to {}", file_path))
    }
    
    /// Write a batch of EEG data to the CSV file
    pub fn write_data(&mut self, data: &EegBatchData) -> io::Result<String> {
        if !self.is_recording || self.writer.is_none() {
            return Ok("Not recording".to_string());
        }
        
        // Set start timestamp if this is the first data
        if self.start_timestamp.is_none() {
            self.start_timestamp = Some(data.timestamp);
        }
        
        let writer = self.writer.as_mut().unwrap();
        let num_channels = data.channels.len().min(4); // Limit to 4 channels
        let samples_per_channel = data.channels[0].len();
        
        // Calculate microseconds per sample based on sample rate
        let us_per_sample = 1_000_000 / self.sample_rate as u64;
        
        // Write each sample as a row
        for i in 0..samples_per_channel {
            let sample_timestamp = data.timestamp + (i as u64 * us_per_sample);
            
            // Create a record with timestamp and channel values
            let mut record = Vec::with_capacity(num_channels + 1);
            record.push(sample_timestamp.to_string());
            
            for ch in 0..num_channels {
                record.push(data.channels[ch][i].to_string());
            }
            
            // Pad with zeros if we have fewer than 4 channels
            for _ in num_channels..4 {
                record.push("0.0".to_string());
            }
            
            writer.write_record(&record)?;
        }
        
        // Check if it's time to flush (hardcoded to 5 seconds)
        let now = Instant::now();
        if now.duration_since(self.last_flush_time).as_secs() >= 5 {
            writer.flush()?;
            self.last_flush_time = now;
        }
        
        // Check if we've exceeded the maximum recording length
        if let Some(start_time) = self.recording_start_time {
            if now.duration_since(start_time).as_secs() >= (self.config.max_recording_length_minutes * 60) as u64 {
                // Stop current recording and start a new one
                let old_file = self.stop_recording()?;
                let new_file = self.start_recording()?;
                return Ok(format!("Maximum recording length reached. Stopped recording to {} and started new recording to {}", old_file, new_file));
            }
        }
        
        Ok("Data written successfully".to_string())
    }
}

// Function to process EEG data batches
pub async fn process_eeg_data(
    mut data_rx: tokio::sync::mpsc::Receiver<ProcessedData>,
    tx: tokio::sync::broadcast::Sender<EegBatchData>,
    csv_recorder: Arc<Mutex<CsvRecorder>>,
) {
    let mut count = 0;
    let mut last_time = std::time::Instant::now();
    let mut last_timestamp = None;
    
    while let Some(data) = data_rx.recv().await {
        // Create smaller batches to send more frequently
        // Split the incoming data into chunks of 32 samples
        let batch_size = 32;
        let num_channels = data.data.len();
        let samples_per_channel = data.data[0].len();
        
        for chunk_start in (0..samples_per_channel).step_by(batch_size) {
            let chunk_end = (chunk_start + batch_size).min(samples_per_channel);
            let mut chunk_channels = Vec::with_capacity(num_channels);
            
            for channel in &data.data {
                chunk_channels.push(channel[chunk_start..chunk_end].to_vec());
            }
            
            let chunk_timestamp = data.timestamp + (chunk_start as u64 * 4000); // Adjust timestamp for each chunk
            
            let eeg_batch_data = EegBatchData {
                channels: chunk_channels,
                timestamp: chunk_timestamp / 1000, // Convert to milliseconds
            };
            
            // Write to CSV if recording is active
            if let Ok(mut recorder) = csv_recorder.try_lock() {
                match recorder.write_data(&eeg_batch_data) {
                    Ok(msg) => {
                        // Only log if something interesting happened (like auto-rotating files)
                        if msg != "Data written successfully" && msg != "Not recording" {
                            println!("CSV Recording: {}", msg);
                        }
                    },
                    Err(e) => {
                        println!("Warning: Failed to write data to CSV: {}", e);
                    }
                }
            }
            
            if let Err(e) = tx.send(eeg_batch_data) {
                println!("Warning: Failed to send data chunk to WebSocket clients: {}", e);
            }
        }
        
        count += data.data[0].len();
        last_timestamp = Some(data.timestamp);
        
        if let Some(last_ts) = last_timestamp {
            let delta_us = data.timestamp - last_ts;
            let delta_ms = delta_us as f64 / 1000.0;  // Convert to milliseconds for display
            if delta_ms > 5.0 {
                println!("Large timestamp gap detected: {:.2}ms ({} µs)", delta_ms, delta_us);
                println!("Sample count: {}", count);
                println!("Expected time between batches: {:.2}ms", (32_000.0 / 250.0)); // For 32 samples at 250Hz
            }
        }
        
        // Print stats every 250 samples (about 1 second of data at 250Hz)
        if count % 250 == 0 {
            let elapsed = last_time.elapsed();
            let rate = 250.0 / elapsed.as_secs_f32();
            println!("Processing rate: {:.2} Hz", rate);
            println!("Total samples processed: {}", count);
            println!("Sample data (first 5 values from first channel):");
            println!("  Channel 0: {:?}", &data.data[0][..5]);
            last_time = std::time::Instant::now();
        }
    }
}