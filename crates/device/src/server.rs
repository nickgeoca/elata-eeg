use warp::ws::{Message, WebSocket};
use warp::Filter;
use serde::{Serialize, Deserialize};
use futures_util::{StreamExt, SinkExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{broadcast, Mutex, mpsc};
use eeg_sensor::AdcConfig;
use crate::connection_manager;

#[derive(Clone, Debug)]
pub enum ClientType {
    EegMonitor,
}



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
    is_recording: Arc<AtomicBool>,
) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    println!("Configuration WebSocket client connected");

    // Create a channel to queue messages for the WebSocket sender.
    let (tx, mut rx) = mpsc::channel::<Message>(32);

    // Spawn a sender task that reads from the channel and sends to the WebSocket.
    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if ws_tx.send(message).await.is_err() {
                println!("Config WebSocket: client disconnected.");
                break;
            }
        }
    });

    // Send the initial configuration to the client.
    let initial_config = {
        let config_guard = config.lock().await;
        config_guard.clone()
    };
    if let Ok(config_json) = serde_json::to_string(&initial_config) {
        println!("Config WebSocket: Queuing initial config for client: {}", config_json);
        if tx.send(Message::text(config_json)).await.is_err() {
            println!("Config WebSocket: Failed to queue initial config. Client might have disconnected.");
            return; // Close connection if we can't even send the first message.
        }
    }

    // Spawn a task to listen for broadcasted config updates and forward them.
    let broadcast_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            match config_applied_rx.recv().await {
                Ok(applied_config) => {
                    if let Ok(config_json) = serde_json::to_string(&applied_config) {
                        if broadcast_tx.send(Message::text(config_json)).await.is_err() {
                            // Sender channel is closed, so the main task has ended.
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    println!("Config WebSocket: Lagged behind applied config broadcast by {} messages.", n);
                }
            }
        }
    });

    // Process incoming messages from the client in a loop.
    while let Some(result) = ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                println!("Config WebSocket: error receiving message: {}", e);
                break;
            }
        };

        if msg.is_close() {
            println!("Config WebSocket: Received close frame from client.");
            break;
        }

        if let Ok(text_from_client) = msg.to_str() {
            match serde_json::from_str::<ConfigMessage>(text_from_client) {
                Ok(config_msg) => {
                    if is_recording.load(Ordering::Relaxed) {
                        let response = CommandResponse {
                            status: "error".to_string(),
                            message: "Cannot change configuration during recording".to_string(),
                        };
                        if let Ok(json) = serde_json::to_string(&response) {
                            if tx.send(Message::text(json)).await.is_err() { break; }
                        }
                        continue;
                    }

                    let mut config_guard = config.lock().await;
                    let mut updated_config = config_guard.clone();
                    let mut config_changed = false;

                    if let Some(new_channels) = config_msg.channels {
                        if new_channels.is_empty() {
                            let response = CommandResponse { status: "error".to_string(), message: "Channel list cannot be empty".to_string() };
                            if let Ok(json) = serde_json::to_string(&response) {
                                if tx.send(Message::text(json)).await.is_err() { break; }
                            }
                            continue;
                        }
                        let new_channels_u8: Vec<u8> = new_channels.iter().map(|&x| x as u8).collect();
                        if updated_config.channels != new_channels_u8 {
                            updated_config.channels = new_channels_u8;
                            config_changed = true;
                        }
                    }

                    if let Some(new_sample_rate) = config_msg.sample_rate {
                        if updated_config.sample_rate != new_sample_rate {
                            updated_config.sample_rate = new_sample_rate;
                            config_changed = true;
                        }
                    }

                    if config_changed {
                        drop(config_guard);
                        if config_update_tx.send(updated_config).await.is_err() {
                            let response = CommandResponse { status: "error".to_string(), message: "Failed to submit update".to_string() };
                            if let Ok(json) = serde_json::to_string(&response) {
                                if tx.send(Message::text(json)).await.is_err() { break; }
                            }
                        } else {
                            let response = CommandResponse { status: "ok".to_string(), message: "Config update submitted".to_string() };
                            if let Ok(json) = serde_json::to_string(&response) {
                                if tx.send(Message::text(json)).await.is_err() { break; }
                            }
                        }
                    } else {
                        let response = CommandResponse { status: "ok".to_string(), message: "Configuration unchanged".to_string() };
                        if let Ok(json) = serde_json::to_string(&response) {
                            if tx.send(Message::text(json)).await.is_err() { break; }
                        }
                    }
                }
                Err(e) => {
                    let response = CommandResponse { status: "error".to_string(), message: format!("Invalid config format: {}", e) };
                    if let Ok(json) = serde_json::to_string(&response) {
                        if tx.send(Message::text(json)).await.is_err() { break; }
                    }
                }
            }
        }
    }

    println!("Config WebSocket: Connection handler finished for a client.");
}

