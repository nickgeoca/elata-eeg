//! Manages WebSocket connections for streaming data to UI clients.
use anyhow::Result;
use bytes::Bytes;
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;
use warp::ws::{Message, WebSocket};
use serde::{Deserialize, Serialize};

use eeg_types::event::{SensorEvent, WebSocketTopic, PROTOCOL_VERSION};
use eeg_types::plugin::EventBus;

type ClientId = String;
type ClientTx = SplitSink<WebSocket, Message>;

/// Subscription message types from WebSocket clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SubscriptionMessage {
    #[serde(rename = "subscribe")]
    Subscribe { topics: Vec<String> },
    #[serde(rename = "unsubscribe")]
    Unsubscribe { topics: Vec<String> },
}

/// Manages all WebSocket client connections and broadcasts events.
pub struct ConnectionManager {
    connection_rx: mpsc::Receiver<WebSocket>,
    event_rx: broadcast::Receiver<SensorEvent>,
    clients: Arc<Mutex<HashMap<ClientId, ClientTx>>>,
    /// Track which connections are subscribed to which topics
    connection_subscriptions: Arc<Mutex<HashMap<ClientId, HashSet<WebSocketTopic>>>>,
    /// Reference to EventBus for topic subscription tracking
    event_bus: Arc<dyn EventBus>,
}

// Helper struct for cloning parts of ConnectionManager needed in spawned tasks
#[derive(Clone)]
struct ConnectionManagerHandle {
    connection_subscriptions: Arc<Mutex<HashMap<ClientId, HashSet<WebSocketTopic>>>>,
    event_bus: Arc<dyn EventBus>,
}

impl ConnectionManagerHandle {
    /// Handle subscription message from a client
    async fn handle_subscription_message(&self, client_id: &str, message: SubscriptionMessage) {
        match message {
            SubscriptionMessage::Subscribe { topics } => {
                let mut subscriptions = self.connection_subscriptions.lock().await;
                let client_subs = subscriptions.entry(client_id.to_string()).or_insert_with(HashSet::new);
                
                for topic_str in topics {
                    if let Some(topic) = ConnectionManager::parse_topic(&topic_str) {
                        client_subs.insert(topic);
                        self.event_bus.add_topic_subscriber(topic, client_id.to_string());
                        debug!(client_id = %client_id, topic = ?topic, "Client subscribed to topic");
                    } else {
                        warn!(client_id = %client_id, topic = %topic_str, "Unknown topic in subscription");
                    }
                }
            }
            SubscriptionMessage::Unsubscribe { topics } => {
                let mut subscriptions = self.connection_subscriptions.lock().await;
                if let Some(client_subs) = subscriptions.get_mut(client_id) {
                    for topic_str in topics {
                        if let Some(topic) = ConnectionManager::parse_topic(&topic_str) {
                            client_subs.remove(&topic);
                            self.event_bus.remove_topic_subscriber(topic, client_id.to_string());
                            debug!(client_id = %client_id, topic = ?topic, "Client unsubscribed from topic");
                        }
                    }
                }
            }
        }
    }

    /// Clean up subscriptions when a client disconnects
    async fn handle_client_disconnect(&self, client_id: &str) {
        let mut subscriptions = self.connection_subscriptions.lock().await;
        if let Some(client_subs) = subscriptions.remove(client_id) {
            for topic in client_subs {
                self.event_bus.remove_topic_subscriber(topic, client_id.to_string());
                debug!(client_id = %client_id, topic = ?topic, "Cleaned up subscription on disconnect");
            }
        }
    }
}

impl ConnectionManager {
    pub fn new(
        connection_rx: mpsc::Receiver<WebSocket>,
        event_rx: broadcast::Receiver<SensorEvent>,
        event_bus: Arc<dyn EventBus>,
    ) -> Self {
        Self {
            connection_rx,
            event_rx,
            clients: Arc::new(Mutex::new(HashMap::new())),
            connection_subscriptions: Arc::new(Mutex::new(HashMap::new())),
            event_bus,
        }
    }

    /// Convert string topic name to WebSocketTopic enum
    fn parse_topic(topic_str: &str) -> Option<WebSocketTopic> {
        match topic_str {
            "FilteredEeg" => Some(WebSocketTopic::FilteredEeg),
            "Fft" => Some(WebSocketTopic::Fft),
            "Log" => Some(WebSocketTopic::Log),
            _ => None,
        }
    }

    /// Create a handle for use in spawned tasks
    fn create_handle(&self) -> ConnectionManagerHandle {
        ConnectionManagerHandle {
            connection_subscriptions: self.connection_subscriptions.clone(),
            event_bus: self.event_bus.clone(),
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
                    if let SensorEvent::WebSocketBroadcast { topic, payload } = event {
                        self.dispatch_binary_event(topic as u8, payload).await;
                    }
                }
            }
        }
        info!("ConnectionManager has shut down.");
    }

    /// Adds a new client and spawns a task to handle its lifecycle.
    fn add_client(&self, websocket: WebSocket) {
        let client_id = Uuid::new_v4().to_string();
        let (sender, mut receiver) = websocket.split();
        let clients_arc = self.clients.clone();
        let handle = self.create_handle();

        info!(client_id = %client_id, "New WebSocket client connected.");

        // Spawn a task to add the client and then monitor its connection.
        tokio::spawn(async move {
            clients_arc.lock().await.insert(client_id.clone(), sender);

            // Listen for messages from the client
            while let Some(result) = receiver.next().await {
                match result {
                    Ok(message) => {
                        if message.is_close() {
                            debug!(client_id = %client_id, "Client sent close frame.");
                            break;
                        } else if message.is_text() {
                            // Handle subscription messages
                            if let Ok(text) = message.to_str() {
                                match serde_json::from_str::<SubscriptionMessage>(text) {
                                    Ok(sub_msg) => {
                                        handle.handle_subscription_message(&client_id, sub_msg).await;
                                    }
                                    Err(e) => {
                                        debug!(client_id = %client_id, "Failed to parse subscription message: {}", e);
                                    }
                                }
                            }
                        }
                        // Ignore binary messages for now
                    }
                    Err(e) => {
                        warn!(client_id = %client_id, "Error on client WebSocket: {}", e);
                        break;
                    }
                }
            }
            
            info!(client_id = %client_id, "Client disconnected. Removing.");
            clients_arc.lock().await.remove(&client_id);
            handle.handle_client_disconnect(&client_id).await;
        });
    }

    /// Dispatches a binary event to all connected clients.
    async fn dispatch_binary_event(&self, topic: u8, payload: Bytes) {
        let mut clients_guard = self.clients.lock().await;
        if clients_guard.is_empty() {
            return;
        }

        // Prepare the message once
        let header = [PROTOCOL_VERSION, topic];
        let mut combined = header.to_vec();
        combined.extend_from_slice(&payload);
        let message = Message::binary(Bytes::from(combined));

        let mut disconnected_clients = Vec::new();

        for (id, client_tx) in clients_guard.iter_mut() {
            if let Err(e) = client_tx.send(message.clone()).await {
                warn!(client_id = %id, "Failed to send message, client disconnected: {}", e);
                disconnected_clients.push(id.clone());
            }
        }

        // Clean up disconnected clients
        for id in disconnected_clients {
            clients_guard.remove(&id);
            info!(client_id = %id, "Removed disconnected client.");
        }
    }
}

