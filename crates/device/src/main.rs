mod config;
mod server;
mod pid_manager;
mod plugin_manager;
mod elata_emu_v1;

use eeg_sensor::{AdcConfig, AdcData};
use crate::elata_emu_v1::EegSystem;
use tokio::sync::{broadcast, Mutex};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::fmt;

// Define a custom error type that implements Send + Sync
#[derive(Debug)]
struct DaemonError(String);

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DaemonError {}

// Helper function to convert any error to our custom error type
fn to_daemon_error<E: std::fmt::Display>(e: E) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(DaemonError(e.to_string()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logger - Reads RUST_LOG environment variable
    env_logger::init();

    // Initialize PID manager to ensure single daemon instance
    let pid_file_path = "/tmp/eeg_daemon.pid";
    let pid_manager = pid_manager::PidManager::new(pid_file_path);
    
    // Check if another instance is already running
    if let Err(e) = pid_manager.acquire_lock() {
        eprintln!("Failed to start daemon: {}", e);
        eprintln!("If you're sure no other instance is running, try removing the PID file: {}", pid_file_path);
        std::process::exit(1);
    }
    
    println!("EEG Daemon starting (PID: {})...", std::process::id());

    // Load daemon configuration
    let daemon_config = config::load_config();
    println!("Daemon configuration:");
    println!("  Max recording length: {} minutes", daemon_config.max_recording_length_minutes);
    println!("  Recordings directory: {}", daemon_config.recordings_directory);
    println!("  Batch size: {}", daemon_config.batch_size);
    println!("  Driver type: {:?}", daemon_config.driver_type);
    
    // Debug: Print current working directory
    match std::env::current_dir() {
        Ok(path) => println!("Current working directory: {:?}", path),
        Err(e) => println!("Failed to get current working directory: {}", e),
    }

    // Channel for raw ADC data to be sent to plugins
    let (_tx_adc_data, _) = broadcast::channel::<AdcData>(32);

    // Create the ADC configuration
    let initial_config = AdcConfig {
        sample_rate: 500, // Should come from config.json
        channels: vec![0, 1, 2], // Should come from config.json
        gain: 24.0,
        board_driver: daemon_config.driver_type.clone(),
        batch_size: daemon_config.batch_size,
        vref: 4.5,
    };
    
    // Create shared state
    let config = Arc::new(Mutex::new(initial_config.clone()));
    let is_recording = Arc::new(AtomicBool::new(false));

    println!("Starting EEG system...");
    
    // Create and start the EEG system
    let (mut eeg_system, mut data_rx) = EegSystem::new(initial_config.clone()).await
        .map_err(to_daemon_error)?;
    
    eeg_system.start(initial_config.clone()).await
        .map_err(to_daemon_error)?;

    println!("EEG system started. Waiting for data...");

    // Initialize PluginManager
    let plugin_manager = match plugin_manager::PluginManager::new().await {
        Ok(pm) => Arc::new(Mutex::new(pm)),
        Err(e) => {
            eprintln!("Failed to initialize PluginManager: {}", e);
            return Err(e);
        }
    };

    // Create a broadcast channel for config updates
    let (config_applied_tx, _) = broadcast::channel::<AdcConfig>(16);

    // Create a broadcast channel for EEG data
    let (eeg_data_tx, _) = broadcast::channel::<Vec<u8>>(32);

    // Broadcast the initial configuration
    if let Err(e) = config_applied_tx.send(initial_config.clone()) {
        println!("Error broadcasting initial config: {}", e);
    }

    // Set up WebSocket routes and get config update channel
    let (ws_routes, mut config_update_rx) = server::setup_websocket_routes(
        config.clone(),
        is_recording.clone(),
        config_applied_tx.clone(),
        eeg_data_tx.clone(),
    );
    
    println!("WebSocket server starting on:");
    println!("- ws://0.0.0.0:8080/config (Configuration)");
    println!("- ws://0.0.0.0:8080/command (Recording control)");
    println!("- ws://0.0.0.0:8080/eeg (EEG data streaming)");

    // Spawn WebSocket server
    let mut server_handle = tokio::spawn(warp::serve(ws_routes).run(([0, 0, 0, 0], 8080)));

    // Spawn task to forward data from EegSystem to the PluginManager and WebSocket clients
    let plugin_manager_clone = plugin_manager.clone();
    let eeg_data_tx_clone = eeg_data_tx.clone();
    let config_clone = config.clone();
    let mut data_forwarding_handle = tokio::spawn(async move {
        while let Some(adc_data) = data_rx.recv().await {
            // Forward to plugin manager
            if let Err(e) = plugin_manager_clone.lock().await.send_data(adc_data.clone()).await {
                eprintln!("Error forwarding data to plugin: {}", e);
            }
            
            // Convert AdcData batch to binary format and broadcast to WebSocket clients
            let current_config = config_clone.lock().await.clone();
            // For now, let's just send individual samples as they come
            // We can batch them later if needed
            let mut buffer = Vec::with_capacity(12); // 4 bytes batch_size + 4 bytes channel + 4 bytes value
            
            // Write batch size of 1
            buffer.extend_from_slice(&1u32.to_le_bytes());
            
            // Write channel as u32
            buffer.extend_from_slice(&(adc_data.channel as u32).to_le_bytes());
            
            // Convert ADC value to voltage and write as f32
            let voltage = (adc_data.value as f32) * (current_config.vref as f32) / (1 << 23) as f32;
            buffer.extend_from_slice(&voltage.to_le_bytes());
            
            if let Err(e) = eeg_data_tx_clone.send(buffer) {
                // Only log if there are subscribers (ignore if no WebSocket clients)
                if !matches!(e, broadcast::error::SendError(_)) {
                    eprintln!("Error broadcasting EEG data: {}", e);
                }
            }
        }
        println!("Data forwarding task completed");
    });

    // Main event loop
    let mut current_eeg_system = eeg_system;
    
    loop {
        tokio::select! {
            result = &mut data_forwarding_handle => {
                println!("Data forwarding task completed: {:?}", result);
                break;
            },
            result = &mut server_handle => {
                println!("Server task completed: {:?}", result);
                break;
            },
            config_update = config_update_rx.recv() => {
                if let Some(new_config) = config_update {
                    println!("Received config update. Channels: {:?}, Sample rate: {}", 
                             new_config.channels, new_config.sample_rate);
                    
                    // Check if recording is in progress
                    let recording_in_progress = is_recording.load(Ordering::Relaxed);
                    
                    if recording_in_progress {
                        println!("Warning: Cannot update configuration during recording");
                    } else {
                        // Update the shared config
                        {
                            let mut config_guard = config.lock().await;
                            *config_guard = new_config.clone();
                        }
                        
                        // Reconfigure the EEG system
                        if let Err(e) = current_eeg_system.reconfigure(new_config.clone()).await {
                            eprintln!("Error reconfiguring EEG system: {}", e);
                        } else {
                            println!("EEG system reconfigured successfully");
                            
                            // Broadcast the applied configuration
                            if let Err(e) = config_applied_tx.send(new_config) {
                                println!("Error broadcasting applied config: {}", e);
                            }
                        }
                    }
                }
            }
        }
    }

    // Cleanup
    println!("Shutting down EEG system...");
    if let Err(e) = current_eeg_system.shutdown().await {
        eprintln!("Error shutting down EEG system: {}", e);
    }

    // Shutdown PluginManager
    println!("Shutting down PluginManager...");
    if let Err(e) = plugin_manager.lock().await.shutdown().await {
        eprintln!("Error shutting down PluginManager: {}", e);
    }

    // Release PID lock
    if let Err(e) = pid_manager.release_lock() {
        eprintln!("Warning: Failed to release PID lock: {}", e);
    }

    println!("EEG Daemon stopped");
    Ok(())
}
