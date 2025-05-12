use warp::ws::{Message, WebSocket};
use warp::Filter;
use serde::{Serialize, Deserialize};
use futures_util::{StreamExt, SinkExt};
use std::sync::Arc;
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
    config_update_tx: mpsc::Sender<AdcConfig>,
    is_recording: Arc<Mutex<bool>>
) {
    let (mut tx, mut rx) = ws.split(); // Keep both sender (tx) and receiver (rx)
    
    println!("Configuration WebSocket client connected");
    
    // Send initial configuration to the client
    let initial_config = {
        let config_guard = config.lock().await;
        config_guard.clone()
    };
    
    // Convert the configuration to JSON and send it
    if let Ok(config_json) = serde_json::to_string(&initial_config) {
        if let Err(e) = tx.send(Message::text(config_json)).await {
            println!("Error sending configuration: {}", e);
        } else {
            println!("Configuration sent successfully");
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
    
    // Process incoming configuration messages
    while let Some(result) = rx.next().await {
        match result {
            Ok(msg) => {
                if msg.is_close() {
                    println!("Config WebSocket: Received close frame");
                    break; // Exit loop on close frame
                }
                
                if msg.is_text() {
                    let text = msg.to_str().unwrap_or_default();
                    
                    // Try to parse as ConfigMessage
                    match serde_json::from_str::<ConfigMessage>(text) {
                        Ok(config_msg) => {
                            // Check if recording is in progress
                            let is_recording_guard = is_recording.lock().await;
                            if *is_recording_guard {
                                // Cannot change config during recording
                                let response = CommandResponse {
                                    status: "error".to_string(),
                                    message: "Cannot change configuration during recording".to_string(),
                                };
                                
                                if let Ok(response_json) = serde_json::to_string(&response) {
                                    if let Err(e) = tx.send(Message::text(response_json)).await {
                                        println!("Error sending config error response: {}", e);
                                    }
                                }
                                continue;
                            }
                            drop(is_recording_guard); // Release the lock
                            
                            // Update configuration if channels are provided
                            if let Some(new_channels) = config_msg.channels {
                                let mut config_guard = config.lock().await;
                                
                                // Basic validation
                                if new_channels.is_empty() {
                                    let response = CommandResponse {
                                        status: "error".to_string(),
                                        message: "Channel list cannot be empty".to_string(),
                                    };
                                    
                                    if let Ok(response_json) = serde_json::to_string(&response) {
                                        if let Err(e) = tx.send(Message::text(response_json)).await {
                                            println!("Error sending config error response: {}", e);
                                        }
                                    }
                                    continue;
                                }
                                
                                // Update channels
                                config_guard.channels = new_channels;
                                let updated_config = config_guard.clone();
                                drop(config_guard); // Release the lock
                                
                                // Send the updated config via the channel
                                if let Err(e) = config_update_tx.send(updated_config.clone()).await {
                                    println!("Error sending config update: {}", e);
                                    
                                    let response = CommandResponse {
                                        status: "error".to_string(),
                                        message: format!("Failed to update configuration: {}", e),
                                    };
                                    
                                    if let Ok(response_json) = serde_json::to_string(&response) {
                                        if let Err(e) = tx.send(Message::text(response_json)).await {
                                            println!("Error sending config error response: {}", e);
                                        }
                                    }
                                } else {
                                    // Send success response
                                    let response = CommandResponse {
                                        status: "ok".to_string(),
                                        message: format!("Configuration updated with channels: {:?}", new_channels),
                                    };
                                    
                                    if let Ok(response_json) = serde_json::to_string(&response) {
                                        if let Err(e) = tx.send(Message::text(response_json)).await {
                                            println!("Error sending config success response: {}", e);
                                        }
                                    }
                                    
                                    // Also send the updated config
                                    if let Ok(config_json) = serde_json::to_string(&updated_config) {
                                        if let Err(e) = tx.send(Message::text(config_json)).await {
                                            println!("Error sending updated configuration: {}", e);
                                        }
                                    }
                                }
                            } else {
                                // No channels provided
                                let response = CommandResponse {
                                    status: "error".to_string(),
                                    message: "No channels provided in configuration update".to_string(),
                                };
                                
                                if let Ok(response_json) = serde_json::to_string(&response) {
                                    if let Err(e) = tx.send(Message::text(response_json)).await {
                                        println!("Error sending config error response: {}", e);
                                    }
                                }
                            }
                        },
                        Err(e) => {
                            println!("Error parsing config message: {}", e);
                            let response = CommandResponse {
                                status: "error".to_string(),
                                message: format!("Invalid configuration format: {}", e),
                            };
                            
                            if let Ok(response_json) = serde_json::to_string(&response) {
                                if let Err(e) = tx.send(Message::text(response_json)).await {
                                    println!("Error sending config error response: {}", e);
                                }
                            }
                        }
                    }
                } else {
                    println!("Config WebSocket: Received non-text message: {:?}", msg);
                }
            },
            Err(e) => {
                println!("Config WebSocket: Error receiving message: {}", e);
                break; // Exit loop on error
            }
        }
    }
  
    println!("Config WebSocket: Connection handler finished.");
}

/// Handle WebSocket connection for recording control commands
pub async fn handle_command_websocket(
    ws: WebSocket,
    recorder: Arc<Mutex<CsvRecorder>>,
    is_recording: Arc<Mutex<bool>>
) {
    let (mut tx, mut rx) = ws.split();
    
    println!("Command WebSocket client connected");
    
    // Send initial status
    let initial_status = {
        let recorder_guard = recorder.lock().await;
        CommandResponse {
            status: "ok".to_string(),
            message: if recorder_guard.is_recording {
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
    
    // Spawn a task to handle periodic status updates
    let status_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        
        loop {
            interval.tick().await;
            
            let status_update = {
                let recorder_guard = recorder_clone.lock().await;
                CommandResponse {
                    status: "ok".to_string(),
                    message: if recorder_guard.is_recording {
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
                let response = match serde_json::from_str::<CommandMessage>(text) {
                    Ok(cmd) => {
                        match cmd.command.as_str() {
                            "start" => {
                                let mut is_recording_guard = is_recording.lock().await;
                                if *is_recording_guard {
                                    CommandResponse {
                                        status: "error".to_string(),
                                        message: "Already recording".to_string(),
                                    }
                                } else {
                                    let mut recorder_guard = recorder.lock().await;
                                    match recorder_guard.start_recording() {
                                        Ok(msg) => {
                                            *is_recording_guard = true;
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
                                let mut is_recording_guard = is_recording.lock().await;
                                let mut recorder_guard = recorder.lock().await;
                                match recorder_guard.stop_recording() {
                                    Ok(msg) => {
                                        *is_recording_guard = false;
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
                                let is_recording_guard = is_recording.lock().await;
                                let recorder_guard = recorder.lock().await;
                                CommandResponse {
                                    status: "ok".to_string(),
                                    message: if *is_recording_guard {
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
    tx: broadcast::Sender<EegBatchData>,
    config: Arc<Mutex<AdcConfig>>,
    csv_recorder: Arc<Mutex<CsvRecorder>>,
    is_recording: Arc<Mutex<bool>>,
) -> (impl warp::Filter<Extract = impl warp::Reply> + Clone, mpsc::Receiver<AdcConfig>) {
    // Create a channel for config updates
    let (config_update_tx, config_update_rx) = mpsc::channel::<AdcConfig>(32);
    let eeg_ws_route = warp::path("eeg")
        .and(warp::ws())
        .and(warp::any().map(move || tx.subscribe()))
        .map(|ws: warp::ws::Ws, rx: broadcast::Receiver<EegBatchData>| {
            ws.on_upgrade(move |socket| handle_websocket(socket, rx))
        });
        
    let config_clone = config.clone();
    let config_update_tx_clone = config_update_tx.clone();
    let is_recording_clone = is_recording.clone();
    let config_ws_route = warp::path("config")
        .and(warp::ws())
        .and(warp::any().map(move || config_clone.clone()))
        .and(warp::any().map(move || config_update_tx_clone.clone()))
        .and(warp::any().map(move || is_recording_clone.clone()))
        .map(|ws: warp::ws::Ws, config: Arc<Mutex<AdcConfig>>, config_update_tx: mpsc::Sender<AdcConfig>, is_recording: Arc<Mutex<bool>>| {
            ws.on_upgrade(move |socket| handle_config_websocket(socket, config, config_update_tx, is_recording))
        });
    
    let recorder_clone = csv_recorder.clone();
    let is_recording_clone = is_recording.clone();
    let command_ws_route = warp::path("command")
        .and(warp::ws())
        .and(warp::any().map(move || recorder_clone.clone()))
        .and(warp::any().map(move || is_recording_clone.clone()))
        .map(|ws: warp::ws::Ws, recorder: Arc<Mutex<CsvRecorder>>, is_recording: Arc<Mutex<bool>>| {
            ws.on_upgrade(move |socket| handle_command_websocket(socket, recorder, is_recording))
        });
    
    (eeg_ws_route.or(config_ws_route).or(command_ws_route), config_update_rx)
}