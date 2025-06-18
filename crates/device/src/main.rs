mod config;
mod driver_handler;
mod server;
mod pid_manager;
mod plugin_manager;
mod connection_manager;
mod elata_emu_v1;

// New event-driven modules
mod event;
mod plugin;
mod event_bus;

use eeg_sensor::AdcConfig;
use tokio::sync::{broadcast, Mutex};
use crate::elata_emu_v1::EegSystem;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::fmt;
use tokio_util::sync::CancellationToken;
use std::time::{SystemTime, UNIX_EPOCH};

// Import event-driven types
use crate::event::{EegPacket, SensorEvent, SystemEvent, SystemEventType};
use crate::event_bus::EventBus;
use crate::plugin::{EegPlugin, EventFilter};

use crate::driver_handler::{
    CsvRecorder,
    EegBatchData,
    FilteredEegData,
    ProcessedData,
    process_eeg_data
};

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

/// Plugin supervisor configuration
#[derive(Debug, Clone)]
struct SupervisorConfig {
    max_retries: u8,
    base_backoff_ms: u64,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_backoff_ms: 1000,
        }
    }
}

/// Supervise a plugin with automatic restart on failure
async fn supervise_plugin(
    plugin: Arc<dyn EegPlugin>,
    bus: Arc<EventBus>,
    shutdown_token: CancellationToken,
    config: SupervisorConfig,
) {
    let plugin_name = plugin.name();
    let mut attempts = 0;
    
    tracing::info!(plugin = plugin_name, "Starting plugin supervision");
    
    loop {
        // Check if shutdown was requested before starting/restarting
        if shutdown_token.is_cancelled() {
            tracing::info!(plugin = plugin_name, "Shutdown requested, stopping supervision");
            break;
        }
        
        // Subscribe to the event bus for this plugin instance
        let receiver = bus.subscribe(
            format!("{}-{}", plugin_name, attempts),
            128, // Buffer size
            plugin.event_filter(),
        ).await;
        
        // Initialize the plugin
        if let Err(e) = plugin.initialize().await {
            tracing::error!(plugin = plugin_name, error = %e, "Plugin initialization failed");
            attempts += 1;
            if attempts > config.max_retries {
                tracing::error!(plugin = plugin_name, "Giving up after {} attempts", config.max_retries);
                break;
            }
            
            let backoff_duration = tokio::time::Duration::from_millis(
                config.base_backoff_ms * (2_u64.pow(attempts as u32 - 1))
            );
            tracing::warn!(plugin = plugin_name, "Retrying in {:?}", backoff_duration);
            tokio::time::sleep(backoff_duration).await;
            continue;
        }
        
        // Run the plugin
        let run_result = plugin.run(bus.clone(), receiver, shutdown_token.clone()).await;
        
        // Cleanup the plugin
        if let Err(e) = plugin.cleanup().await {
            tracing::warn!(plugin = plugin_name, error = %e, "Plugin cleanup failed");
        }
        
        match run_result {
            Ok(()) => {
                tracing::info!(plugin = plugin_name, "Plugin completed successfully");
                break; // Normal completion
            }
            Err(e) => {
                attempts += 1;
                tracing::error!(
                    plugin = plugin_name,
                    error = %e,
                    attempt = attempts,
                    "Plugin crashed"
                );
                
                if attempts > config.max_retries {
                    tracing::error!(
                        plugin = plugin_name,
                        "Giving up after {} attempts",
                        config.max_retries
                    );
                    break;
                }
                
                // Exponential backoff
                let backoff_duration = tokio::time::Duration::from_millis(
                    config.base_backoff_ms * (2_u64.pow(attempts as u32 - 1))
                );
                tracing::warn!(
                    plugin = plugin_name,
                    "Retrying in {:?}",
                    backoff_duration
                );
                tokio::time::sleep(backoff_duration).await;
            }
        }
    }
    
    tracing::info!(plugin = plugin_name, "Plugin supervision ended");
}

