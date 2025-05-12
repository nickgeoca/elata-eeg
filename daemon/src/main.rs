mod config;
mod driver_handler;
mod server;

use eeg_driver::{AdcConfig, EegSystem, DriverType};
use tokio::sync::{broadcast, Mutex, mpsc};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logger - Reads RUST_LOG environment variable
    env_logger::init();

    // Load daemon configuration
    let daemon_config = config::load_config();
    println!("Daemon configuration:");
    println!("  Max recording length: {} minutes", daemon_config.max_recording_length_minutes);
    println!("  Recordings directory: {}", daemon_config.recordings_directory);
    println!("  High-pass filter cutoff: {} Hz", daemon_config.dsp_high_pass_cutoff_hz);
    println!("  Low-pass filter cutoff: {} Hz", daemon_config.dsp_low_pass_cutoff_hz);
    println!("  Batch size: {}", daemon_config.batch_size);
    println!("  Driver type: {:?}", daemon_config.driver_type);
    
    // Debug: Print current working directory
    match std::env::current_dir() {
        Ok(path) => println!("Current working directory: {:?}", path),
        Err(e) => println!("Failed to get current working directory: {}", e),
    }

    // Increase channel capacity but not too much to avoid excessive buffering
    let (tx, _) = broadcast::channel::<driver_handler::EegBatchData>(32);  // Reduced from 1024
    let tx_ws = tx.clone();

    // Create the ADC configuration
    let initial_config = AdcConfig {
        sample_rate: 500,
        // channels: vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31],
        channels: vec![0, 1],
        // channels: vec![0, 1, 2, 3, 4, 5, 6, 7],
        gain: 24.0,
        board_driver: daemon_config.driver_type,
        batch_size: daemon_config.batch_size,
        Vref: 4.5,
        dsp_high_pass_cutoff_hz: daemon_config.dsp_high_pass_cutoff_hz,
        dsp_low_pass_cutoff_hz: daemon_config.dsp_low_pass_cutoff_hz,
    };
    
    // Create shared state
    let config = Arc::new(Mutex::new(initial_config.clone()));
    let is_recording = Arc::new(AtomicBool::new(false));

    println!("Starting EEG system...");
    
    // Create and start the EEG system
    let (mut eeg_system, data_rx) = EegSystem::new(initial_config.clone()).await?;
    eeg_system.start(initial_config.clone()).await?;

    println!("EEG system started. Waiting for data...");

    // Create CSV recorder with daemon config and ADC config
    let csv_recorder = Arc::new(Mutex::new(driver_handler::CsvRecorder::new(
        initial_config.sample_rate,
        daemon_config.clone(),
        initial_config.clone(),
        is_recording.clone()
    )));

    // Set up WebSocket routes and get config update channel
    let (ws_routes, config_update_rx) = server::setup_websocket_routes(
        tx_ws,
        config.clone(),
        csv_recorder.clone(),
        is_recording.clone()
    );

    println!("WebSocket server starting on:");
    println!("- ws://localhost:8080/eeg (EEG data)");
    println!("- ws://localhost:8080/config (Configuration)");
    println!("- ws://localhost:8080/command (Recording control)");

    // Spawn WebSocket server
    let server_handle = tokio::spawn(warp::serve(ws_routes).run(([0, 0, 0, 0], 8080)));

    // Create a cancellation token for the processing task
    let processing_token = CancellationToken::new();
    let processing_token_clone = processing_token.clone();

    // Process EEG data
    let processing_handle = tokio::spawn(driver_handler::process_eeg_data(
        data_rx,
        tx,
        csv_recorder.clone(),
        is_recording.clone(),
        processing_token
    ));

    // Create a loop to handle configuration updates
    let mut current_processing_handle = processing_handle;
    let mut current_eeg_system = eeg_system;
    let mut current_token = processing_token_clone;
    
    loop {
        tokio::select! {
            _ = &mut current_processing_handle => {
                println!("Processing task completed");
                break;
            },
            _ = server_handle => {
                println!("Server task completed");
                break;
            },
            config_update = config_update_rx.recv() => {
                if let Some(new_config) = config_update {
                    println!("Received configuration update: {:?}", new_config.channels);
                    
                    // Check if recording is in progress
                    let recording_in_progress = is_recording.load(Ordering::Relaxed);
                    
                    if recording_in_progress {
                        println!("Warning: Cannot update configuration during recording");
                    } else {
                        println!("Stopping current EEG system...");
                        // Stop current EEG system
                        if let Err(e) = current_eeg_system.stop().await {
                            println!("Error stopping EEG system: {}", e);
                        }
                        
                        // Check if the new config is actually different from the current one
                        let current_config = {
                            let config_guard = config.lock().await;
                            config_guard.clone()
                        };
                        
                        // If the config hasn't changed, skip the restart
                        if new_config == current_config {
                            println!("Configuration unchanged, skipping restart");
                            continue;
                        }
                        
                        // Signal cancellation to the processing task
                        current_token.cancel();
                        
                        // Wait for the task to complete gracefully with a longer timeout
                        // This allows time for CSV flushing and other cleanup operations
                        if let Err(e) = tokio::time::timeout(
                            tokio::time::Duration::from_secs(10), // Increased from 2s to 10s
                            &mut current_processing_handle
                        ).await {
                            println!("Warning: Processing task did not complete in time, forcing abort: {}", e);
                            current_processing_handle.abort();
                        }
                        
                        println!("Starting new EEG system with updated configuration...");
                        // Create and start new EEG system with updated configuration
                        let (new_eeg_system, new_data_rx) = EegSystem::new(new_config.clone()).await?;
                        if let Err(e) = new_eeg_system.start(new_config.clone()).await {
                            println!("Error starting new EEG system: {}", e);
                            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other,
                                format!("Failed to start new EEG system: {}", e))));
                        }
                        
                        // Update the shared config
                        {
                            let mut config_guard = config.lock().await;
                            *config_guard = new_config.clone();
                        }
                        
                        // Update CSV recorder with new config
                        {
                            let mut recorder_guard = csv_recorder.lock().await;
                            recorder_guard.update_config(new_config.clone());
                        }
                        
                        // Create a new cancellation token
                        let new_token = CancellationToken::new();
                        
                        // Start new processing task
                        let new_processing_handle = tokio::spawn(driver_handler::process_eeg_data(
                            new_data_rx,
                            tx.clone(),
                            csv_recorder.clone(),
                            is_recording.clone(),
                            new_token.clone()
                        ));
                        
                        // Update variables for next iteration
                        current_eeg_system = new_eeg_system;
                        current_processing_handle = new_processing_handle;
                        current_token = new_token;
                        
                        println!("EEG system restarted with new configuration");
                    }
                }
            }
        }
    }

    // Cleanup
    eeg_system.stop().await?;
    
    Ok(())
}
