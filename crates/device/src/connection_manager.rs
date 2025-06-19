//! Manages WebSocket connections for streaming data to UI clients.
use anyhow::Result;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, error, debug};
use uuid::Uuid;
use warp::ws::{Message, WebSocket};
use eeg_types::SensorEvent;
use serde_json::json;

type ClientId = String;

/// Represents a connected WebSocket client.
struct Client {
    /// The sending half of the WebSocket connection.
    sender: SplitSink<WebSocket, Message>,
}

/// Manages all WebSocket client connections.
///
/// This struct is responsible for:
/// - Accepting new WebSocket connections.
/// - Subscribing to the main `EventBus`.
/// - Forwarding relevant events to the appropriate clients.
/// - Handling client disconnections gracefully.
pub struct ConnectionManager {
    /// Receives new WebSocket connections from the server routes.
    connection_rx: mpsc::Receiver<WebSocket>,
    /// Receives sensor events from the main event bus.
    event_rx: mpsc::Receiver<SensorEvent>,
    /// Stores all active client connections.
    clients: HashMap<ClientId, Client>,
}

impl ConnectionManager {
    /// Creates a new `ConnectionManager`.
    pub fn new(
        connection_rx: mpsc::Receiver<WebSocket>,
        event_rx: mpsc::Receiver<SensorEvent>,
    ) -> Self {
        Self {
            connection_rx,
            event_rx,
            clients: HashMap::new(),
        }
    }

    /// The main run loop for the manager.
    ///
    /// This should be spawned as a long-running, supervised task.
    pub async fn run(&mut self, shutdown_token: CancellationToken) {
        info!("ConnectionManager is running.");
        loop {
            tokio::select! {
                biased; // Prioritize shutdown
                _ = shutdown_token.cancelled() => {
                    info!("ConnectionManager received shutdown signal.");
                    break;
                }

                // Handle new incoming WebSocket connections
                Some(websocket) = self.connection_rx.recv() => {
                    self.add_client(websocket);
                }

                // Handle new events from the event bus
                Some(event) = self.event_rx.recv() => {
                    if let SensorEvent::Fft(fft_packet) = event {
                        // For now, we broadcast to all clients.
                        // A future improvement would be topic-based subscriptions.
                        let json_message = match parse_fft_binary_data(&fft_packet.to_binary()) {
                            Ok(fft_results) => {
                                let response = json!({
                                    "timestamp": chrono::Utc::now().timestamp_millis(),
                                    "fft_results": fft_results
                                });
                                response.to_string()
                            }
                            Err(e) => {
                                error!("Failed to parse FFT data for client: {}", e);
                                let error_response = json!({
                                    "timestamp": chrono::Utc::now().timestamp_millis(),
                                    "error": format!("Failed to parse FFT data: {}", e)
                                });
                                error_response.to_string()
                            }
                        };

                        // Broadcast the JSON message to all connected clients
                        self.broadcast_message(Message::text(json_message)).await;
                    }
                }
            }
        }
        info!("ConnectionManager has shut down.");
    }

    /// Adds a new client to the manager.
    fn add_client(&mut self, websocket: WebSocket) {
        let client_id = Uuid::new_v4().to_string();
        let (sender, mut receiver) = websocket.split();

        info!(client_id = %client_id, "New WebSocket client connected.");

        let client = Client { sender };
        self.clients.insert(client_id.clone(), client);

        // Spawn a task to handle incoming messages from this specific client
        // (mostly for handling disconnection).
        let client_id_clone = client_id.clone();
        tokio::spawn(async move {
            while let Some(result) = receiver.next().await {
                match result {
                    Ok(msg) => {
                        if msg.is_close() {
                            debug!(client_id = %client_id_clone, "Client sent close frame.");
                            break;
                        }
                        // We don't expect any other messages from the client for now.
                    }
                    Err(e) => {
                        warn!(client_id = %client_id_clone, "Error receiving from client: {}", e);
                        break;
                    }
                }
            }
            // When the loop breaks, the client has disconnected.
            // The ConnectionManager will handle the cleanup when it tries to send.
        });
    }

    /// Sends a message to all connected clients.
    async fn broadcast_message(&mut self, message: Message) {
        if self.clients.is_empty() {
            return;
        }

        let mut disconnected_clients = Vec::new();

        for (id, client) in self.clients.iter_mut() {
            if let Err(e) = client.sender.send(message.clone()).await {
                warn!(client_id = %id, "Failed to send message, client disconnected: {}", e);
                disconnected_clients.push(id.clone());
            }
        }

        // Clean up disconnected clients
        for id in disconnected_clients {
            self.clients.remove(&id);
            info!(client_id = %id, "Removed disconnected client.");
        }
    }
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