/// Data acquisition loop that converts ProcessedData to SensorEvents and broadcasts them
async fn data_acquisition_loop(
    mut data_rx: tokio::sync::mpsc::Receiver<ProcessedData>,
    bus: Arc<EventBus>,
    shutdown_token: CancellationToken,
) -> anyhow::Result<()> {
    let mut frame_counter = 0u64;
    
    tracing::info!("Starting data acquisition loop");
    
    loop {
        tokio::select! {
            biased; // Prioritize shutdown
            _ = shutdown_token.cancelled() => {
                tracing::info!("Data acquisition loop received shutdown signal");
                break;
            }
            data = data_rx.recv() => {
                match data {
                    Some(processed_data) => {
                        // Convert ProcessedData to EegPacket
                        let channel_count = processed_data.voltage_samples.len();
                        let sample_rate = 500.0; // TODO: Get from config
                        
                        // Flatten the voltage samples (channel-interleaved format)
                        let mut flattened_samples = Vec::new();
                        if !processed_data.voltage_samples.is_empty() {
                            let samples_per_channel = processed_data.voltage_samples[0].len();
                            for sample_idx in 0..samples_per_channel {
                                for channel_samples in &processed_data.voltage_samples {
                                    if sample_idx < channel_samples.len() {
                                        flattened_samples.push(channel_samples[sample_idx]);
                                    }
                                }
                            }
                        }
                        
                        let eeg_packet = EegPacket::new(
                            processed_data.timestamp,
                            frame_counter,
                            flattened_samples,
                            channel_count,
                            sample_rate,
                        );
                        
                        let event = SensorEvent::RawEeg(Arc::new(eeg_packet));
                        
                        // Broadcast the event
                        bus.broadcast(event).await;
                        
                        frame_counter += 1;
                        
                        if frame_counter % 100 == 0 {
                            tracing::debug!("Processed {} frames", frame_counter);
                        }
                    }
                    None => {
                        tracing::warn!("Data receiver channel closed");
                        break;
                    }
                }
            }
        }
    }
    
    tracing::info!("Data acquisition loop ended");
    Ok(())
}

