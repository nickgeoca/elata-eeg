mod config;
mod server;
mod pid_manager;
mod connection_manager;
mod elata_emu_v1;

// New event-driven modules
mod event;
mod plugin;
mod event_bus;

// Import plugin implementations
use csv_recorder_plugin::{CsvRecorderPlugin, CsvRecorderConfig};
use basic_voltage_filter_plugin::{BasicVoltageFilterPlugin, BasicVoltageFilterConfig};

use eeg_sensor::AdcConfig;
use tokio::sync::{broadcast, Mutex};
use crate::elata_emu_v1::EegSystem;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::fmt;
use tokio_util::sync::CancellationToken;

// Import event-driven types
use eeg_types::{EegPacket, SensorEvent, EegPlugin};
use eeg_sensor::AdcData;
use crate::event_bus::EventBus;

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

/// Data acquisition loop that converts raw ADC data to SensorEvents and broadcasts them
async fn data_acquisition_loop(
    mut adc_rx: tokio::sync::mpsc::Receiver<AdcData>,
    bus: Arc<EventBus>,
    config: Arc<Mutex<AdcConfig>>,
    shutdown_token: CancellationToken,
) -> anyhow::Result<()> {
    use std::collections::HashMap;

    let mut frame_counter = 0u64;
    let mut channel_buffers: HashMap<u8, Vec<(i32, u64)>> = HashMap::new();

    tracing::info!("Starting data acquisition loop");

    loop {
        tokio::select! {
            biased; // Prioritize shutdown
            _ = shutdown_token.cancelled() => {
                tracing::info!("Data acquisition loop received shutdown signal");
                break;
            }
            Some(adc_data) = adc_rx.recv() => {
                // Get current config
                let (batch_size, vref, num_channels, sample_rate) = {
                    let config_guard = config.lock().await;
                    (
                        config_guard.batch_size as usize,
                        config_guard.vref,
                        config_guard.channels.len(),
                        config_guard.sample_rate as f32,
                    )
                };

                // Accumulate data by channel
                let buffer = channel_buffers.entry(adc_data.channel).or_insert_with(Vec::new);
                buffer.push((adc_data.value, adc_data.timestamp));

                // Check if we have enough data for a batch
                let min_buffer_size = channel_buffers.values().map(|v| v.len()).min().unwrap_or(0);
                if min_buffer_size >= batch_size {
                    let mut voltage_samples = Vec::new();
                    let mut latest_timestamp = 0u64;

                    // Process each channel
                    for channel_idx in 0..num_channels {
                        let channel = channel_idx as u8;
                        if let Some(buffer) = channel_buffers.get_mut(&channel) {
                            let batch: Vec<_> = buffer.drain(0..batch_size).collect();

                            let voltages: Vec<f32> = batch.iter().map(|(raw, _)| {
                                let vref_f32 = vref as f32;
                                (*raw as f32) * (vref_f32 / (1 << 24) as f32)
                            }).collect();

                            if let Some((_, timestamp)) = batch.last() {
                                latest_timestamp = latest_timestamp.max(*timestamp);
                            }
                            voltage_samples.push(voltages);
                        } else {
                            voltage_samples.push(vec![0.0; batch_size]);
                        }
                    }

                    // Flatten the voltage samples for the EegPacket
                    let mut flattened_samples = Vec::new();
                    if !voltage_samples.is_empty() {
                        let samples_per_channel = voltage_samples[0].len();
                        for sample_idx in 0..samples_per_channel {
                            for channel_samples in &voltage_samples {
                                if sample_idx < channel_samples.len() {
                                    flattened_samples.push(channel_samples[sample_idx]);
                                }
                            }
                        }
                    }

                    let eeg_packet = EegPacket::new(
                        latest_timestamp,
                        frame_counter,
                        flattened_samples,
                        num_channels,
                        sample_rate.into(),
                    );

                    let event = SensorEvent::RawEeg(Arc::new(eeg_packet));
                    bus.broadcast(event).await;
                    frame_counter += 1;

                    if frame_counter % 100 == 0 {
                        tracing::debug!("Processed {} frames", frame_counter);
                    }
                }
            }
            else => {
                tracing::warn!("ADC data receiver channel closed");
                break;
            }
        }
    }

    tracing::info!("Data acquisition loop ended");
    Ok(())
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

    // === EVENT-DRIVEN ARCHITECTURE SETUP ===
    
    // Create the EventBus and CancellationToken
    let event_bus = Arc::new(EventBus::new());
    let shutdown_token = CancellationToken::new();
    
    tracing::info!("EventBus initialized");
    
    // Spawn the data acquisition loop
    let data_acq_bus = event_bus.clone();
    let data_acq_shutdown = shutdown_token.clone();
    let data_acq_config = config.clone();
    let mut data_acquisition_handle = tokio::spawn(async move {
        if let Err(e) = data_acquisition_loop(adc_data_rx, data_acq_bus, data_acq_config, data_acq_shutdown).await {
            tracing::error!("Data acquisition loop failed: {}", e);
        }
    });
    
    // === PLUGIN INITIALIZATION (Phase 3) ===
    
    // Convert daemon_config to eeg_types::DaemonConfig
    let eeg_daemon_config = Arc::new(eeg_types::DaemonConfig {
        max_recording_length_minutes: daemon_config.max_recording_length_minutes,
        recordings_directory: daemon_config.recordings_directory.clone(),
        session: daemon_config.session.clone(),
        batch_size: daemon_config.batch_size,
        driver_type: match daemon_config.driver_type {
            eeg_sensor::DriverType::Mock => eeg_types::DriverType::MockEeg,
            eeg_sensor::DriverType::Ads1299 => eeg_types::DriverType::Ads1299,
        },
        filter_config: eeg_types::FilterConfig {
            dsp_high_pass_cutoff_hz: daemon_config.filter_config.dsp_high_pass_cutoff_hz,
            dsp_low_pass_cutoff_hz: daemon_config.filter_config.dsp_low_pass_cutoff_hz,
            powerline_filter_hz: daemon_config.filter_config.powerline_filter_hz.unwrap_or(0) as f32,
        },
    });

    // Create plugin configurations
    let csv_config = CsvRecorderConfig {
        daemon_config: eeg_daemon_config.clone(),
        adc_config: initial_config.clone(),
        is_recording_shared: is_recording.clone(),
    };
    
    let filter_config = BasicVoltageFilterConfig {
        daemon_config: eeg_daemon_config.clone(),
        sample_rate: initial_config.sample_rate,
        num_channels: initial_config.channels.len(),
    };
    
    // Create plugin instances
    let csv_plugin = Arc::new(CsvRecorderPlugin::new(csv_config)) as Arc<dyn EegPlugin>;
    let filter_plugin = Arc::new(BasicVoltageFilterPlugin::new(filter_config)) as Arc<dyn EegPlugin>;
    
    // Supervisor configuration
    let supervisor_config = SupervisorConfig::default();
    
    // Start plugin supervision tasks
    let csv_supervisor_bus = event_bus.clone();
    let csv_supervisor_shutdown = shutdown_token.clone();
    let csv_supervisor_config = supervisor_config.clone();
    let mut csv_supervisor_handle = tokio::spawn(async move {
        supervise_plugin(csv_plugin, csv_supervisor_bus, csv_supervisor_shutdown, csv_supervisor_config).await;
    });
    
    let filter_supervisor_bus = event_bus.clone();
    let filter_supervisor_shutdown = shutdown_token.clone();
    let filter_supervisor_config = supervisor_config.clone();
    let mut filter_supervisor_handle = tokio::spawn(async move {
        supervise_plugin(filter_plugin, filter_supervisor_bus, filter_supervisor_shutdown, filter_supervisor_config).await;
    });
    
    tracing::info!("Event-driven plugins initialized and supervised");
    

    // Create a broadcast channel for config updates
    let (config_applied_tx, _) = broadcast::channel::<AdcConfig>(16);


    // Create ConnectionManager
    let dsp_coordinator = Arc::new(Mutex::new(connection_manager::DspCoordinator::new()));
    let connection_manager = Arc::new(connection_manager::ConnectionManager::new(
        dsp_coordinator.clone(),
        initial_config.channels.iter().map(|&c| c as usize).collect(),
    ));

    // Set up WebSocket routes and get config update channel
    let (ws_routes, mut config_update_rx) = server::setup_websocket_routes(
        config.clone(),
        config_applied_tx.clone(),
        connection_manager.clone(),
        is_recording.clone(),
    );
    
    println!("WebSocket server starting on:");
    println!("- ws://0.0.0.0:8080/config (Configuration)");
    println!("- ws://0.0.0.0:8080/command (Recording control)");
    println!("- ws://0.0.0.0:8080/eeg (EEG data streaming)");

    // Spawn WebSocket server
    let mut server_handle = tokio::spawn(warp::serve(ws_routes).run(([0, 0, 0, 0], 8080)));


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
            
            
            result = &mut csv_supervisor_handle => {
                tracing::warn!("CSV recorder plugin supervisor completed: {:?}", result);
                break;
            },
            
            result = &mut filter_supervisor_handle => {
                tracing::warn!("Basic voltage filter plugin supervisor completed: {:?}", result);
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
    
    // Wait for data acquisition loop to complete
    if let Err(e) = data_acquisition_handle.await {
        tracing::error!("Data acquisition handle join error: {}", e);
    }
    
    // Give a moment for any remaining cleanup
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Shutdown EEG system
    tracing::info!("Shutting down EEG system...");
    if let Err(e) = current_eeg_system.shutdown().await {
        tracing::error!("Error shutting down EEG system: {}", e);
    }


    // Release PID lock
    if let Err(e) = pid_manager.release_lock() {
        tracing::warn!("Failed to release PID lock: {}", e);
    }

    tracing::info!("EEG Daemon stopped gracefully");
    Ok(())
}
