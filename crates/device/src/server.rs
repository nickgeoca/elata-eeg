use warp::ws::{Message, WebSocket};
use warp::Filter;
use serde::{Serialize, Deserialize};
use futures_util::{StreamExt, SinkExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{broadcast, Mutex, mpsc};
use eeg_sensor::AdcConfig;



/// Command message for WebSocket control
#[derive(Deserialize, Debug)]
#[serde(tag = "command")] // Use the "command" field as the tag
enum DaemonCommand {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "status")]
    Status,
    #[serde(rename = "set_powerline_filter")]
    SetPowerlineFilter {
        // If 'value' is missing or null in JSON, this will be None.
        // If 'value' is a number, it will be Some(number).
        value: Option<u32>,
    },
}

/// Configuration message for WebSocket control

// Helper functions for warp filters
fn with_broadcast_rx<T: Clone + Send + 'static>(
    rx: broadcast::Sender<T>,
) -> impl Filter<Extract = (broadcast::Receiver<T>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || rx.subscribe())
}

fn with_shared_state<T: Clone + Send + Sync + 'static>(
    state: Arc<Mutex<T>>,
) -> impl Filter<Extract = (Arc<Mutex<T>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}

fn with_mpsc_tx<T: Send + 'static>(
    tx: mpsc::Sender<T>,
) -> impl Filter<Extract = (mpsc::Sender<T>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || tx.clone())
}


fn with_atomic_bool(
    atomic: Arc<AtomicBool>,
) -> impl Filter<Extract = (Arc<AtomicBool>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || atomic.clone())
}


#[derive(Deserialize, Debug)]
pub struct ConfigMessage {
    pub channels: Option<Vec<u32>>,
    pub sample_rate: Option<u32>,
    // powerline_filter_hz field removed as part of DSP refactor
}

impl ConfigMessage {
    // Helper method for debugging (powerline filter functionality removed)
    pub fn debug_config(&self) {
        println!("[CONFIG_DEBUG] ConfigMessage channels: {:?}, sample_rate: {:?}", self.channels, self.sample_rate);
    }
}

/// Response message for WebSocket commands
#[derive(Serialize)]
pub struct CommandResponse {
    pub status: String,
    pub message: String,
}



