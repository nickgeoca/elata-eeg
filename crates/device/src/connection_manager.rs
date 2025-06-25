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
    pending_clients: Arc<Mutex<HashMap<ClientId, SplitSink<WebSocket, Message>>>>,
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
            pending_clients: Arc::new(Mutex::new(HashMap::new())),
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
        let pending_clients_arc = self.pending_clients.clone();

        info!(client_id = %client_id, "New WebSocket client connected, holding in pending.");

        // Spawn a task to manage this specific client's lifecycle
        let client_id_clone = client_id.clone();
        tokio::spawn(async move {
            // Initially, the client is pending and has no subscriptions.
            // We only store the sender part until the first subscription is received.
            pending_clients_arc.lock().await.insert(client_id_clone.clone(), sender);

            let mut is_activated = false;

            loop {
                match receiver.next().await {
                    Some(Ok(msg)) => {
                        if msg.is_close() {
                            debug!(client_id = %client_id_clone, "Client sent close frame.");
                            break;
                        }
                        if let Ok(text) = msg.to_str() {
                            if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(text) {
                                if !is_activated {
                                    // First valid message must be a subscription
                                    if client_msg.action == "subscribe" {
                                        info!(client_id = %client_id_clone, "Received first subscription. Activating client.");
                                        if let Some(mut sender) = pending_clients_arc.lock().await.remove(&client_id_clone) {
                                            // Send confirmation message
                                            let confirmation = json!({
                                                "type": "status",
                                                "status": "subscription_ok"
                                            }).to_string();
                                            
                                            if sender.send(Message::text(confirmation)).await.is_ok() {
                                                let mut clients_guard = clients_arc.lock().await;
                                                let new_client = Client {
                                                    sender,
                                                    subscriptions: client_msg.topics.into_iter().collect(),
                                                };
                                                clients_guard.insert(client_id_clone.clone(), new_client);
                                                is_activated = true;
                                            } else {
                                                warn!(client_id = %client_id_clone, "Failed to send subscription confirmation. Closing connection.");
                                                break; // Break the loop to disconnect the client
                                            }
                                        }
                                    } else {
                                        warn!(client_id = %client_id_clone, "First message was not 'subscribe'. Closing connection.");
                                        // Optionally send an error message before closing
                                        break;
                                    }
                                } else {
                                    // Client is already active, handle other messages
                                    let mut clients_guard = clients_arc.lock().await;
                                    if let Some(client) = clients_guard.get_mut(&client_id_clone) {
                                        match client_msg.action.as_str() {
                                            "subscribe" => {
                                                info!(client_id = %client_id_clone, "Subscribing to additional topics: {:?}", client_msg.topics);
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
            // Cleanup: Remove the client from both maps upon disconnection
            info!(client_id = %client_id_clone, "Client disconnected. Removing.");
            pending_clients_arc.lock().await.remove(&client_id_clone);
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
                let json_message = json!({
                    "type": "FftPacket",
                    "data": *fft_packet
                });
                Some(("FftPacket".to_string(), Message::text(json_message.to_string())))
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

