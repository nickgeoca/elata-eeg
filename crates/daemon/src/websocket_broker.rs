use axum::extract::ws::{Message, WebSocket};
use dashmap::DashMap;
use eeg_types::comms::{BrokerMessage, BrokerPayload};
use futures::{stream::FuturesUnordered, SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::{
    select,
    sync::{broadcast, Mutex},
    time::interval,
};
use tracing::{debug, info, warn};

const WEBSOCKET_BUFFER_SIZE: usize = 1024;
const PING_INTERVAL_S: u64 = 20;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
enum ClientMessage {
    Subscribe(String),
    Unsubscribe(String),
}

/// Manages WebSocket connections and routes data from the pipeline to subscribed clients.
pub struct WebSocketBroker {
    /// Receives messages from the pipeline.
    pipeline_rx: Mutex<broadcast::Receiver<Arc<BrokerMessage>>>,
    /// Manages client subscriptions to different topics.
    topics: DashMap<String, broadcast::Sender<Arc<BrokerMessage>>>,
}

impl WebSocketBroker {
    pub fn new(pipeline_rx: broadcast::Receiver<Arc<BrokerMessage>>) -> Self {
        Self {
            pipeline_rx: Mutex::new(pipeline_rx),
            topics: DashMap::new(),
        }
    }

    /// A long-running task that listens for incoming messages and broadcasts them to clients.
    pub fn start(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                let msg = match self.pipeline_rx.lock().await.recv().await {
                    Ok(msg) => msg,
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("[Broker] Main pipeline channel closed. Shutting down message broadcast task.");
                        break;
                    }
                    Err(e) => {
                        warn!("[Broker] Error receiving message from pipeline: {}. Ignoring.", e);
                        continue;
                    }
                };

                if let Some(topic_sender) = self.topics.get(&msg.topic) {
                    if topic_sender.send(msg.clone()).is_err() {
                        debug!("[Broker] No subscribers for topic '{}'. Removing sender.", msg.topic);
                        self.topics.remove(&msg.topic);
                    }
                }
            }
        });
    }

    /// Adds a new client to the broker, creating a dedicated task to manage its lifecycle.
    pub async fn add_client(self: Arc<Self>, ws: WebSocket) {
        let client_id = uuid::Uuid::new_v4().to_string();
        info!("[Client {}] New WebSocket connection established.", client_id);
        self.handle_client(ws, client_id).await;
    }

    /// Manages the entire lifecycle of a single client connection.
    async fn handle_client(self: Arc<Self>, ws: WebSocket, client_id: String) {
        let (mut ws_tx, mut ws_rx) = ws.split();
        let (local_tx, mut local_rx) = tokio::sync::mpsc::channel::<Message>(WEBSOCKET_BUFFER_SIZE);
        let mut subs: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_S));

        loop {
            select! {
                // --- 1. Send a periodic ping to keep the connection alive ---
                _ = ping_interval.tick() => {
                    if ws_tx.send(Message::Ping(vec![])).await.is_err() {
                        info!("[Client {}] Failed to send ping, client likely disconnected.", client_id);
                        break;
                    }
                }

                // --- 2. Forward messages from the unified local channel to the client ---
                Some(frame) = local_rx.recv() => {
                    if ws_tx.send(frame).await.is_err() {
                        info!("[Client {}] Failed to forward message, client disconnected.", client_id);
                        break;
                    }
                }

                // --- 3. Handle control messages from the client ---
                maybe_msg = ws_rx.next() => {
                    match maybe_msg {
                        Some(Ok(Message::Text(txt))) => {
                            match serde_json::from_str::<ClientMessage>(&txt) {
                                Ok(ClientMessage::Subscribe(topic)) => {
                                    if !subs.contains_key(&topic) {
                                        info!("[Client {}] Subscribed to topic: {}", client_id, topic);
                                        let sender = self.topics
                                            .entry(topic.clone())
                                            .or_insert_with(|| {
                                                debug!("[Broker] Creating new topic sender for: {}", topic);
                                                let (s,_) = broadcast::channel(WEBSOCKET_BUFFER_SIZE);
                                                s
                                            })
                                            .clone();

                                        let mut rx = sender.subscribe();
                                        let local_tx_clone = local_tx.clone();

                                        let task = tokio::spawn(async move {
                                            loop {
                                                match rx.recv().await {
                                                    Ok(msg) => {
                                                        let frame = match &msg.payload {
                                                            BrokerPayload::Meta(json) => Message::Text(json.clone()),
                                                            BrokerPayload::Data(bin)  => Message::Binary(bin.clone()),
                                                        };
                                                        if local_tx_clone.send(frame).await.is_err() {
                                                            // Main client loop has probably terminated
                                                            break;
                                                        }
                                                    }
                                                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                                        // Lagged errors are expected under back-pressure conditions
                                                        // We log them for monitoring but continue processing
                                                        tracing::warn!("WebSocket receiver lagged by {} messages", n);
                                                        continue;
                                                    }
                                                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                                        // Channel closed, terminate the task
                                                        break;
                                                    }
                                                }
                                            }
                                        });
                                        subs.insert(topic, task);
                                    }
                                }
                                Ok(ClientMessage::Unsubscribe(topic)) => {
                                    if let Some(task) = subs.remove(&topic) {
                                        info!("[Client {}] Unsubscribed from topic: {}", client_id, topic);
                                        task.abort();
                                    }
                                }
                                Err(e) => warn!("[Client {}] Received malformed control message: {}", client_id, e),
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("[Client {}] Received close frame.", client_id);
                            break;
                        }
                        None => {
                            info!("[Client {}] WebSocket stream ended (None).", client_id);
                            break;
                        }
                        Some(Err(e)) => {
                            warn!("[Client {}] WebSocket error: {}", client_id, e);
                            break;
                        }
                        _ => {} // Ignore other message types like Pong, etc.
                    }
                }
            }
        }
        info!("[Client {}] Cleaning up connection, aborting subscription tasks.", client_id);
        for task in subs.values() {
            task.abort();
        }
    }
}