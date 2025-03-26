use serde::{Serialize, Deserialize};
use std::sync::Arc;

/// Configuration for the daemon
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Maximum recording length in minutes before starting a new file
    pub max_recording_length_minutes: u32,
    /// Directory where recordings are stored
    pub recordings_directory: String,
    /// Session identifier for recordings
    pub session: String,
    /// High-pass filter cutoff frequency in Hz
    pub dsp_high_pass_cutoff_hz: f32,
    /// Low-pass filter cutoff frequency in Hz
    pub dsp_low_pass_cutoff_hz: f32,
    /// Batch size for processing data
    pub batch_size: usize,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            max_recording_length_minutes: 60,
            recordings_directory: "../recordings/".to_string(),
            session: "".to_string(),
            dsp_high_pass_cutoff_hz: 0.1,  // Default value from current implementation
            dsp_low_pass_cutoff_hz: 100.0, // Default value from current implementation
            batch_size: 32,
        }
    }
}

/// Load daemon configuration from file or create default if not found
pub fn load_config() -> Arc<DaemonConfig> {
    // Try to load from file
    let config_path = "../config.json";
    match std::fs::read_to_string(config_path) {
        Ok(contents) => {
            match serde_json::from_str(&contents) {
                Ok(config) => {
                    println!("Loaded configuration from {}", config_path);
                    Arc::new(config)
                },
                Err(e) => {
                    println!("Error parsing configuration file: {}. Using defaults.", e);
                    let default_config = DaemonConfig::default();
                    
                    // Create default config file for future use
                    if let Ok(json) = serde_json::to_string_pretty(&default_config) {
                        let _ = std::fs::write(config_path, json);
                        println!("Created default configuration file at {}", config_path);
                    }
                    
                    Arc::new(default_config)
                }
            }
        },
        Err(_) => {
            println!("Configuration file not found. Using defaults.");
            let default_config = DaemonConfig::default();
            
            // Create default config file for future use
            if let Ok(json) = serde_json::to_string_pretty(&default_config) {
                let _ = std::fs::write(config_path, json);
                println!("Created default configuration file at {}", config_path);
            }
            
            Arc::new(default_config)
        }
    }
}