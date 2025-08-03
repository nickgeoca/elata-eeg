use axum::{
    extract::ws::{Message, WebSocket},
    Error,
};
use dashmap::DashMap;
use eeg_types::comms::BrokerMessage;
use futures::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

const WEBSOCKET_BUFFER_SIZE: usize = 100;

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
    pub async fn add_client(&self, ws: WebSocket, initial_topic: String) {
        let topic_sender = self
            .subscriptions
            .entry(initial_topic.clone())
            .or_insert_with(|| {
                let (sender, _) = broadcast::channel(WEBSOCKET_BUFFER_SIZE);
                sender
            })
            .value()
            .clone();

        let mut topic_rx = topic_sender.subscribe();
        let (mut ws_tx, mut ws_rx) = ws.split();

        // Task to forward messages from the broker to the client
        tokio::spawn(async move {
            while let Ok(msg) = topic_rx.recv().await {
                let json_msg = serde_json::to_string(&*msg).unwrap();
                if ws_tx.send(Message::Text(json_msg)).await.is_err() {
                    // Client disconnected
                    break;
                }
            }
        });

        // Task to handle messages from the client (e.g., subscription changes)
        // This part is not fully implemented as per instructions, but the task is spawned.
        tokio::spawn(async move {
            while let Some(Ok(msg)) = ws_rx.next().await {
                // Handle incoming messages from client, e.g., to change subscriptions
                if let Message::Close(_) = msg {
                    break;
                }
            }
        });
    }
}