/// Handle WebSocket connection for configuration data
pub async fn handle_config_websocket(
    ws: WebSocket,
    config: Arc<Mutex<AdcConfig>>,
    config_update_tx: mpsc::Sender<AdcConfig>, // For sending proposed updates to main
    mut config_applied_rx: broadcast::Receiver<AdcConfig>, // For receiving applied updates from main
    is_recording: Arc<AtomicBool>
) {
    let (mut ws_tx, mut ws_rx) = ws.split(); // WebSocket sender and receiver
    
    println!("Configuration WebSocket client connected");
    
    // Send initial configuration to the client
    let initial_config = {
        let config_guard = config.lock().await;
        config_guard.clone()
    };
    
    // Convert the configuration to JSON and send it
    if let Ok(config_json) = serde_json::to_string(&initial_config) {
        if let Err(e) = ws_tx.send(Message::text(config_json)).await {
            println!("Error sending initial configuration: {}", e);
        } else {
            println!("Initial configuration sent successfully");
            println!("Sample rate: {}", initial_config.sample_rate);
            println!("Channels: {:?}", initial_config.channels);
            println!("Gain: {}", initial_config.gain);
            println!("Board driver: {:?}", initial_config.board_driver);
            println!("Batch size: {}", initial_config.batch_size);
            println!("Vref: {}", initial_config.vref);
        }
    } else {
        println!("Error serializing configuration");
    }

    // Create an MPSC channel to forward messages from the spawned task to ws_tx
    let (mpsc_tx, mut mpsc_rx) = mpsc::channel::<Message>(32); // Channel for warp::ws::Message

    // **NEW: Send current config immediately on connection**
    let initial_config_for_client = {
        let config_guard = config.lock().await; // 'config' is Arc<Mutex<AdcConfig>> passed to handle_config_websocket
        config_guard.clone()
    };
    if let Ok(config_json) = serde_json::to_string(&initial_config_for_client) {
        println!("Config WebSocket: Queuing initial config for client: {}", config_json);
        if let Err(e) = mpsc_tx.send(Message::text(config_json)).await {
             println!("Config WebSocket: Error queueing initial config for client: {}", e);
             // Optionally, could send an error back to client or close if this fails.
        }
    }
    // **END NEW**

    // Task to listen for applied config updates from main.rs and send them to the client via the MPSC channel
    let ws_tx_forwarder = mpsc_tx.clone();
    tokio::spawn(async move {
        loop {
            match config_applied_rx.recv().await {
                Ok(applied_config) => {
                    println!("Config WebSocket: Received applied config from main: {:?}", applied_config.channels);
                    // powerline_filter_hz field removed as part of DSP refactor
                    if let Ok(config_json) = serde_json::to_string(&applied_config) {
                        println!("Config WebSocket: Sending JSON to client: {}", config_json);
                        if let Err(e) = ws_tx_forwarder.send(Message::text(config_json)).await {
                            println!("Config WebSocket: Error queueing applied config for client: {}", e);
                            break; // Stop if queueing fails (channel might be closed)
                        }
                    } else {
                        println!("Config WebSocket: Error serializing applied config");
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    println!("Config WebSocket: Lagged behind applied config broadcast by {} messages. Consider increasing channel capacity.", n);
                    // Potentially resend current config or signal client to refresh
                }
                Err(broadcast::error::RecvError::Closed) => {
                    println!("Config WebSocket: Applied config broadcast channel closed.");
                    break;
                }
            }
        }
        println!("Config WebSocket: Applied config listener task finished.");
    });
    
    // Process incoming configuration messages from this specific client
    loop {
        tokio::select! {
            // Arm to receive messages from the MPSC channel (either from spawned task or client message handler)
            // and forward them to the WebSocket client.
            Some(message_to_send) = mpsc_rx.recv() => {
                if let Err(e) = ws_tx.send(message_to_send).await {
                    println!("Config WebSocket: Error sending message from mpsc_rx to client: {}", e);
                    break; // Error sending to client, close connection
                }
            }

            // Arm to process incoming messages from the WebSocket client
            result = ws_rx.next() => {
                match result {
                    Some(Ok(msg)) => {
                        if msg.is_close() {
                            println!("Config WebSocket: Received close frame from client.");
                            break; // Exit loop on close frame
                        }
                        
                        if msg.is_text() {
                            let text_from_client = msg.to_str().unwrap_or_default();
                            println!("Config WebSocket: Received text message: {}", text_from_client);
                            
                            // ADD THIS LOG
                            println!("[SERVER_DEBUG] Attempting to parse JSON: >>>{}<<<", text_from_client);
                            
                            // Try to parse as ConfigMessage
                            match serde_json::from_str::<ConfigMessage>(text_from_client) {
                                Ok(mut config_msg) => {
                                    // Use our new debug method
                                    config_msg.debug_config();
                                    
                                    // ADD THIS LOG
                                    println!("[SERVER_DEBUG] Parsed config_msg - channels: {:?}, sample_rate: {:?}", config_msg.channels, config_msg.sample_rate);

                                    // ADD THIS LOG
                                    println!("[SERVER_DEBUG] Checking is_recording status: {}", is_recording.load(Ordering::Relaxed));

                                    if is_recording.load(Ordering::Relaxed) {
                                        let response = CommandResponse {
                                            status: "error".to_string(),
                                            message: "Cannot change configuration during recording".to_string(),
                                        };
                                        if let Ok(response_json) = serde_json::to_string(&response) {
                                            // Send response via mpsc_tx
                                            if let Err(e) = mpsc_tx.send(Message::text(response_json)).await {
                                                println!("Config WebSocket: Error queueing 'recording active' response: {}", e);
                                            }
                                        }
                                        continue;
                                    }
                                    
                                    // ADD THIS LOG
                                    println!("[SERVER_DEBUG] About to acquire config lock.");
                                    let config_guard = config.lock().await;
                                    // ADD THIS LOG
                                    println!("[SERVER_DEBUG] Config lock acquired. Current shared config: channels={:?}, sample_rate={}", config_guard.channels, config_guard.sample_rate);
                                    let mut updated_config = config_guard.clone();
                                    let mut config_changed = false;
                                    let mut update_message = String::new();
                                    let no_params_provided = config_msg.channels.is_none() &&
                                                            config_msg.sample_rate.is_none() &&
                                                            false; // powerline filter removed
 
                                    // ADD THIS LOG
                                    println!("[SERVER_DEBUG] About to process channels. config_msg.channels.is_some(): {}", config_msg.channels.is_some());
                                    if let Some(new_channels) = config_msg.channels.take() {
                                        if new_channels.is_empty() {
                                            let response = CommandResponse { status: "error".to_string(), message: "Channel list cannot be empty".to_string() };
                                            if let Ok(json) = serde_json::to_string(&response) {
                                                if let Err(e) = mpsc_tx.send(Message::text(json)).await {
                                                     println!("Config WebSocket: Error queueing 'channel empty' response: {}", e);
                                                }
                                            }
                                            continue;
                                        }
                                        // Channel validation is now handled by the driver
                                        let new_channels_u8: Vec<u8> = new_channels.iter().map(|&x| x as u8).collect();
                                        if updated_config.channels != new_channels_u8 {
                                            updated_config.channels = new_channels_u8;
                                            config_changed = true;
                                            update_message = format!("channels: {:?}", updated_config.channels);
                                        }
                                    }

                                    if let Some(new_sample_rate) = config_msg.sample_rate {
                                        // Sample rate validation is now handled by the driver
                                        if updated_config.sample_rate != new_sample_rate {
                                            updated_config.sample_rate = new_sample_rate;
                                            config_changed = true;
                                            if !update_message.is_empty() { update_message.push_str(", "); }
                                            update_message.push_str(&format!("sample rate: {}", new_sample_rate));
                                        }
                                    }

                                    // Powerline filter handling removed as part of DSP refactor
                                    
                                    if !config_changed {
                                        let msg_text_response = if no_params_provided { "No channels or sample rate provided" } else { "Configuration unchanged" };
                                        let status_str = if no_params_provided { "error" } else { "ok" };
                                        let response = CommandResponse { status: status_str.to_string(), message: msg_text_response.to_string() };
                                        if let Ok(response_json) = serde_json::to_string(&response) {
                                            if let Err(e) = mpsc_tx.send(Message::text(response_json)).await {
                                                println!("Config WebSocket: Error queueing 'config unchanged' response: {}", e);
                                            }
                                        }
                                        continue;
                                    }
                                    
                                    drop(config_guard);
                                    
                                    if let Err(e) = config_update_tx.send(updated_config.clone()).await {
                                        println!("Error sending config update to main: {}", e);
                                        let response = CommandResponse { status: "error".to_string(), message: format!("Failed to submit update: {}", e) };
                                        if let Ok(response_json) = serde_json::to_string(&response) {
                                            if let Err(e_send) = mpsc_tx.send(Message::text(response_json)).await {
                                                println!("Config WebSocket: Error queueing 'update submission failed' response: {}", e_send);
                                            }
                                        }
                                    } else {
                                        let response = CommandResponse {
                                            status: "ok".to_string(),
                                            message: format!("Config update for {} submitted for processing.", update_message),
                                        };
                                        if let Ok(response_json) = serde_json::to_string(&response) {
                                            if let Err(e) = mpsc_tx.send(Message::text(response_json)).await {
                                                println!("Config WebSocket: Error queueing 'update submitted' response: {}", e);
                                            }
                                        }
                                        // DO NOT send updated_config here. Client will get it via broadcast from the other task.
                                    }
                                },
                                Err(e) => {
                                    println!("Error parsing config message: {}", e);
                                    let response = CommandResponse { status: "error".to_string(), message: format!("Invalid config format: {}", e) };
                                    if let Ok(response_json) = serde_json::to_string(&response) {
                                        if let Err(e_send) = mpsc_tx.send(Message::text(response_json)).await {
                                            println!("Config WebSocket: Error queueing 'invalid format' response: {}", e_send);
                                        }
                                    }
                                }
                            }
                        } else {
                            println!("Config WebSocket: Received non-text message: {:?}", msg);
                        }
                    },
                    Some(Err(e)) => {
                        println!("Config WebSocket: Error receiving message from client: {}", e);
                        break;
                    }
                    None => {
                        println!("Config WebSocket: Client disconnected (stream ended).");
                        break;
                    }
                }
            },
        }
    }
  
    println!("Config WebSocket: Connection handler finished for a client.");
}

/// Handle WebSocket connection for recording control commands
pub async fn handle_command_websocket(
    ws: WebSocket,
    is_recording: Arc<AtomicBool>,
    config: Arc<Mutex<AdcConfig>>,
    config_update_tx: mpsc::Sender<AdcConfig>
) {
    let (mut tx, mut rx) = ws.split();
    
    println!("Command WebSocket client connected");
    
    // Send initial status
    let initial_status = {
        CommandResponse {
            status: "ok".to_string(),
            message: if is_recording.load(Ordering::Relaxed) {
                "Currently recording".to_string()
            } else {
                "Not recording".to_string()
            },
        }
    };
    
    if let Ok(status_json) = serde_json::to_string(&initial_status) {
        if let Err(e) = tx.send(Message::text(status_json)).await {
            println!("Error sending initial status: {}", e);
            return;
        }
    }
    
    // Set up periodic status updates (every 5 seconds)
    
    // Use a channel to send messages to the status update task
    let (status_tx, mut status_rx) = tokio::sync::mpsc::channel::<String>(32);
    let status_tx_clone = status_tx.clone();
    
    // Clone is_recording before moving it into the task
    let is_recording_clone = is_recording.clone();
    
    // Spawn a task to handle periodic status updates
    let status_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        
        loop {
            interval.tick().await;
            
            let status_update = {
                CommandResponse {
                    status: "ok".to_string(),
                    message: if is_recording_clone.load(Ordering::Relaxed) {
                        "Currently recording".to_string()
                    } else {
                        "Not recording".to_string()
                    },
                }
            };
            
            if let Ok(status_json) = serde_json::to_string(&status_update) {
                if let Err(_) = status_tx.send(status_json).await {
                    println!("Error sending status update to channel");
                    break;
                }
            }
        }
    });
    
    // Spawn a task to forward messages from the channel to the WebSocket
    let forward_task = tokio::spawn(async move {
        while let Some(msg) = status_rx.recv().await {
            if let Err(e) = tx.send(Message::text(msg)).await {
                println!("Error sending status update: {}", e);
                break;
            }
        }
    });
    
    // Process incoming commands
    while let Some(result) = rx.next().await {
        match result {
            Ok(msg) => {
                if !msg.is_text() {
                    continue;
                }
                
                let text = msg.to_str().unwrap_or_default();
                // Clone is_recording for use in the match block
                let is_recording_local = is_recording.clone();
                
                let response = match serde_json::from_str::<DaemonCommand>(text) {
                    Ok(daemon_cmd) => {
                        match daemon_cmd {
                            DaemonCommand::Start => {
                                // NOTE: Recording functionality is currently disabled
                                // and will be handled by a dedicated plugin in the future.
                                is_recording_local.store(true, Ordering::Relaxed);
                                CommandResponse {
                                    status: "ok".to_string(),
                                    message: "Recording started (placeholder)".to_string(),
                                }
                            },
                            DaemonCommand::Stop => {
                                // NOTE: Recording functionality is currently disabled.
                                is_recording_local.store(false, Ordering::Relaxed);
                                CommandResponse {
                                    status: "ok".to_string(),
                                    message: "Recording stopped (placeholder)".to_string(),
                                }
                            },
                            DaemonCommand::Status => {
                                CommandResponse {
                                    status: "ok".to_string(),
                                    message: if is_recording.load(Ordering::SeqCst) {
                                        "Currently recording".to_string()
                                    } else {
                                        "Not recording".to_string()
                                    },
                                }
                            },
                            DaemonCommand::SetPowerlineFilter { value: new_powerline_filter_opt } => {
                                if is_recording_local.load(Ordering::Relaxed) {
                                    CommandResponse {
                                        status: "error".to_string(),
                                        message: "Cannot change configuration during recording".to_string(),
                                    }
                                } else {
                                    // Validate the powerline filter value
                                    // new_powerline_filter_opt is Option<u32>
                                    // We need to check if it's Some(value) where value is not 50 or 60
                                    let is_valid_filter_value = match new_powerline_filter_opt {
                                        Some(val) => val == 50 || val == 60,
                                        None => true, // None (off) is valid
                                    };

                                    if !is_valid_filter_value {
                                        CommandResponse {
                                            status: "error".to_string(),
                                            message: format!("Invalid powerline filter value: {:?}. Valid: 50, 60, or null (off)", new_powerline_filter_opt)
                                        }
                                    } else {
                                        let config_guard = config.lock().await;
                                        let updated_config = config_guard.clone();
                                        let config_changed = false;

                                        // Powerline filter handling removed as part of DSP refactor
                                        // config_changed remains false since no powerline filter to update

                                        if config_changed {
                                            drop(config_guard); // Release lock before sending to channel
                                            match config_update_tx.send(updated_config.clone()).await {
                                                Ok(_) => CommandResponse {
                                                    status: "ok".to_string(),
                                                    message: format!("Powerline filter update ({:?}) submitted.",
                                                        new_powerline_filter_opt.map_or("Off".to_string(), |f| f.to_string() + "Hz"))
                                                },
                                                Err(e) => CommandResponse {
                                                    status: "error".to_string(),
                                                    message: format!("Failed to submit powerline filter update: {}", e),
                                                },
                                            }
                                        } else {
                                            CommandResponse {
                                                status: "ok".to_string(),
                                                message: "Powerline filter configuration unchanged.".to_string(),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Err(e) => CommandResponse {
                        status: "error".to_string(),
                        message: format!("Invalid command format: {}", e),
                    },
                };
                
                if let Ok(response_json) = serde_json::to_string(&response) {
                    if let Err(_) = status_tx_clone.send(response_json).await {
                        println!("Error sending command response to channel");
                        break;
                    }
                }
            },
            Err(e) => {
                println!("Error receiving command: {}", e);
                break;
            }
        }
    }
    
    // Cancel both tasks when the WebSocket connection is closed
    status_task.abort();
    forward_task.abort();
    
    println!("Command WebSocket client disconnected");
}

/// Handle WebSocket connection for EEG data streaming
pub async fn handle_eeg_websocket(
    ws: WebSocket,
    mut data_rx: broadcast::Receiver<Vec<u8>>, // Receiver for binary EEG data
) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    
    println!("EEG data WebSocket client connected");
    
    // Handle both data forwarding and client messages in a single loop
    loop {
        tokio::select! {
            // Handle incoming WebSocket messages from client
            result = ws_rx.next() => {
                match result {
                    Some(Ok(msg)) => {
                        if msg.is_close() {
                            println!("EEG WebSocket: Received close frame from client");
                            break;
                        }
                        // Ignore other message types for now
                    }
                    Some(Err(e)) => {
                        println!("EEG WebSocket: Error receiving message: {}", e);
                        break;
                    }
                    None => {
                        println!("EEG WebSocket: Client disconnected");
                        break;
                    }
                }
            }
            // Handle incoming EEG data
            data_result = data_rx.recv() => {
                match data_result {
                    Ok(data_packet) => {
                        if let Err(e) = ws_tx.send(Message::binary(data_packet)).await {
                            println!("Error sending EEG data to client: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        println!("EEG WebSocket: Lagged behind data broadcast by {} messages", n);
                        // Continue receiving - client will get next available data
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        println!("EEG WebSocket: Data broadcast channel closed");
                        break;
                    }
                }
            }
        }
    }
    
    println!("EEG WebSocket: Connection handler finished");
}

// Brain waves FFT WebSocket handler moved to elata_dsp_brain_waves_fft crate

// Set up WebSocket routes and server
use crate::connection_manager::{ConnectionManager, ClientType};
use crate::driver_handler::{EegBatchData, FilteredEegData, CsvRecorder};

pub fn create_eeg_binary_packet(batch: &EegBatchData) -> Vec<u8> {
    let mut buffer = Vec::new();
    buffer.extend_from_slice(&batch.timestamp.to_le_bytes());
    buffer.push(batch.error.is_some() as u8);
    if let Some(error) = &batch.error {
        buffer.extend_from_slice(error.as_bytes());
    } else {
        for i in 0..batch.channels[0].len() {
            for channel_data in &batch.channels {
                buffer.extend_from_slice(&channel_data[i].to_le_bytes());
            }
        }
    }
    buffer
}

pub fn setup_websocket_routes(
    config: Arc<Mutex<AdcConfig>>,
    csv_recorder: Arc<Mutex<CsvRecorder>>,
    config_applied_tx: broadcast::Sender<AdcConfig>,
    eeg_batch_rx: broadcast::Receiver<EegBatchData>,
    filtered_eeg_rx: broadcast::Receiver<FilteredEegData>,
    connection_manager: Arc<ConnectionManager>,
) -> (
    impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone,
    mpsc::Receiver<AdcConfig>,
) {
    // Channel for config updates from WebSocket to main
    let (config_update_tx, config_update_rx) = mpsc::channel::<AdcConfig>(32);

    // Route for configuration
    let config_route = warp::path("config")
        .and(warp::ws())
        .and(with_shared_state(config.clone()))
        .and(with_mpsc_tx(config_update_tx.clone()))
        .and(with_broadcast_rx(config_applied_tx.clone()))
        .and(with_atomic_bool(is_recording.clone()))
        .map(|ws: warp::ws::Ws, conf, tx, rx_applied, is_rec| {
            ws.on_upgrade(move |socket| handle_config_websocket(socket, conf, tx, rx_applied, is_rec))
        });

    // Route for recording control
    let command_route = warp::path("command")
        .and(warp::ws())
        .and(with_shared_state(csv_recorder.clone()))
        .and(with_shared_state(config.clone()))
        .and(with_mpsc_tx(config_update_tx.clone()))
        .map(|ws: warp::ws::Ws, is_rec, conf, tx| {
            ws.on_upgrade(move |socket| handle_command_websocket(socket, is_rec, conf, tx))
        });

    // Route for EEG data streaming
    let eeg_route = warp::path("eeg")
        .and(warp::ws())
        .and(with_broadcast_rx(eeg_batch_rx))
        .and(with_shared_state(connection_manager.clone()))
        .map(|ws: warp::ws::Ws, data_rx, conn_manager: Arc<ConnectionManager>| {
            ws.on_upgrade(move |socket| {
                let client_id = uuid::Uuid::new_v4().to_string();
                let conn_manager_clone = conn_manager.clone();
                async move {
                    conn_manager_clone.register_client_pipeline(client_id.clone(), ClientType::EegMonitor).await.ok();
                    handle_eeg_websocket(socket, data_rx).await;
                    conn_manager_clone.unregister_client_pipeline(&client_id).await.ok();
                }
            })
        });

    let routes = config_route.or(command_route).or(eeg_route);

    (routes, config_update_rx)
}