use warp::ws::{Message, WebSocket};
use warp::Filter;
use serde::{Serialize, Deserialize};
use futures_util::{StreamExt, SinkExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{broadcast, Mutex, mpsc};
use eeg_driver::AdcConfig;

use crate::driver_handler::{EegBatchData, CsvRecorder};

/// Command message for WebSocket control
#[derive(Deserialize)]
pub struct CommandMessage {
    pub command: String,
}

/// Configuration message for WebSocket control
#[derive(Deserialize)]
pub struct ConfigMessage {
    pub channels: Option<Vec<u32>>,
    pub sample_rate: Option<u32>,
}

/// Response message for WebSocket commands
#[derive(Serialize)]
pub struct CommandResponse {
    pub status: String,
    pub message: String,
}

/// Creates a binary EEG packet according to the specified format:
/// [timestamp (8 bytes)] [channel_samples...] for each channel in the data
pub fn create_eeg_binary_packet(eeg_batch_data: &EegBatchData) -> Vec<u8> {
    // Check if this is an error packet
    if let Some(error) = &eeg_batch_data.error {
        // For error packets, we'll use a special format:
        // - timestamp (8 bytes)
        // - error flag (1 byte, value 1)
        // - error message (UTF-8 encoded)
        let mut buffer = Vec::with_capacity(9 + error.len());
        
        // Write timestamp (8 bytes) in little-endian format
        buffer.extend_from_slice(&eeg_batch_data.timestamp.to_le_bytes());
        
        // Write error flag (1 byte)
        buffer.push(1);
        
        // Write error message as UTF-8 bytes
        buffer.extend_from_slice(error.as_bytes());
        
        return buffer;
    }
    
    // For normal data packets:
    // Get timestamp in milliseconds
    let timestamp = eeg_batch_data.timestamp;
    
    // Use the actual number of channels from the data
    let num_channels = eeg_batch_data.channels.len();
    
    // Check if we have any channels with data
    if num_channels == 0 {
        // Return just a timestamp with no data
        let mut buffer = Vec::with_capacity(9);
        buffer.extend_from_slice(&timestamp.to_le_bytes());
        buffer.push(0); // Not an error
        return buffer;
    }
    
    let samples_per_channel = eeg_batch_data.channels[0].len();
    
    // Calculate buffer size: 8 bytes for timestamp + 1 byte for error flag + 4 bytes per float per channel
    let buffer_size = 9 + (num_channels * samples_per_channel * 4);
    let mut buffer = Vec::with_capacity(buffer_size);
    
    // Write timestamp (8 bytes) in little-endian format
    buffer.extend_from_slice(&timestamp.to_le_bytes());
    
    // Write error flag (1 byte, value 0 for no error)
    buffer.push(0);
    
    // Write each channel's samples
    // Use all available channels from the data
    for channel_idx in 0..num_channels {
        let channel_data = if channel_idx < eeg_batch_data.channels.len() {
            &eeg_batch_data.channels[channel_idx]
        } else {
            // If we don't have enough channels, use the last available channel
            &eeg_batch_data.channels[eeg_batch_data.channels.len() - 1]
        };
        
        for &sample in channel_data {
            buffer.extend_from_slice(&sample.to_le_bytes());
        }
    }
    
    buffer
}

/// Handle WebSocket connection for EEG data streaming
pub async fn handle_websocket(ws: WebSocket, mut rx: broadcast::Receiver<EegBatchData>) {
    let (mut tx, _) = ws.split();
    
    println!("WebSocket client connected - sending binary EEG data");
    println!("Binary format: [timestamp (8 bytes)] [channel_samples...] for each channel");
    
    let mut packet_count = 0;
    let start_time = std::time::Instant::now();
    
    while let Ok(eeg_batch_data) = rx.recv().await {
        // Create binary packet
        let binary_data = create_eeg_binary_packet(&eeg_batch_data);
        let packet_size = binary_data.len();
        let samples_count = eeg_batch_data.channels[0].len();
        
        // Send binary message
        if let Err(_) = tx.send(Message::binary(binary_data)).await {
            println!("WebSocket client disconnected");
            break;
        }
        
        packet_count += 1;
        
        // Log stats every 100 packets
        if packet_count % 100 == 0 {
            let elapsed = start_time.elapsed().as_secs_f32();
            let rate = packet_count as f32 / elapsed;
            println!("Sent {} binary packets at {:.2} Hz", packet_count, rate);
            println!("Last packet size: {} bytes", packet_size);
            println!("Samples per channel: {}", samples_count);
        }
    }
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
            println!("Vref: {}", initial_config.Vref);
        }
    } else {
        println!("Error serializing configuration");
    }

    // Create an MPSC channel to forward messages from the spawned task to ws_tx
    let (mpsc_tx, mut mpsc_rx) = mpsc::channel::<Message>(32); // Channel for warp::ws::Message

    // Task to listen for applied config updates from main.rs and send them to the client via the MPSC channel
    let ws_tx_forwarder = mpsc_tx.clone();
    tokio::spawn(async move {
        loop {
            match config_applied_rx.recv().await {
                Ok(applied_config) => {
                    println!("Config WebSocket: Received applied config from main: {:?}", applied_config.channels);
                    if let Ok(config_json) = serde_json::to_string(&applied_config) {
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
                            
                            // Try to parse as ConfigMessage
                            match serde_json::from_str::<ConfigMessage>(text_from_client) {
                                Ok(mut config_msg) => {
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
                                    
                                    let mut config_guard = config.lock().await;
                                    let mut updated_config = config_guard.clone();
                                    let mut config_changed = false;
                                    let mut update_message = String::new();
                                    let no_params_provided = config_msg.channels.is_none() && config_msg.sample_rate.is_none();

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
                                        let mut unique_channels = new_channels.clone();
                                        unique_channels.sort();
                                        unique_channels.dedup();
                                        if unique_channels.len() != new_channels.len() {
                                            let response = CommandResponse { status: "error".to_string(), message: "Duplicate channels detected".to_string() };
                                            if let Ok(json) = serde_json::to_string(&response) {
                                                if let Err(e) = mpsc_tx.send(Message::text(json)).await {
                                                    println!("Config WebSocket: Error queueing 'duplicate channels' response: {}", e);
                                                }
                                            }
                                            continue;
                                        }
                                        let max_channel = *new_channels.iter().max().unwrap_or(&0);
                                        if max_channel > 7 {
                                            let response = CommandResponse { status: "error".to_string(), message: format!("Invalid channel index: {}. Max is 7.", max_channel) };
                                            if let Ok(json) = serde_json::to_string(&response) {
                                                if let Err(e) = mpsc_tx.send(Message::text(json)).await {
                                                    println!("Config WebSocket: Error queueing 'invalid channel idx' response: {}", e);
                                                }
                                            }
                                            continue;
                                        }
                                        let new_channels_usize: Vec<usize> = new_channels.iter().map(|&x| x as usize).collect();
                                        if updated_config.channels != new_channels_usize {
                                            updated_config.channels = new_channels_usize;
                                            config_changed = true;
                                            update_message = format!("channels: {:?}", updated_config.channels);
                                        }
                                    }

                                    if let Some(new_sample_rate) = config_msg.sample_rate {
                                        let valid_sample_rates = vec![250, 500, 1000, 2000];
                                        if !valid_sample_rates.contains(&new_sample_rate) {
                                            let response = CommandResponse { status: "error".to_string(), message: format!("Invalid sample rate: {}. Valid: {:?}", new_sample_rate, valid_sample_rates) };
                                            if let Ok(json) = serde_json::to_string(&response) {
                                                if let Err(e) = mpsc_tx.send(Message::text(json)).await {
                                                    println!("Config WebSocket: Error queueing 'invalid sample rate' response: {}", e);
                                                }
                                            }
                                            continue;
                                        }
                                        if updated_config.sample_rate != new_sample_rate {
                                            updated_config.sample_rate = new_sample_rate;
                                            config_changed = true;
                                            if !update_message.is_empty() { update_message.push_str(", "); }
                                            update_message.push_str(&format!("sample rate: {}", new_sample_rate));
                                        }
                                    }
                                    
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
    recorder: Arc<Mutex<CsvRecorder>>,
    is_recording: Arc<AtomicBool>
) {
    let (mut tx, mut rx) = ws.split();
    
    println!("Command WebSocket client connected");
    
    // Send initial status
    let initial_status = {
        let recorder_guard = recorder.lock().await;
        CommandResponse {
            status: "ok".to_string(),
            message: if is_recording.load(Ordering::Relaxed) {
                format!("Currently recording to {}", recorder_guard.file_path.clone().unwrap_or_default())
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
    let recorder_clone = recorder.clone();
    
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
                let recorder_guard = recorder_clone.lock().await;
                CommandResponse {
                    status: "ok".to_string(),
                    message: if is_recording_clone.load(Ordering::Relaxed) {
                        format!("Currently recording to {}", recorder_guard.file_path.clone().unwrap_or_default())
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
                
                let response = match serde_json::from_str::<CommandMessage>(text) {
                    Ok(cmd) => {
                        match cmd.command.as_str() {
                            "start" => {
                                if is_recording_local.load(Ordering::Relaxed) {
                                    CommandResponse {
                                        status: "error".to_string(),
                                        message: "Already recording".to_string(),
                                    }
                                } else {
                                    let mut recorder_guard = recorder.lock().await;
                                    match recorder_guard.start_recording().await {
                                        Ok(msg) => {
                                            // No need to update is_recording here as the recorder will do it
                                            CommandResponse {
                                                status: "ok".to_string(),
                                                message: msg,
                                            }
                                        },
                                        Err(e) => CommandResponse {
                                            status: "error".to_string(),
                                            message: format!("Failed to start recording: {}", e),
                                        },
                                    }
                                }
                            },
                            "stop" => {
                                let mut recorder_guard = recorder.lock().await;
                                match recorder_guard.stop_recording().await {
                                    Ok(msg) => {
                                        // No need to update is_recording here as the recorder will do it
                                        CommandResponse {
                                            status: "ok".to_string(),
                                            message: msg,
                                        }
                                    },
                                    Err(e) => CommandResponse {
                                        status: "error".to_string(),
                                        message: format!("Failed to stop recording: {}", e),
                                    },
                                }
                            },
                            "status" => {
                                let recorder_guard = recorder.lock().await;
                                CommandResponse {
                                    status: "ok".to_string(),
                                    message: if is_recording.load(Ordering::SeqCst) {
                                        format!("Currently recording to {}", recorder_guard.file_path.clone().unwrap_or_default())
                                    } else {
                                        "Not recording".to_string()
                                    },
                                }
                            },
                            _ => CommandResponse {
                                status: "error".to_string(),
                                message: format!("Unknown command: {}", cmd.command),
                            },
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

// Set up WebSocket routes and server
pub fn setup_websocket_routes(
    tx: broadcast::Sender<EegBatchData>, // For EEG data
    config: Arc<Mutex<AdcConfig>>, // Shared current config
    csv_recorder: Arc<Mutex<CsvRecorder>>,
    is_recording: Arc<AtomicBool>,
    config_applied_tx: broadcast::Sender<AdcConfig>, // Sender for applied configs (from main.rs)
) -> (impl warp::Filter<Extract = impl warp::Reply> + Clone, mpsc::Receiver<AdcConfig>) {
    // Channel for clients to send proposed config updates TO main.rs
    let (config_update_to_main_tx, config_update_to_main_rx) = mpsc::channel::<AdcConfig>(32);
    let eeg_ws_route = warp::path("eeg")
        .and(warp::ws())
        .and(warp::any().map(move || tx.subscribe()))
        .map(|ws: warp::ws::Ws, rx: broadcast::Receiver<EegBatchData>| {
            ws.on_upgrade(move |socket| handle_websocket(socket, rx))
        });
        
    let config_clone = config.clone(); // Arc for current config
    let config_update_to_main_tx_clone = config_update_to_main_tx.clone();
    let is_recording_clone_config = is_recording.clone(); // Separate clone for config route
    let config_applied_tx_clone = config_applied_tx.clone(); // Clone for the route

    let config_ws_route = warp::path("config")
        .and(warp::ws())
        .and(warp::any().map(move || config_clone.clone())) // Pass current config Arc
        .and(warp::any().map(move || config_update_to_main_tx_clone.clone())) // Pass sender for proposed updates
        .and(warp::any().map(move || config_applied_tx_clone.subscribe())) // Pass receiver for applied updates
        .and(warp::any().map(move || is_recording_clone_config.clone())) // Pass recording status
        .map(|ws: warp::ws::Ws, cfg_arc: Arc<Mutex<AdcConfig>>, cfg_upd_tx: mpsc::Sender<AdcConfig>, cfg_app_rx: broadcast::Receiver<AdcConfig>, rec_status: Arc<AtomicBool>| {
            println!("[DEBUG] /config route matched, attempting WebSocket upgrade...");
            ws.on_upgrade(move |socket| handle_config_websocket(socket, cfg_arc, cfg_upd_tx, cfg_app_rx, rec_status))
        });
    
    let recorder_clone = csv_recorder.clone();
    let is_recording_clone = is_recording.clone();
    let command_ws_route = warp::path("command")
        .and(warp::ws())
        .and(warp::any().map(move || recorder_clone.clone()))
        .and(warp::any().map(move || is_recording_clone.clone()))
        .map(|ws: warp::ws::Ws, recorder: Arc<Mutex<CsvRecorder>>, is_recording: Arc<AtomicBool>| {
            ws.on_upgrade(move |socket| handle_command_websocket(socket, recorder, is_recording))
        });
    
    (eeg_ws_route.or(config_ws_route).or(command_ws_route), config_update_to_main_rx)
}