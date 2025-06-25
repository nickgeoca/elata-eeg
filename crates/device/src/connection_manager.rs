//! Manages WebSocket connections for streaming data to UI clients.
use anyhow::Result;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, error, debug};
use uuid::Uuid;
use warp::ws::{Message, WebSocket};
use eeg_types::SensorEvent;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

type ClientId = String;

/// Defines the structure for messages sent from the client to the server.
#[derive(Deserialize, Debug)]
struct ClientMessage {
    action: String,
    topics: Vec<String>,
}

/// Represents a connected WebSocket client with its subscriptions.
struct Client {
    /// The sending half of the WebSocket connection.
    sender: SplitSink<WebSocket, Message>,
    /// The set of topics the client is subscribed to.
    subscriptions: HashSet<String>,
}

/// Manages all WebSocket client connections and their subscriptions.
pub struct ConnectionManager {
    connection_rx: mpsc::Receiver<WebSocket>,
    event_rx: broadcast::Receiver<SensorEvent>,
    clients: Arc<Mutex<HashMap<ClientId, Client>>>,
}

impl ConnectionManager {
    pub fn new(
        connection_rx: mpsc::Receiver<WebSocket>,
        event_rx: broadcast::Receiver<SensorEvent>,
    ) -> Self {
        Self {
            connection_rx,
            event_rx,
            clients: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run(&mut self, shutdown_token: CancellationToken) {
        info!("ConnectionManager is running.");
        loop {
            tokio::select! {
                biased;
                _ = shutdown_token.cancelled() => {
                    info!("ConnectionManager received shutdown signal.");
                    break;
                }
                Some(websocket) = self.connection_rx.recv() => {
                    self.add_client(websocket);
                }
                Ok(event) = self.event_rx.recv() => {
                    self.dispatch_event(event).await;
                }
            }
        }
        info!("ConnectionManager has shut down.");
    }

    /// Adds a new client and spawns a task to handle its messages.
    fn add_client(&mut self, websocket: WebSocket) {
        let client_id = Uuid::new_v4().to_string();
        let (sender, mut receiver) = websocket.split();
        let clients_arc = self.clients.clone();

        info!(client_id = %client_id, "New WebSocket client connected.");

        // Initial client state with no subscriptions
        let client = Client {
            sender,
            subscriptions: HashSet::new(),
        };
        
        // Spawn a task to manage this specific client's lifecycle
        let client_id_clone = client_id.clone();
        tokio::spawn(async move {
            // Insert the client into the shared map
            clients_arc.lock().await.insert(client_id_clone.clone(), client);

            loop {
                match receiver.next().await {
                    Some(Ok(msg)) => {
                        if msg.is_close() {
                            debug!(client_id = %client_id_clone, "Client sent close frame.");
                            break;
                        }
                        if let Ok(text) = msg.to_str() {
                            if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(text) {
                                let mut clients_guard = clients_arc.lock().await;
                                if let Some(client) = clients_guard.get_mut(&client_id_clone) {
                                    match client_msg.action.as_str() {
                                        "subscribe" => {
                                            info!(client_id = %client_id_clone, "Subscribing to topics: {:?}", client_msg.topics);
                                            client.subscriptions.extend(client_msg.topics);
                                        }
                                        "unsubscribe" => {
                                            info!(client_id = %client_id_clone, "Unsubscribing from topics: {:?}", client_msg.topics);
                                            for topic in client_msg.topics {
                                                client.subscriptions.remove(&topic);
                                            }
                                        }
                                        _ => warn!("Unknown client message action: {}", client_msg.action),
                                    }
                                }
                            }
                        }
                    }
                    Some(Err(e)) => {
                        warn!(client_id = %client_id_clone, "Error receiving from client: {}", e);
                        break;
                    }
                    None => {
                        // Stream has ended
                        break;
                    }
                }
            }
            // Cleanup: Remove the client from the map upon disconnection
            info!(client_id = %client_id_clone, "Client disconnected. Removing.");
            clients_arc.lock().await.remove(&client_id_clone);
        });
    }

    /// Dispatches an event to all clients subscribed to its topic.
    async fn dispatch_event(&self, event: SensorEvent) {
        let (topic, message) = match self.create_message_from_event(event) {
            Some(result) => result,
            None => return, // Event is not for UI clients
        };

        let mut clients_guard = self.clients.lock().await;
        if clients_guard.is_empty() {
            return;
        }

        let mut disconnected_clients = Vec::new();

        for (id, client) in clients_guard.iter_mut() {
            if client.subscriptions.contains(&topic) {
                if let Err(e) = client.sender.send(message.clone()).await {
                    warn!(client_id = %id, "Failed to send message, client disconnected: {}", e);
                    disconnected_clients.push(id.clone());
                }
            }
        }

        // Clean up disconnected clients
        for id in disconnected_clients {
            clients_guard.remove(&id);
            info!(client_id = %id, "Removed disconnected client.");
        }
    }

    /// Creates a topic and a WebSocket message from a SensorEvent.
    fn create_message_from_event(&self, event: SensorEvent) -> Option<(String, Message)> {
        match event {
            SensorEvent::FilteredEeg(packet) => {
                let binary_data = eeg_packet_to_binary(&packet);
                Some(("FilteredEeg".to_string(), Message::binary(binary_data)))
            }
            SensorEvent::Fft(fft_packet) => {
                let json_message = match parse_fft_binary_data(&fft_packet.to_binary()) {
                    Ok(fft_results) => json!({
                        "type": "FftPacket",
                        "timestamp": chrono::Utc::now().timestamp_millis(),
                        "data": fft_results
                    }).to_string(),
                    Err(e) => {
                        error!("Failed to parse FFT data for client: {}", e);
                        json!({
                            "type": "error",
                            "message": format!("Failed to parse FFT data: {}", e)
                        }).to_string()
                    }
                };
                Some(("FftPacket".to_string(), Message::text(json_message)))
            }
            _ => None, // Ignore RawEeg, System events, etc.
        }
    }
}

/// Serializes a `FilteredEegPacket` into a binary format for WebSocket transmission.
///
/// The binary format is designed to be easily parsed by the frontend `EegDataHandler`.
///
/// # Binary Format Layout:
///
/// | Part          | Type        | Size (bytes)                | Description                                     |
/// |---------------|-------------|-----------------------------|-------------------------------------------------|
/// | Total Samples | `u32`       | 4                           | Total number of sample points in the packet.    |
/// | Timestamps    | `u64[]`     | `total_samples * 8`         | Array of timestamps (little-endian).            |
/// | Sample Values | `f32[]`     | `total_samples * 4`         | Array of EEG voltage values (little-endian).    |
///
fn eeg_packet_to_binary(packet: &eeg_types::FilteredEegPacket) -> Vec<u8> {
    let total_samples = packet.samples.len();
    let timestamp_bytes = total_samples * 8;
    let sample_bytes = total_samples * 4;
    let total_capacity = 4 + timestamp_bytes + sample_bytes;

    let mut buffer = Vec::with_capacity(total_capacity);

    // 1. Write total number of samples (u32)
    buffer.extend_from_slice(&(total_samples as u32).to_le_bytes());

    // 2. Write all timestamps (u64)
    for &timestamp in packet.timestamps.iter() {
        buffer.extend_from_slice(&timestamp.to_le_bytes());
    }

    // 3. Write all sample values (f32)
    for &sample in packet.samples.iter() {
        buffer.extend_from_slice(&sample.to_le_bytes());
    }

    buffer
}


/// Parse binary FFT data into the JSON format expected by the kiosk.
/// This function is moved from `server.rs` to centralize logic here.
fn parse_fft_binary_data(binary_data: &[u8]) -> Result<Vec<serde_json::Value>, String> {
    if binary_data.len() < 16 {
        return Err("Binary data too short".to_string());
    }

    let mut offset = 0;

    // Timestamp (8 bytes) and source_frame_id (8 bytes) are part of the binary format
    // but not directly used in the final JSON, so we read and discard them.
    offset += 16;

    // Read number of channels (4 bytes)
    if offset + 4 > binary_data.len() {
        return Err("Not enough data for num_channels".to_string());
    }
    let num_channels = u32::from_le_bytes(
        binary_data[offset..offset + 4].try_into()
            .map_err(|_| "Failed to read num_channels")?
    ) as usize;
    offset += 4;

    let mut fft_results = Vec::new();

    // Read brain wave data for each channel
    for _ in 0..num_channels {
        // Each channel block has channel_idx (4) + 5 bands * 4 bytes/band = 24 bytes
        if offset + 24 > binary_data.len() {
            return Err("Not enough data for a full channel block".to_string());
        }

        // Skip channel index (4 bytes)
        offset += 4;

        // Read brain wave band powers (5 * 4 bytes each)
        let delta = f32::from_le_bytes(binary_data[offset..offset+4].try_into().unwrap());
        offset += 4;
        let theta = f32::from_le_bytes(binary_data[offset..offset+4].try_into().unwrap());
        offset += 4;
        let alpha = f32::from_le_bytes(binary_data[offset..offset+4].try_into().unwrap());
        offset += 4;
        let beta = f32::from_le_bytes(binary_data[offset..offset+4].try_into().unwrap());
        offset += 4;
        let gamma = f32::from_le_bytes(binary_data[offset..offset+4].try_into().unwrap());
        offset += 4;

        // The kiosk applet expects 'power' and 'frequencies' arrays.
        // We provide representative frequencies for each band.
        let frequencies = vec![2.0, 6.0, 10.5, 21.5, 65.0];
        let power = vec![delta, theta, alpha, beta, gamma];

        fft_results.push(json!({
            "power": power,
            "frequencies": frequencies
        }));
    }

    Ok(fft_results)
}