/// Handle WebSocket connection for recording control commands
pub async fn handle_command_websocket(
    ws: WebSocket,
    is_recording: Arc<AtomicBool>,
    config: Arc<Mutex<AdcConfig>>,
    config_update_tx: mpsc::Sender<AdcConfig>,
) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    println!("Command WebSocket client connected");

    let (tx, mut rx) = mpsc::channel::<Message>(32);

    // Spawn a sender task.
    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if ws_tx.send(message).await.is_err() {
                println!("Command WebSocket: client disconnected.");
                break;
            }
        }
    });

    // Send initial status.
    let initial_status = CommandResponse {
        status: "ok".to_string(),
        message: if is_recording.load(Ordering::Relaxed) {
            "Currently recording".to_string()
        } else {
            "Not recording".to_string()
        },
    };
    if let Ok(status_json) = serde_json::to_string(&initial_status) {
        if tx.send(Message::text(status_json)).await.is_err() {
            return; // Disconnected.
        }
    }

    // Spawn a task for periodic status updates.
    let periodic_tx = tx.clone();
    let is_recording_clone = is_recording.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            let status_update = CommandResponse {
                status: "ok".to_string(),
                message: if is_recording_clone.load(Ordering::Relaxed) {
                    "Currently recording".to_string()
                } else {
                    "Not recording".to_string()
                },
            };
            if let Ok(status_json) = serde_json::to_string(&status_update) {
                if periodic_tx.send(Message::text(status_json)).await.is_err() {
                    break; // Disconnected.
                }
            }
        }
    });

    // Process incoming commands.
    while let Some(result) = ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                println!("Command WebSocket: error receiving message: {}", e);
                break;
            }
        };

        if msg.is_close() {
            break;
        }

        if let Ok(text) = msg.to_str() {
            let response = match serde_json::from_str::<DaemonCommand>(text) {
                Ok(daemon_cmd) => {
                    match daemon_cmd {
                        DaemonCommand::Start => {
                            is_recording.store(true, Ordering::Relaxed);
                            CommandResponse {
                                status: "ok".to_string(),
                                message: "Recording started (placeholder)".to_string(),
                            }
                        }
                        DaemonCommand::Stop => {
                            is_recording.store(false, Ordering::Relaxed);
                            CommandResponse {
                                status: "ok".to_string(),
                                message: "Recording stopped (placeholder)".to_string(),
                            }
                        }
                        DaemonCommand::Status => CommandResponse {
                            status: "ok".to_string(),
                            message: if is_recording.load(Ordering::SeqCst) {
                                "Currently recording".to_string()
                            } else {
                                "Not recording".to_string()
                            },
                        },
                        DaemonCommand::SetPowerlineFilter { value: new_powerline_filter_opt } => {
                            if is_recording.load(Ordering::Relaxed) {
                                CommandResponse {
                                    status: "error".to_string(),
                                    message: "Cannot change configuration during recording".to_string(),
                                }
                            } else {
                                let is_valid = match new_powerline_filter_opt {
                                    Some(val) => val == 50 || val == 60,
                                    None => true,
                                };
                                if !is_valid {
                                    CommandResponse {
                                        status: "error".to_string(),
                                        message: "Invalid powerline filter value".to_string(),
                                    }
                                } else {
                                    let mut config_guard = config.lock().await;
                                    // Powerline filter handling removed.
                                    let config_changed = false;
                                    if config_changed {
                                        // This block is now effectively dead code.
                                    }
                                    CommandResponse {
                                        status: "ok".to_string(),
                                        message: "Powerline filter configuration unchanged.".to_string(),
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => CommandResponse {
                    status: "error".to_string(),
                    message: format!("Invalid command format: {}", e),
                },
            };

            if let Ok(response_json) = serde_json::to_string(&response) {
                if tx.send(Message::text(response_json)).await.is_err() {
                    break; // Disconnected.
                }
            }
        }
    }

    println!("Command WebSocket client disconnected");
}

/// Handle WebSocket connection for EEG data streaming
pub async fn handle_eeg_websocket(
    ws: WebSocket,
    data_rx: broadcast::Receiver<Vec<u8>>, // Receiver for binary EEG data
) {
    println!("EEG data WebSocket client connected");
    let mut ws_receiver = connection_manager::split_and_spawn_sender(ws, data_rx);

    // Handle incoming messages from the client
    while let Some(result) = ws_receiver.next().await {
        match result {
            Ok(msg) => {
                if msg.is_close() {
                    println!("EEG WebSocket: Received close frame from client");
                    break;
                }
                // Ignore other message types for now
            }
            Err(e) => {
                println!("EEG WebSocket: Error receiving message: {}", e);
                break;
            }
        }
    }

    println!("EEG WebSocket: Connection handler finished");
}

/// Handle WebSocket connection for brain waves FFT data streaming
pub async fn handle_brain_waves_websocket(
    ws: WebSocket,
    data_rx: broadcast::Receiver<Vec<u8>>, // Receiver for binary FFT data
) {
    println!("Brain waves WebSocket client connected");
    let mut ws_receiver = connection_manager::split_and_spawn_sender(ws, data_rx);

    // Handle incoming messages from the client
    while let Some(result) = ws_receiver.next().await {
        match result {
            Ok(msg) => {
                if msg.is_close() {
                    println!("Brain waves WebSocket: Received close frame from client");
                    break;
                }
                // Ignore other message types for now
            }
            Err(e) => {
                println!("Brain waves WebSocket: Error receiving message: {}", e);
                break;
            }
        }
    }

    println!("Brain waves WebSocket: Connection handler finished");
}

/// Handle WebSocket connection for brain waves applet (JSON format expected by kiosk)
pub async fn handle_applet_brain_waves_websocket(
    ws: WebSocket,
    mut data_rx: broadcast::Receiver<Vec<u8>>, // Receiver for binary FFT data
) {
    use serde_json::json;
    
    println!("Brain waves applet WebSocket client connected");
    let (mut ws_tx, mut ws_rx) = ws.split();

    // Spawn a task to handle sending FFT data to the client in JSON format
    let send_task = tokio::spawn(async move {
        loop {
            match data_rx.recv().await {
                Ok(binary_data) => {
                    // Parse the binary FFT data and convert to JSON format expected by kiosk
                    match parse_fft_binary_data(&binary_data) {
                        Ok(fft_results) => {
                            let response = json!({
                                "timestamp": chrono::Utc::now().timestamp_millis(),
                                "fft_results": fft_results
                            });
                            
                            let json_str = response.to_string();
                            if ws_tx.send(Message::text(json_str)).await.is_err() {
                                println!("Brain waves applet WebSocket: Error sending JSON data, client likely disconnected");
                                break;
                            }
                        }
                        Err(e) => {
                            println!("Brain waves applet WebSocket: Error parsing FFT data: {}", e);
                            let error_response = json!({
                                "timestamp": chrono::Utc::now().timestamp_millis(),
                                "error": format!("Failed to parse FFT data: {}", e)
                            });
                            let json_str = error_response.to_string();
                            if ws_tx.send(Message::text(json_str)).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    println!("Brain waves applet WebSocket: Lagged by {} messages", n);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    println!("Brain waves applet WebSocket: Broadcast channel closed");
                    break;
                }
            }
        }
    });

    // Handle incoming messages from the client (mostly just close frames)
    let receive_task = tokio::spawn(async move {
        while let Some(result) = ws_rx.next().await {
            match result {
                Ok(msg) => {
                    if msg.is_close() {
                        println!("Brain waves applet WebSocket: Received close frame from client");
                        break;
                    }
                    // Ignore other message types for now
                }
                Err(e) => {
                    println!("Brain waves applet WebSocket: Error receiving message: {}", e);
                    break;
                }
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = send_task => {},
        _ = receive_task => {},
    }

    println!("Brain waves applet WebSocket: Connection handler finished");
}

/// Parse binary FFT data into the JSON format expected by the kiosk
fn parse_fft_binary_data(binary_data: &[u8]) -> Result<Vec<serde_json::Value>, String> {
    use serde_json::json;
    
    if binary_data.len() < 16 {
        return Err("Binary data too short".to_string());
    }
    
    let mut offset = 0;
    
    // Read timestamp (8 bytes)
    let _timestamp = u64::from_le_bytes(
        binary_data[offset..offset + 8].try_into()
            .map_err(|_| "Failed to read timestamp")?
    );
    offset += 8;
    
    // Read source_frame_id (8 bytes)
    let _source_frame_id = u64::from_le_bytes(
        binary_data[offset..offset + 8].try_into()
            .map_err(|_| "Failed to read source_frame_id")?
    );
    offset += 8;
    
    // Read number of channels (4 bytes)
    let num_channels = u32::from_le_bytes(
        binary_data[offset..offset + 4].try_into()
            .map_err(|_| "Failed to read num_channels")?
    ) as usize;
    offset += 4;
    
    let mut fft_results = Vec::new();
    
    // Read brain wave data for each channel
    for _channel_idx in 0..num_channels {
        if offset + 24 > binary_data.len() {
            return Err("Not enough data for channel brain waves".to_string());
        }
        
        // Read channel index (4 bytes)
        let _channel = u32::from_le_bytes(
            binary_data[offset..offset + 4].try_into()
                .map_err(|_| "Failed to read channel index")?
        );
        offset += 4;
        
        // Read brain wave band powers (5 * 4 bytes each)
        let delta = f32::from_le_bytes(
            binary_data[offset..offset + 4].try_into()
                .map_err(|_| "Failed to read delta")?
        );
        offset += 4;
        
        let theta = f32::from_le_bytes(
            binary_data[offset..offset + 4].try_into()
                .map_err(|_| "Failed to read theta")?
        );
        offset += 4;
        
        let alpha = f32::from_le_bytes(
            binary_data[offset..offset + 4].try_into()
                .map_err(|_| "Failed to read alpha")?
        );
        offset += 4;
        
        let beta = f32::from_le_bytes(
            binary_data[offset..offset + 4].try_into()
                .map_err(|_| "Failed to read beta")?
        );
        offset += 4;
        
        let gamma = f32::from_le_bytes(
            binary_data[offset..offset + 4].try_into()
                .map_err(|_| "Failed to read gamma")?
        );
        offset += 4;
        
        // Convert brain wave bands to frequency bins and power values
        // This is a simplified conversion - in a real implementation you might want
        // to generate more detailed frequency bins
        let frequencies = vec![2.0, 6.0, 10.5, 21.5, 65.0]; // Representative frequencies for each band
        let power = vec![delta, theta, alpha, beta, gamma];
        
        fft_results.push(json!({
            "power": power,
            "frequencies": frequencies
        }));
    }
    
    Ok(fft_results)
}

// Set up WebSocket routes and server


pub fn setup_websocket_routes(
    config: Arc<Mutex<AdcConfig>>,
    config_applied_tx: broadcast::Sender<AdcConfig>,
    eeg_data_tx: broadcast::Sender<Vec<u8>>,
    fft_data_tx: broadcast::Sender<Vec<u8>>,
    is_recording: Arc<AtomicBool>,
) -> (
    impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone,
    mpsc::Receiver<AdcConfig>,
) {
    // Channel for config updates from WebSocket to main
    let (config_update_tx, config_update_rx) = mpsc::channel::<AdcConfig>(32);

    let config_route = warp::path("config")
        .and(warp::ws())
        .and(with_shared_state(config.clone()))
        .and(with_mpsc_tx(config_update_tx.clone()))
        .and(with_broadcast_rx(config_applied_tx.clone()))
        .and(with_atomic_bool(is_recording.clone()))
        .map(|ws: warp::ws::Ws, conf, tx, rx, is_rec| {
            ws.on_upgrade(move |socket| {
                handle_config_websocket(socket, conf, tx, rx, is_rec)
            })
        });

    let command_route = warp::path("command")
        .and(warp::ws())
        .and(with_atomic_bool(is_recording.clone()))
        .and(with_shared_state(config.clone()))
        .and(with_mpsc_tx(config_update_tx.clone()))
        .map(|ws: warp::ws::Ws, is_rec, conf, tx| {
            ws.on_upgrade(move |socket| {
                handle_command_websocket(socket, is_rec, conf, tx)
            })
        });

    let eeg_route = warp::path("eeg")
        .and(warp::ws())
        .and(with_broadcast_rx(eeg_data_tx.clone()))
        .map(|ws: warp::ws::Ws, rx| {
            ws.on_upgrade(move |socket| handle_eeg_websocket(socket, rx))
        });

    let brain_waves_route = warp::path("brain_waves")
        .and(warp::ws())
        .and(with_broadcast_rx(fft_data_tx.clone()))
        .map(|ws: warp::ws::Ws, rx| {
            ws.on_upgrade(move |socket| handle_brain_waves_websocket(socket, rx))
        });

    // Add the applet route that the kiosk expects
    let applet_brain_waves_route = warp::path!("applet" / "brain_waves" / "data")
        .and(warp::ws())
        .and(with_broadcast_rx(fft_data_tx.clone()))
        .map(|ws: warp::ws::Ws, rx| {
            ws.on_upgrade(move |socket| handle_applet_brain_waves_websocket(socket, rx))
        });

    let routes = config_route.or(command_route).or(eeg_route).or(brain_waves_route).or(applet_brain_waves_route);

    (routes, config_update_rx)
}