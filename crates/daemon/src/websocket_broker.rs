use axum::{
    extract::ws::{Message, WebSocket},
    Error,
};
use dashmap::DashMap;
use eeg_types::comms::{BrokerMessage, BrokerPayload};
use futures::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

const WEBSOCKET_BUFFER_SIZE: usize = 100;

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
    subscriptions: DashMap<String, broadcast::Sender<Arc<BrokerMessage>>>,
}

impl WebSocketBroker {
    pub fn new(pipeline_rx: broadcast::Receiver<Arc<BrokerMessage>>) -> Self {
        Self {
            pipeline_rx: Mutex::new(pipeline_rx),
            subscriptions: DashMap::new(),
        }
    }

    /// A long-running task that listens for incoming messages and broadcasts them to clients.
    pub async fn run(&self) {
        loop {
            let msg = match self.pipeline_rx.lock().await.recv().await {
                Ok(msg) => msg,
                Err(e) => {
                    eprintln!("Error receiving message from pipeline: {}", e);
                    // The sender has been dropped, so we can't receive any more messages.
                    if e == broadcast::error::RecvError::Closed {
                        break;
                    }
                    continue;
                }
            };

            if let Some(topic_sender) = self.subscriptions.get(&msg.topic) {
                // Topic exists, broadcast the message
                if let Err(e) = topic_sender.send(msg.clone()) {
                    eprintln!("Error broadcasting message: {}", e);
                }
            }
        }
    }

    /// Adds a new client to the broker, subscribing them to a specific topic.
    pub async fn add_client(self: Arc<Self>, ws: WebSocket) {
        let (ws_tx, mut ws_rx) = ws.split();
        let ws_tx = Arc::new(Mutex::new(ws_tx));
        let client_subscriptions = Arc::new(Mutex::new(HashSet::new()));

        // Task to handle messages from the client (e.g., subscription changes)
        let broker = self.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = ws_rx.next().await {
                if let Message::Text(text) = msg {
                    match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(ClientMessage::Subscribe(topic)) => {
                            let mut subs = client_subscriptions.lock().await;
                            if subs.insert(topic.clone()) {
                                let topic_sender = broker
                                    .subscriptions
                                    .entry(topic.clone())
                                    .or_insert_with(|| {
                                        let (sender, _) =
                                            broadcast::channel(WEBSOCKET_BUFFER_SIZE);
                                        sender
                                    })
                                    .value()
                                    .clone();
                                let mut topic_rx = topic_sender.subscribe();
                                let ws_tx_clone = ws_tx.clone();

                                // Task to forward messages from the broker to the client
                                tokio::spawn(async move {
                                    while let Ok(msg) = topic_rx.recv().await {
                                        let ws_message = match &msg.payload {
                                            BrokerPayload::Meta(json_str) => {
                                                Message::Text(json_str.clone())
                                            }
                                            BrokerPayload::Data(bin_vec) => {
                                                Message::Binary(bin_vec.clone())
                                            }
                                        };

                                        if ws_tx_clone.lock().await.send(ws_message).await.is_err()
                                        {
                                            // Client disconnected
                                            break;
                                        }
                                    }
                                });
                            }
                        }
                        Ok(ClientMessage::Unsubscribe(topic)) => {
                            let mut subs = client_subscriptions.lock().await;
                            subs.remove(&topic);
                        }
                        Err(e) => {
                            eprintln!("Failed to parse client message: {}", e);
                        }
                    }
                } else if let Message::Close(_) = msg {
                    break;
                }
            }
        });
    }
}