use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{
        ws::{Message, WebSocket},
        ConnectInfo, State, WebSocketUpgrade,
    },
    response::Response,
};
use dashmap::DashMap;
use eeg_types::comms::BrokerMessage;
use eeg_types::data::PacketOwned;
use flume::Receiver;
use futures::{stream::StreamExt, SinkExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{info, warn};

/// Represents a connected WebSocket client.
struct Client {
    /// The client's address.
    addr: SocketAddr,
    /// The channel used to send packets to this client.
    sender: mpsc::UnboundedSender<Arc<PacketOwned>>,
}

/// The central state for the WebSocket broker.
#[derive(Default)]
struct BrokerState {
    /// Maps a topic string to a list of connected clients subscribed to that topic.
    subscriptions: DashMap<String, Vec<mpsc::UnboundedSender<Arc<PacketOwned>>>>,
}

/// The main broker struct that runs the central message-passing loop.
pub struct WebSocketBroker {
    /// The receiver for messages coming from all pipeline stages.
    packet_receiver: Receiver<BrokerMessage>,
    /// The shared state containing all client subscriptions.
    state: Arc<BrokerState>,
}

/// Defines the messages that clients (like the UI) can send to the broker.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ClientMessage {
    Subscribe(String),
    Unsubscribe(String),
}

impl WebSocketBroker {
    /// Creates a new WebSocketBroker.
    pub fn new(packet_receiver: Receiver<BrokerMessage>) -> Self {
        Self {
            packet_receiver,
            state: Arc::new(BrokerState::default()),
        }
    }

    /// Returns an Axum handler function for upgrading WebSocket connections.
    /// The main run loop for the broker.
    /// This task listens for incoming packets from pipelines and forwards them
    /// to all clients subscribed to the packet's topic.
    pub async fn run(self: Arc<Self>) {
        info!("WebSocket Broker started");
        while let Ok(msg) = self.packet_receiver.recv_async().await {
            if let Some(subscribers) = self.state.subscriptions.get(&msg.topic) {
                for sender in subscribers.iter() {
                    // If sending fails, the client has disconnected. We don't need to handle it
                    // here; the client's own read/write task will detect and clean up.
                    let _ = sender.send(Arc::new(msg.packet.clone()));
                }
            }
        }
        warn!("WebSocket Broker is shutting down.");
    }

    /// Manages a single client's WebSocket connection.
    pub(crate) async fn handle_connection(self: Arc<Self>, socket: WebSocket, addr: SocketAddr) {
        info!("New WebSocket connection received from {}", addr);
        let (mut socket_sender, mut socket_receiver) = socket.split();

        // Create an MPSC channel for this specific client.
        // The broker's main loop will use the sender half to push packets here.
        let (client_tx, client_rx) = mpsc::unbounded_channel::<Arc<PacketOwned>>();
        let mut client_rx = UnboundedReceiverStream::new(client_rx);

        // This task is responsible for taking packets from the MPSC channel
        // and serializing them into WebSocket messages.
        info!("Spawning write task for {}", addr);
        let write_task = tokio::spawn(async move {
            info!("Entering write loop for {}", addr);
            while let Some(packet) = client_rx.next().await {
                info!("Sending message to client {}", addr);
                match serde_json::to_vec(&*packet) {
                    Ok(bytes) => {
                        if socket_sender.send(Message::Binary(bytes)).await.is_err() {
                            // Connection closed
                            break;
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to serialize packet for client {}: {}. Packet: {:?}",
                            addr, e, packet
                        );
                    }
                }
            }
            info!("Exiting write loop for {}", addr);
        });

        // This task handles incoming messages from the client (e.g., subscribe/unsubscribe).
        let state = self.state.clone();
        info!("Spawning read task for {}", addr);
        let read_task = tokio::spawn(async move {
            let mut client_subscriptions: Vec<String> = Vec::new();

            info!("Entering read loop for {}", addr);
            while let Some(Ok(msg)) = socket_receiver.next().await {
                if let Message::Text(text) = msg {
                    info!("Received message from client {}: {}", addr, text);
                    match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(ClientMessage::Subscribe(topic)) => {
                            info!("Client {} subscribing to topic '{}'", addr, topic);
                            let mut subs = state.subscriptions.entry(topic.clone()).or_default();
                            subs.push(client_tx.clone());
                            client_subscriptions.push(topic);
                        }
                        Ok(ClientMessage::Unsubscribe(topic)) => {
                            info!("Client {} unsubscribing from topic '{}'", addr, topic);
                            if let Some(mut subs) = state.subscriptions.get_mut(&topic) {
                                subs.retain(|s| !s.same_channel(&client_tx));
                            }
                            client_subscriptions.retain(|t| t != &topic);
                        }
                        Err(e) => {
                            warn!("Failed to parse client message from {}: {}", addr, e);
                        }
                    }
                }
            }
            // When the client disconnects, clean up all of its subscriptions.
            info!("Cleaning up subscriptions for disconnected client {}", addr);
            for topic in client_subscriptions {
                if let Some(mut subs) = state.subscriptions.get_mut(&topic) {
                    subs.retain(|s| !s.same_channel(&client_tx));
                }
            }
            info!("Exiting read loop for {}", addr);
        });

        // Wait for either task to finish. If one does, the other should also be aborted.
        tokio::select! {
            _ = write_task => {},
            _ = read_task => {},
        }

        info!("Client disconnected: {}", addr);
    }
}