// Helper function to convert AdcData to ProcessedData
async fn convert_adc_to_processed_data(
    mut adc_rx: tokio::sync::mpsc::Receiver<eeg_sensor::AdcData>,
    processed_tx: tokio::sync::mpsc::Sender<crate::driver_handler::ProcessedData>,
    config: Arc<Mutex<AdcConfig>>,
) {
    use std::collections::HashMap;
    
    let mut channel_buffers: HashMap<u8, Vec<(i32, u64)>> = HashMap::new();
    
    while let Some(adc_data) = adc_rx.recv().await {
        // Get current config
        let (batch_size, vref, num_channels) = {
            let config_guard = config.lock().await;
            (config_guard.batch_size as usize, config_guard.vref, config_guard.channels.len())
        };
        
        // Accumulate data by channel
        let buffer = channel_buffers.entry(adc_data.channel).or_insert_with(Vec::new);
        buffer.push((adc_data.value, adc_data.timestamp));
        
        // Check if we have enough data for a batch
        let min_buffer_size = channel_buffers.values().map(|v| v.len()).min().unwrap_or(0);
        if min_buffer_size >= batch_size {
            // Create ProcessedData from accumulated AdcData
            let mut voltage_samples = Vec::new();
            let mut raw_samples = Vec::new();
            let mut latest_timestamp = 0u64;
            
            // Process each channel
            for channel_idx in 0..num_channels {
                let channel = channel_idx as u8;
                if let Some(buffer) = channel_buffers.get_mut(&channel) {
                    let batch: Vec<_> = buffer.drain(0..batch_size).collect();
                    
                    let voltages: Vec<f32> = batch.iter().map(|(raw, _)| {
                        // Convert raw ADC value to voltage
                        // This is a simplified conversion - adjust based on your ADC specs
                        let vref_f32 = vref as f32;
                        (*raw as f32) * (vref_f32 / (1 << 24) as f32)
                    }).collect();
                    
                    let raws: Vec<i32> = batch.iter().map(|(raw, _)| *raw).collect();
                    
                    // Use the latest timestamp from this batch
                    if let Some((_, timestamp)) = batch.last() {
                        latest_timestamp = latest_timestamp.max(*timestamp);
                    }
                    
                    voltage_samples.push(voltages);
                    raw_samples.push(raws);
                } else {
                    // No data for this channel, add empty vectors
                    voltage_samples.push(vec![0.0; batch_size]);
                    raw_samples.push(vec![0; batch_size]);
                }
            }
            
            // Create ProcessedData from the batched data
            let processed_data = crate::driver_handler::ProcessedData {
                timestamp: latest_timestamp,
                voltage_samples,
                raw_samples,
                power_spectrums: None,
                frequency_bins: None,
                error: None,
            };
            
            // Send the processed data
            if let Err(_) = processed_tx.send(processed_data).await {
                break; // Receiver dropped
            }
        }
    }
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
    
    tracing::info!("EEG Daemon starting (PID: {})...", std::process::id());

    // Load daemon configuration
    let daemon_config = config::load_config();
    tracing::info!("Daemon configuration:");
    tracing::info!("  Max recording length: {} minutes", daemon_config.max_recording_length_minutes);
    tracing::info!("  Recordings directory: {}", daemon_config.recordings_directory);
    tracing::info!("  Batch size: {}", daemon_config.batch_size);
    tracing::info!("  Driver type: {:?}", daemon_config.driver_type);
    
    // Debug: Print current working directory
    match std::env::current_dir() {
        Ok(path) => println!("Current working directory: {:?}", path),
        Err(e) => println!("Failed to get current working directory: {}", e),
    }

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
    let (mut eeg_system, adc_data_rx) = EegSystem::new(initial_config.clone()).await
        .map_err(to_daemon_error)?;
    
    eeg_system.start(initial_config.clone()).await
        .map_err(to_daemon_error)?;

    tracing::info!("EEG system started. Waiting for data...");
    
    // Create a channel for ProcessedData
    let (processed_tx, data_rx) = tokio::sync::mpsc::channel::<crate::driver_handler::ProcessedData>(100);
    
    // Spawn the conversion task
    let config_for_converter = config.clone();
    tokio::spawn(convert_adc_to_processed_data(adc_data_rx, processed_tx, config_for_converter));

    // === EVENT-DRIVEN ARCHITECTURE SETUP ===
    
    // Create the EventBus and CancellationToken
    let event_bus = Arc::new(EventBus::new());
    let shutdown_token = CancellationToken::new();
    
    tracing::info!("EventBus initialized");
    
    // Create a channel for the data acquisition loop
    let (data_acq_tx, data_acq_rx) = tokio::sync::mpsc::channel::<ProcessedData>(100);
    
    // Spawn the data acquisition loop that converts ProcessedData to events
    let data_acq_bus = event_bus.clone();
    let data_acq_shutdown = shutdown_token.clone();
    let mut data_acquisition_handle = tokio::spawn(async move {
        if let Err(e) = data_acquisition_loop(data_acq_rx, data_acq_bus, data_acq_shutdown).await {
            tracing::error!("Data acquisition loop failed: {}", e);
        }
    });
    
    // TODO: Initialize plugins here (Phase 3)
    // For now, we'll keep the existing plugin manager for compatibility
    let plugin_manager = match plugin_manager::PluginManager::new().await {
        Ok(pm) => Arc::new(Mutex::new(pm)),
        Err(e) => {
            eprintln!("Failed to initialize PluginManager: {}", e);
            return Err(e);
        }
    };

    // Create a broadcast channel for config updates
    let (config_applied_tx, _) = broadcast::channel::<AdcConfig>(16);

    // Create broadcast channels for different data pipelines
    let (tx_eeg_batch, _) = broadcast::channel::<EegBatchData>(256);
    let (tx_filtered_eeg, _) = broadcast::channel::<FilteredEegData>(256);

    // Create ConnectionManager
    let dsp_coordinator = Arc::new(Mutex::new(connection_manager::DspCoordinator::new()));
    let connection_manager = Arc::new(connection_manager::ConnectionManager::new(
        dsp_coordinator.clone(),
        initial_config.channels.iter().map(|&c| c as usize).collect(),
    ));

    // Create CsvRecorder
    let csv_recorder = Arc::new(Mutex::new(CsvRecorder::new(
        initial_config.sample_rate,
        daemon_config.clone(),
        initial_config.clone(),
        is_recording.clone(),
    )));

    // Set up WebSocket routes and get config update channel
    let (ws_routes, mut config_update_rx) = server::setup_websocket_routes(
        config.clone(),
        csv_recorder.clone(),
        config_applied_tx.clone(),
        tx_eeg_batch.clone(),
        tx_filtered_eeg.clone(),
        connection_manager.clone(),
        is_recording.clone(),
    );
    
    println!("WebSocket server starting on:");
    println!("- ws://0.0.0.0:8080/config (Configuration)");
    println!("- ws://0.0.0.0:8080/command (Recording control)");
    println!("- ws://0.0.0.0:8080/eeg (EEG data streaming)");

    // Spawn WebSocket server
    let mut server_handle = tokio::spawn(warp::serve(ws_routes).run(([0, 0, 0, 0], 8080)));

    // === LEGACY COMPATIBILITY ===
    // Keep the old processing task for now (will be removed in Phase 4)
    let cancellation_token = CancellationToken::new();
    let mut processing_handle = tokio::spawn(process_eeg_data(
        data_rx,
        tx_eeg_batch.clone(),
        tx_filtered_eeg.clone(),
        csv_recorder.clone(),
        is_recording.clone(),
        connection_manager.clone(),
        cancellation_token.clone(),
    ));

    // === EVENT-DRIVEN MAIN LOOP ===
    let mut current_eeg_system = eeg_system;
    
    tracing::info!("EEG Daemon fully initialized and running");
    
    loop {
        tokio::select! {
            biased; // Prioritize shutdown signal
            
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Ctrl-C received, initiating shutdown");
                shutdown_token.cancel();
                break;
            },
            
            result = &mut data_acquisition_handle => {
                tracing::warn!("Data acquisition loop completed: {:?}", result);
                break;
            },
            
            result = &mut processing_handle => {
                tracing::warn!("Legacy processing task completed: {:?}", result);
                break;
            },
            
            result = &mut server_handle => {
                tracing::warn!("Server task completed: {:?}", result);
                break;
            },
            config_update = config_update_rx.recv() => {
                if let Some(new_config) = config_update {
                    tracing::info!("Received config update. Channels: {:?}, Sample rate: {}",
                                 new_config.channels, new_config.sample_rate);
                    
                    // Check if recording is in progress
                    let recording_in_progress = is_recording.load(Ordering::Relaxed);
                    
                    if recording_in_progress {
                        tracing::warn!("Cannot update configuration during recording");
                    } else {
                        // Update the shared config
                        {
                            let mut config_guard = config.lock().await;
                            *config_guard = new_config.clone();
                        }
                        
                        // Reconfigure the EEG system
                        if let Err(e) = current_eeg_system.reconfigure(new_config.clone()).await {
                            tracing::error!("Error reconfiguring EEG system: {}", e);
                        } else {
                            tracing::info!("EEG system reconfigured successfully");
                            
                            // Broadcast the applied configuration
                            if let Err(e) = config_applied_tx.send(new_config) {
                                tracing::error!("Error broadcasting applied config: {}", e);
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

    // === EVENT-DRIVEN SHUTDOWN CLEANUP ===
    
    tracing::info!("Initiating graceful shutdown...");
    
    // Cancel all tasks
    shutdown_token.cancel();
    cancellation_token.cancel();
    
    // Wait for data acquisition loop to complete
    if let Err(e) = data_acquisition_handle.await {
        tracing::error!("Data acquisition handle join error: {}", e);
    }
    
    // Wait for legacy processing task to complete
    if let Err(e) = processing_handle.await {
        tracing::error!("Processing handle join error: {}", e);
    }
    
    // Give a moment for any remaining cleanup
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Shutdown EEG system
    tracing::info!("Shutting down EEG system...");
    if let Err(e) = current_eeg_system.shutdown().await {
        tracing::error!("Error shutting down EEG system: {}", e);
    }

    // Shutdown PluginManager
    tracing::info!("Shutting down PluginManager...");
    if let Err(e) = plugin_manager.lock().await.shutdown().await {
        tracing::error!("Error shutting down PluginManager: {}", e);
    }

    // Release PID lock
    if let Err(e) = pid_manager.release_lock() {
        tracing::warn!("Failed to release PID lock: {}", e);
    }

    tracing::info!("EEG Daemon stopped gracefully");
    Ok(())
}
