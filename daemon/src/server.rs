use warp::ws::{Message, WebSocket};
use warp::Filter;
use serde::{Serialize, Deserialize, Deserializer};
use serde_json::Value;
use futures_util::{StreamExt, SinkExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{broadcast, Mutex, mpsc};
use eeg_driver::AdcConfig;

use crate::driver_handler::{EegBatchData, CsvRecorder, FilteredEegData}; // Added FilteredEegData

#[cfg(feature = "brain_waves_fft_feature")]
use elata_dsp_brain_waves_fft;

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

// Custom deserializer for Option<Option<u32>>
// Maps JSON `null` to `Some(None)` and missing field to `None`.
fn deserialize_option_option_u32<'de, D>(deserializer: D) -> Result<Option<Option<u32>>, D::Error>
where
    D: Deserializer<'de>,
{
    // Deserialize the field's content directly as a Value.
    // This bypasses Option<T>'s default "null -> None" behavior at this stage.
    let v = Value::deserialize(deserializer)?;

    match v {
        Value::Null => Ok(Some(None)), // If the value was JSON null, make it Some(None)
        actual_value => {
            // If it was some other JSON value, try to parse it as u32
            match serde_json::from_value::<u32>(actual_value) {
                Ok(n) => Ok(Some(Some(n))),
                Err(e) => Err(serde::de::Error::custom(e)),
            }
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct ConfigMessage {
    pub channels: Option<Vec<u32>>,
    pub sample_rate: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_option_option_u32")]
    pub powerline_filter_hz: Option<Option<u32>>,
}

impl ConfigMessage {
    // Helper method to debug the powerline filter value
    pub fn debug_powerline(&self) {
        println!("[CONFIG_DEBUG] ConfigMessage.powerline_filter_hz: {:?}", self.powerline_filter_hz);
        if let Some(inner) = &self.powerline_filter_hz {
            println!("[CONFIG_DEBUG] Inner value is: {:?}", inner);
            if inner.is_none() {
                println!("[CONFIG_DEBUG] CONFIRMED: Inner value is None (powerline filter OFF)");
            }
        }
    }
}

/// Response message for WebSocket commands
#[derive(Serialize)]
pub struct CommandResponse {
    pub status: String,
    pub message: String,
}

#[cfg(feature = "brain_waves_fft_feature")]
#[derive(Serialize, Debug, Clone)]
struct ChannelFftResult {
    power: Vec<f32>,
    frequencies: Vec<f32>,
}

#[cfg(feature = "brain_waves_fft_feature")]
#[derive(Serialize)]
struct BrainWavesAppletResponse {
    timestamp: u64,
    fft_results: Vec<ChannelFftResult>,
    error: Option<String>,
}

/// Creates a binary EEG packet.
/// Format: [timestamp_u64_le] [error_flag_u8] [payload]
/// error_flag_u8: 0 = no error, 1 = error
/// Payload:
///   If error_flag = 1: UTF-8 error message
///   If error_flag = 0: f32_le raw samples for each channel
pub fn create_eeg_binary_packet(eeg_batch_data: &EegBatchData) -> Vec<u8> {
    let mut buffer = Vec::new();

    // Write timestamp (8 bytes)
    buffer.extend_from_slice(&eeg_batch_data.timestamp.to_le_bytes());

    // Handle error packet
    if let Some(error_msg) = &eeg_batch_data.error {
        buffer.push(1); // error_flag = 1
        // No fft_flag needed here as error packets don't contain FFT data.
        buffer.extend_from_slice(error_msg.as_bytes());
        return buffer;
    }

    // No error, proceed with data
    buffer.push(0); // error_flag = 0

    // FFT data is no longer part of this binary packet.
    // Applets will receive FFT data via their dedicated JSON WebSocket.

    // Append raw channel data
    let num_raw_channels = eeg_batch_data.channels.len();
    if num_raw_channels > 0 {
        // It's implied that if channels is not empty, channels[0] exists.
        // If channels can be empty but power_spectrums is not, this needs adjustment.
        // For now, assuming if there's data, there are raw channels.
        for channel_data in &eeg_batch_data.channels {
            for &sample in channel_data {
                buffer.extend_from_slice(&sample.to_le_bytes());
            }
        }
    }
    // If num_raw_channels is 0 and no FFT data and no error, the packet will be:
    // timestamp (8) + error_flag (1, 0) + fft_flag (1, 0) = 10 bytes.
    // This case should be handled by the client if it's possible.

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
        if eeg_batch_data.channels.is_empty() {
            println!("Warning: Received EegBatchData with no channels, skipping packet.");
            continue; // Skip to the next message in the loop
        }
        // It's now safe to assume channels[0] exists
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

/// Handle WebSocket connection for FILTERED EEG data streaming
pub async fn handle_filtered_eeg_data_websocket(ws: WebSocket, mut rx: broadcast::Receiver<FilteredEegData>) {
    let (mut tx, _) = ws.split();
    
    println!("Filtered EEG Data WebSocket client connected - sending JSON data");
    
    let mut packet_count = 0;
    let start_time = std::time::Instant::now();
    
    while let Ok(filtered_data) = rx.recv().await {
        match serde_json::to_string(&filtered_data) {
            Ok(json_data) => {
                if let Err(_) = tx.send(Message::text(json_data)).await {
                    println!("Filtered EEG Data WebSocket client disconnected (send error)");
                    break;
                }
            }
            Err(e) => {
                println!("Error serializing FilteredEegData to JSON: {}", e);
                // Optionally send an error message to the client or skip
                let error_response = CommandResponse {
                    status: "error".to_string(),
                    message: format!("Error serializing data: {}", e),
                };
                if let Ok(json_error) = serde_json::to_string(&error_response) {
                    if tx.send(Message::text(json_error)).await.is_err() {
                        println!("Filtered EEG Data WebSocket client disconnected (send error on error serialization)");
                        break;
                    }
                }
                continue;
            }
        }
        
        packet_count += 1;
        
        // Log stats every 100 packets
        if packet_count % 100 == 0 {
            let elapsed = start_time.elapsed().as_secs_f32();
            if elapsed > 0.0 {
                let rate = packet_count as f32 / elapsed;
                println!("Sent {} filtered JSON packets at {:.2} Hz", packet_count, rate);
            }
        }
    }
    println!("Filtered EEG Data WebSocket connection handler finished.");
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
                    println!("Config WebSocket: Applied config powerline_filter_hz: {:?}", applied_config.powerline_filter_hz);
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
                                    config_msg.debug_powerline();
                                    
                                    // ADD THIS LOG
                                    println!("[SERVER_DEBUG] Parsed config_msg.powerline_filter_hz: {:?}", config_msg.powerline_filter_hz);

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
                                    println!("[SERVER_DEBUG] Config lock acquired. Current shared PL: {:?}", config_guard.powerline_filter_hz);
                                    let mut updated_config = config_guard.clone();
                                    let mut config_changed = false;
                                    let mut update_message = String::new();
                                    let no_params_provided = config_msg.channels.is_none() &&
                                                            config_msg.sample_rate.is_none() &&
                                                            config_msg.powerline_filter_hz.is_none();
 
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

                                    // Handle powerline filter update
                                    println!("[SERVER_DEBUG] Processing powerline filter. config_msg.powerline_filter_hz: {:?}", config_msg.powerline_filter_hz);
                                    
                                    if let Some(new_powerline_filter) = config_msg.powerline_filter_hz.clone() {
                                        // Validate the powerline filter value
                                        if new_powerline_filter.is_some() && ![50, 60].contains(&new_powerline_filter.unwrap()) {
                                            let response = CommandResponse {
                                                status: "error".to_string(),
                                                message: format!("Invalid powerline filter: {:?}. Valid: 50Hz, 60Hz, or null (off)", new_powerline_filter)
                                            };
                                            if let Ok(json) = serde_json::to_string(&response) {
                                                if let Err(e) = mpsc_tx.send(Message::text(json)).await {
                                                    println!("Config WebSocket: Error queueing 'invalid powerline filter' response: {}", e);
                                                }
                                            }
                                            continue;
                                        }
                                        
                                        println!("[SERVER_DEBUG] Comparing powerline_filter_hz: current_in_shared_config={:?}, from_client_message={:?}",
                                            updated_config.powerline_filter_hz, new_powerline_filter);
                                        
                                        // ALWAYS update powerline filter when explicitly set to None
                                        if new_powerline_filter.is_none() {
                                            println!("[SERVER_DEBUG] CRITICAL: Setting powerline filter to None (turning off)");
                                            updated_config.powerline_filter_hz = None;
                                            config_changed = true;
                                            if !update_message.is_empty() { update_message.push_str(", "); }
                                            update_message.push_str("powerline filter: Off");
                                        }
                                        // Otherwise, only update if different
                                        else if updated_config.powerline_filter_hz != new_powerline_filter {
                                            println!("[SERVER_DEBUG] Updating powerline filter from {:?} to {:?}",
                                                updated_config.powerline_filter_hz, new_powerline_filter);
                                            updated_config.powerline_filter_hz = new_powerline_filter;
                                            config_changed = true;
                                            if !update_message.is_empty() { update_message.push_str(", "); }
                                            update_message.push_str(&format!("powerline filter: {:?}",
                                                new_powerline_filter.map_or("Off".to_string(), |f| f.to_string() + "Hz")));
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
    is_recording: Arc<AtomicBool>,
    config: Arc<Mutex<AdcConfig>>,         // Added
    config_update_tx: mpsc::Sender<AdcConfig> // Added
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
                
                let response = match serde_json::from_str::<DaemonCommand>(text) {
                    Ok(daemon_cmd) => {
                        match daemon_cmd {
                            DaemonCommand::Start => {
                                if is_recording_local.load(Ordering::Relaxed) {
                                    CommandResponse {
                                        status: "error".to_string(),
                                        message: "Already recording".to_string(),
                                    }
                                } else {
                                    let mut recorder_guard = recorder.lock().await;
                                    match recorder_guard.start_recording().await {
                                        Ok(msg) => {
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
                            DaemonCommand::Stop => {
                                let mut recorder_guard = recorder.lock().await;
                                match recorder_guard.stop_recording().await {
                                    Ok(msg) => {
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
                            DaemonCommand::Status => {
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
                                        let mut updated_config = config_guard.clone();
                                        let mut config_changed = false;

                                        if updated_config.powerline_filter_hz != new_powerline_filter_opt {
                                            updated_config.powerline_filter_hz = new_powerline_filter_opt;
                                            config_changed = true;
                                        }

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

#[cfg(feature = "brain_waves_fft_feature")]
async fn handle_brain_waves_fft_websocket(
    ws: WebSocket,
    mut rx_eeg: broadcast::Receiver<EegBatchData>,
    config_arc: Arc<Mutex<AdcConfig>>,
) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    println!("Brain Waves FFT WebSocket client connected");

    const FFT_WINDOW_DURATION_SECONDS: f32 = 1.0; // Process 1 second of data for FFT
    const FFT_WINDOW_SLIDE_SECONDS: f32 = 0.5; // Slide window by 0.5 seconds (50% overlap if duration is 1s)

    let mut channel_buffers: Vec<Vec<f32>> = Vec::new();
    let mut num_channels = 0;
    let mut sample_rate_f32 = 250.0; // Default, will be updated

    // Initialize buffers and parameters based on current config
    {
        let config_guard = config_arc.lock().await;
        num_channels = config_guard.channels.len();
        sample_rate_f32 = config_guard.sample_rate as f32;
        channel_buffers = vec![Vec::new(); num_channels];
        println!(
            "Brain Waves FFT: Initialized for {} channels, sample rate: {} Hz",
            num_channels, sample_rate_f32
        );
    }

    let fft_window_samples = (sample_rate_f32 * FFT_WINDOW_DURATION_SECONDS).round() as usize;
    let fft_slide_samples = (sample_rate_f32 * FFT_WINDOW_SLIDE_SECONDS).round() as usize;

    if num_channels == 0 {
        println!("Brain Waves FFT: Error - 0 channels configured. Closing WebSocket.");
        let response = BrainWavesAppletResponse {
            timestamp: 0,
            fft_results: Vec::new(),
            error: Some("Daemon configured with 0 channels for FFT.".to_string()),
        };
        if let Ok(json_response) = serde_json::to_string(&response) {
            let _ = ws_tx.send(Message::text(json_response)).await;
        }
        return;
    }
    
    println!(
        "Brain Waves FFT: Window size: {} samples, Slide size: {} samples",
        fft_window_samples, fft_slide_samples
    );

    loop {
        tokio::select! {
            Ok(eeg_batch_data) = rx_eeg.recv() => {
                if let Some(err_msg) = &eeg_batch_data.error {
                    println!("Brain Waves FFT: Received error in EegBatchData: {}", err_msg);
                    let response = BrainWavesAppletResponse {
                        timestamp: eeg_batch_data.timestamp,
                        fft_results: Vec::new(),
                        error: Some(err_msg.clone()),
                    };
                    if let Ok(json_response) = serde_json::to_string(&response) {
                        if ws_tx.send(Message::text(json_response)).await.is_err() {
                            println!("Brain Waves FFT: WebSocket client disconnected while sending error.");
                            break;
                        }
                    }
                    continue;
                }

                if eeg_batch_data.channels.len() != num_channels {
                    println!(
                        "Brain Waves FFT: Mismatch in channel count. Expected {}, got {}. Re-initializing.",
                        num_channels, eeg_batch_data.channels.len()
                    );
                    // Potentially re-initialize or send error
                    // For now, let's update num_channels and re-initialize buffers if it's the first valid data
                    // Or if it changes mid-stream, which might indicate a config change not fully propagated here.
                    // A more robust solution might involve listening to config_applied_rx as well.
                    let config_guard = config_arc.lock().await;
                    num_channels = config_guard.channels.len(); // Re-fetch from potentially updated config
                    sample_rate_f32 = config_guard.sample_rate as f32; // Re-fetch sample rate
                    channel_buffers = vec![Vec::new(); num_channels]; // Re-initialize buffers
                    // fft_window_samples and fft_slide_samples would also need recalculation here.
                    // This simple re-init might lose some buffered data.
                    if num_channels == 0 || eeg_batch_data.channels.len() != num_channels {
                        let err_msg = format!("Channel count mismatch or 0 channels after re-check. Expected {}, got {}. Aborting.", num_channels, eeg_batch_data.channels.len());
                        println!("Brain Waves FFT: {}", err_msg);
                         let response = BrainWavesAppletResponse {
                            timestamp: eeg_batch_data.timestamp,
                            fft_results: Vec::new(),
                            error: Some(err_msg),
                        };
                        if let Ok(json_response) = serde_json::to_string(&response) {
                            if ws_tx.send(Message::text(json_response)).await.is_err() {
                                break;
                            }
                        }
                        continue;
                    }
                }

                for (i, data_vec) in eeg_batch_data.channels.iter().enumerate() {
                    if i < num_channels {
                        channel_buffers[i].extend_from_slice(data_vec);
                    }
                }

                let mut all_channel_fft_results: Vec<ChannelFftResult> = Vec::with_capacity(num_channels);
                let mut processing_error: Option<String> = None;

                for i in 0..num_channels {
                    if channel_buffers[i].len() >= fft_window_samples {
                        let window_data: Vec<f32> = channel_buffers[i][..fft_window_samples].to_vec();
                        
                        // Perform FFT
                        match elata_dsp_brain_waves_fft::process_eeg_data(&window_data, sample_rate_f32) {
                            Ok((power, frequencies)) => {
                                all_channel_fft_results.push(ChannelFftResult { power, frequencies });
                            }
                            Err(fft_err) => {
                                println!("Brain Waves FFT: Error processing FFT for channel {}: {}", i, fft_err);
                                processing_error = Some(format!("FFT error on channel {}: {}", i, fft_err));
                                // Add an empty result to maintain channel order if one fails
                                all_channel_fft_results.push(ChannelFftResult { power: Vec::new(), frequencies: Vec::new()});
                            }
                        }
                        
                        // Slide window: remove processed part
                        if fft_slide_samples > 0 && channel_buffers[i].len() >= fft_slide_samples {
                            channel_buffers[i].drain(0..fft_slide_samples);
                        } else {
                            // If slide is too large or buffer too small, just clear (or keep remaining for next full window)
                            channel_buffers[i].clear();
                        }
                    } else {
                        // Not enough data yet for this channel, push empty result to maintain order
                        // Or, decide not to send until all channels have data. For now, send what we have.
                         all_channel_fft_results.push(ChannelFftResult { power: Vec::new(), frequencies: Vec::new()});
                    }
                }
                
                // Only send if we have some results or an error to report
                if !all_channel_fft_results.iter().all(|res| res.power.is_empty()) || processing_error.is_some() {
                    let response = BrainWavesAppletResponse {
                        timestamp: eeg_batch_data.timestamp, // Use timestamp of the incoming batch
                        fft_results: all_channel_fft_results,
                        error: processing_error,
                    };

                    if let Ok(json_response) = serde_json::to_string(&response) {
                        if ws_tx.send(Message::text(json_response)).await.is_err() {
                            println!("Brain Waves FFT: WebSocket client disconnected while sending data.");
                            break;
                        }
                    } else {
                        println!("Brain Waves FFT: Error serializing response for WebSocket");
                    }
                }
            },
            Some(result) = ws_rx.next() => {
                match result {
                    Ok(msg) => {
                        if msg.is_close() {
                            println!("Brain Waves FFT WebSocket: client sent close frame.");
                            break;
                        }
                        println!("Brain Waves FFT WebSocket: received message: {:?}", msg);
                    }
                    Err(e) => {
                        println!("Brain Waves FFT WebSocket: error receiving message: {}", e);
                        break;
                    }
                }
            },
            else => {
                println!("Brain Waves FFT WebSocket: both streams closed.");
                break;
            }
        }
    }
    println!("Brain Waves FFT WebSocket connection handler finished.");
}

// Set up WebSocket routes and server
pub fn setup_websocket_routes(
    tx_eeg_batch_data: broadcast::Sender<EegBatchData>, // Renamed from tx, for existing /eeg endpoint
    tx_filtered_eeg_data: broadcast::Sender<FilteredEegData>, // New sender for filtered data
    config: Arc<Mutex<AdcConfig>>, // Shared current config
    csv_recorder: Arc<Mutex<CsvRecorder>>,
    is_recording: Arc<AtomicBool>,
    config_applied_tx: broadcast::Sender<AdcConfig>, // Sender for applied configs (from main.rs)
) -> (impl warp::Filter<Extract = impl warp::Reply> + Clone, mpsc::Receiver<AdcConfig>) {
    // Channel for clients to send proposed config updates TO main.rs
    let (config_update_to_main_tx, config_update_to_main_rx) = mpsc::channel::<AdcConfig>(32);
    
    // Existing /eeg endpoint for EegBatchData (typically unfiltered or pre-basic_voltage_filter)
    let eeg_ws_route = warp::path("eeg")
        .and(warp::ws())
        .and(with_broadcast_rx(tx_eeg_batch_data.clone())) // Use the renamed sender
        .map(|ws: warp::ws::Ws, rx: broadcast::Receiver<EegBatchData>| {
            ws.on_upgrade(move |socket| handle_websocket(socket, rx))
        });

    // New /ws/eeg/data__basic_voltage_filter endpoint for FilteredEegData
    let filtered_eeg_data_route = warp::path("ws")
        .and(warp::path("eeg"))
        .and(warp::path("data__basic_voltage_filter"))
        .and(warp::ws())
        .and(with_broadcast_rx(tx_filtered_eeg_data.clone())) // Use the new sender
        .map(|ws: warp::ws::Ws, rx_data: broadcast::Receiver<FilteredEegData>| {
            ws.on_upgrade(move |socket| handle_filtered_eeg_data_websocket(socket, rx_data))
        });
        
    let config_clone = config.clone();
    let config_update_to_main_tx_clone = config_update_to_main_tx.clone();
    let is_recording_clone_config = is_recording.clone();
    let config_applied_tx_clone = config_applied_tx.clone();

    let config_ws_route = warp::path("config")
        .and(warp::ws())
        .and(with_shared_state(config_clone)) // Pass current config Arc
        .and(with_mpsc_tx(config_update_to_main_tx_clone)) // Pass sender for proposed updates
        .and(with_broadcast_rx(config_applied_tx_clone)) // Pass receiver for applied updates
        .and(with_atomic_bool(is_recording_clone_config)) // Pass recording status
        .map(|ws: warp::ws::Ws, cfg_arc: Arc<Mutex<AdcConfig>>, cfg_upd_tx: mpsc::Sender<AdcConfig>, cfg_app_rx: broadcast::Receiver<AdcConfig>, rec_status: Arc<AtomicBool>| {
            println!("[DEBUG] /config route matched, attempting WebSocket upgrade...");
            ws.on_upgrade(move |socket| handle_config_websocket(socket, cfg_arc, cfg_upd_tx, cfg_app_rx, rec_status))
        });
    
    let recorder_clone_cmd = csv_recorder.clone();
    let is_recording_clone_cmd = is_recording.clone();
    let config_clone_cmd = config.clone();
    let config_update_tx_clone_cmd = config_update_to_main_tx.clone();

    let command_ws_route = warp::path("command")
        .and(warp::ws())
        .and(with_shared_recorder(recorder_clone_cmd))
        .and(with_atomic_bool(is_recording_clone_cmd))
        .and(with_shared_state(config_clone_cmd)) // Pass cloned config
        .and(with_mpsc_tx(config_update_tx_clone_cmd)) // Pass cloned sender
        .map(|ws: warp::ws::Ws, recorder: Arc<Mutex<CsvRecorder>>, is_recording: Arc<AtomicBool>, cfg: Arc<Mutex<AdcConfig>>, cfg_upd_tx: mpsc::Sender<AdcConfig>| {
            ws.on_upgrade(move |socket| handle_command_websocket(socket, recorder, is_recording, cfg, cfg_upd_tx))
        });
    
    // Combine base routes including the new filtered data route
    let base_routes = eeg_ws_route
        .or(config_ws_route)
        .or(command_ws_route)
        .or(filtered_eeg_data_route);

    #[cfg(feature = "brain_waves_fft_feature")]
    let routes = {
        // Ensure tx_eeg_batch_data is used for the FFT route as it expects unfiltered data
        let tx_for_fft_route = tx_eeg_batch_data.clone();
        let brain_waves_fft_ws_route = warp::path("applet") // Corrected path as per original
            .and(warp::path("brain_waves"))
            .and(warp::path("data"))
            .and(warp::ws())
            .and(with_broadcast_rx(tx_for_fft_route)) // Use the EegBatchData sender
            .and(with_shared_state(config.clone())) // Pass AdcConfig
            .map(|ws: warp::ws::Ws, rx_eeg: broadcast::Receiver<EegBatchData>, current_config: Arc<Mutex<AdcConfig>>| {
                ws.on_upgrade(move |socket| handle_brain_waves_fft_websocket(socket, rx_eeg, current_config))
            });
        base_routes.or(brain_waves_fft_ws_route).boxed()
    };
    #[cfg(not(feature = "brain_waves_fft_feature"))]
    let routes = base_routes.boxed();

    (routes, config_update_to_main_rx)
}