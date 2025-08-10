use axum::extract::ws::{Message, WebSocket};
use dashmap::DashMap;
use eeg_types::comms::{
    client::{ClientMessage, ServerMessage},
    pipeline::{BrokerMessage, BrokerPayload},
};
use futures::{stream::FuturesUnordered, FutureExt, SinkExt, Stream, StreamExt};
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


/// Manages WebSocket connections and routes data from the pipeline to subscribed clients.
type TopicRx = broadcast::Receiver<Arc<BrokerMessage>>;
type SubMap = HashMap<String, TopicRx>;

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

                let topic = &msg.topic;
                let sender = self
                    .topics
                    .entry(topic.clone())
                    .or_insert_with(|| {
                        let (s, _) = broadcast::channel(WEBSOCKET_BUFFER_SIZE);
                        s
                    })
                    .clone();

                if sender.send(msg.clone()).is_err() {
                    debug!(
                        "[Broker] No subscribers for topic '{}'. Removing sender.",
                        topic
                    );
                    self.topics.remove(topic);
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
        let mut subs: SubMap = HashMap::new();
        let mut topic_futs = FuturesUnordered::new();
        let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_S));

        loop {
            let client_event = select! {
                biased; // Prioritize client messages

                // Client message
                msg = ws_rx.next() => ClientEvent::FromClient(msg),

                // Topic message
                Some((topic, msg_res, sub_rx)) = topic_futs.next(), if !topic_futs.is_empty() => ClientEvent::FromTopic(Some((topic, msg_res, sub_rx))),

                // Ping timer
                _ = ping_interval.tick() => ClientEvent::Ping,

                else => break,
            };

            match client_event {
                ClientEvent::FromClient(msg) => {
                    match msg {
                        Some(Ok(Message::Text(txt))) => {
                            debug!("[Client {}] Received message: {}", client_id, txt);
                            if let Ok(msg) = serde_json::from_str::<ClientMessage>(&txt) {
                                match msg {
                                    ClientMessage::Subscribe { topic } => {
                                        if !subs.contains_key(&topic) {
                                            info!("[Client {}] Subscribing to topic: {}", client_id, topic);

                                            // Get or create the broadcast sender for the topic
                                            let sender = self.topics.entry(topic.clone()).or_insert_with(|| {
                                                let (s, _) = broadcast::channel(WEBSOCKET_BUFFER_SIZE);
                                                s
                                            }).clone();
                                            
                                            let mut sub_rx = sender.subscribe();

                                            // Send ACK *before* waiting for the first message
                                            let ack = ServerMessage::Subscribed(topic.clone());
                                            let ack_msg = Message::Text(serde_json::to_string(&ack).unwrap());
                                            if ws_tx.send(ack_msg).await.is_err() {
                                                info!("[Client {}] Failed to send subscription ACK, client disconnected.", client_id);
                                                break;
                                            }

                                            // Clone the receiver for the future, keeping the original in the map
                                            let mut fut_rx = sub_rx.resubscribe();
                                            subs.insert(topic.clone(), sub_rx);

                                            // Arm the future to listen for messages
                                            topic_futs.push(async move {
                                                let res = fut_rx.recv().await;
                                                (topic, res, fut_rx)
                                            }.boxed());
                                        }
                                    },
                                    ClientMessage::Unsubscribe { topic } => {
                                        info!("[Client {}] Unsubscribed from topic: {}", client_id, topic);
                                        subs.remove(&topic);
                                        // The future will resolve with a Closed error and won't be re-armed
                                    }
                                }
                            } else {
                                warn!("[Client {}] Received malformed control message. Closing.", client_id);
                               ws_tx.send(Message::Close(None)).await.ok();
                               break;
                            }
                        },
                        Some(Ok(Message::Close(_))) | None => {
                            info!("[Client {}] Connection closed.", client_id);
                            break;
                        },
                        Some(Err(e)) => {
                            warn!("[Client {}] WebSocket error: {}", client_id, e);
                            break;
                        },
                        Some(Ok(Message::Binary(_))) => {
                            warn!("[Client {}] Received unexpected binary message. Closing connection.", client_id);
                            ws_tx.send(Message::Close(None)).await.ok();
                            break;
                        }
                        _ => {
                            debug!("[Client {}] Ignoring unsupported message type.", client_id);
                        }
                    }
                }
                ClientEvent::FromTopic(Some((topic, msg_res, mut sub_rx))) => {
                    match msg_res {
                        Ok(msg) => {
                            let frame = match &msg.payload {
                                BrokerPayload::Meta(json) => Message::Text(json.clone()),
                                BrokerPayload::Data(data) => Message::Binary(data.clone()),
                            };
                            if ws_tx.send(frame).await.is_err() {
                                info!("[Client {}] Failed to forward message, client disconnected.", client_id);
                                break;
                            }
                            // Re-arm the future with the same receiver
                            topic_futs.push(async move {
                                let res = sub_rx.recv().await;
                                (topic, res, sub_rx)
                            }.boxed());
                        },
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                             warn!("[Client {}] Channel for topic '{}' lagged by {} messages.", client_id, &topic, n);
                             // Re-arm the future
                             topic_futs.push(async move {
                                let res = sub_rx.recv().await;
                                (topic, res, sub_rx)
                            }.boxed());
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            debug!("[Client {}] Channel for topic '{}' closed. Subscription removed.", client_id, topic);
                            // Do not re-insert or re-arm, receiver is dropped
                        }
                    }
                }
                ClientEvent::FromTopic(None) => {}, // Should not happen
                ClientEvent::Ping => {
                    if ws_tx.send(Message::Ping(vec![])).await.is_err() {
                        info!("[Client {}] Ping failed, client disconnected.", client_id);
                        break;
                    }
                }
            }
        }
        info!("[Client {}] Cleaning up connection.", client_id);
    }
}

#[derive(Debug)]
enum ClientEvent {
    FromClient(Option<Result<Message, axum::Error>>),
    FromTopic(Option<(String, Result<Arc<BrokerMessage>, broadcast::error::RecvError>, TopicRx)>),
    Ping,
}