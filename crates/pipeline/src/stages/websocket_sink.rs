//! WebSocket sink stage for broadcasting EEG data.

use crate::config::StageConfig;
use crate::data::{PacketOwned, PacketView, RtPacket};
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Drains, Stage, StageContext, StageInitCtx};
use flume::{unbounded, Receiver, Sender};
use serde::Deserialize;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use tungstenite::{accept, Message};
use tracing::{info, warn};

/// A factory for creating `WebsocketSink` stages.
#[derive(Default)]
pub struct WebsocketSinkFactory;

impl StageFactory for WebsocketSinkFactory {
    fn create(
        &self,
        config: &StageConfig,
        _: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        let params: WebsocketSinkParams = serde_json::from_value(serde_json::Value::Object(
            config.params.clone().into_iter().collect(),
        ))?;
        Ok((
            Box::new(WebsocketSink::new(config.name.clone(), params)?),
            None,
        ))
    }
}

/// A sink stage that broadcasts incoming data to connected WebSocket clients.
pub struct WebsocketSink {
    id: String,
    clients: Arc<Mutex<Vec<Sender<String>>>>,
}

impl WebsocketSink {
    pub fn new(id: String, params: WebsocketSinkParams) -> Result<Self, StageError> {
        let addr = params
            .addr
            .parse::<SocketAddr>()
            .map_err(|e| StageError::BadParam(format!("Invalid address: {}", e)))?;

        let clients = Arc::new(Mutex::new(Vec::new()));
        let sink = Self {
            id,
            clients: clients.clone(),
        };

        thread::spawn(move || accept_loop(addr, clients));

        Ok(sink)
    }
}

#[derive(Debug, Deserialize)]
pub struct WebsocketSinkParams {
    #[serde(default = "default_addr")]
    pub addr: String,
}

fn default_addr() -> String {
    "127.0.0.1:9001".to_string()
}

fn accept_loop(addr: SocketAddr, clients: Arc<Mutex<Vec<Sender<String>>>>) {
    let listener = match TcpListener::bind(&addr) {
        Ok(listener) => listener,
        Err(e) => {
            warn!(
                "Failed to bind WebSocket sink to address {}: {}. The sink will be disabled.",
                addr, e
            );
            return;
        }
    };
    info!("WebSocket sink listening on: {}", addr);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let (tx, rx) = unbounded();
                clients.lock().unwrap().push(tx);
                thread::spawn(move || handle_connection(stream, rx));
            }
            Err(e) => {
                warn!("Connection failed: {}", e);
            }
        }
    }
}

fn handle_connection(stream: TcpStream, rx: Receiver<String>) {
    let mut websocket = match accept(stream) {
        Ok(ws) => ws,
        Err(e) => {
            warn!("WebSocket handshake failed: {}", e);
            return;
        }
    };

    // Set the stream to non-blocking mode. This is crucial.
    if let Err(e) = websocket.get_mut().set_nonblocking(true) {
        warn!("Failed to set non-blocking mode: {}", e);
        return;
    }

    loop {
        // Try to read a message from the client in a non-blocking way
        match websocket.read() {
            Ok(msg) => {
                if msg.is_close() {
                    break;
                }
                // Handle other messages if necessary (e.g., subscriptions)
            }
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // This is not an error, just means no message from the client right now.
            }
            Err(tungstenite::Error::ConnectionClosed) => {
                break; // Cleanly exit if the client closes the connection
            }
            Err(e) => {
                warn!("Error reading from WebSocket: {}", e);
                break; // Exit on other errors
            }
        }

        // Check for outgoing messages from the pipeline without blocking
        match rx.try_recv() {
            Ok(msg) => {
                if websocket.send(Message::Text(msg)).is_err() {
                    break; // Exit if we can't send to the client
                }
            }
            Err(flume::TryRecvError::Empty) => {
                // No message from the pipeline, continue to the sleep.
            }
            Err(flume::TryRecvError::Disconnected) => {
                break; // The pipeline has shut down
            }
        }

        // Sleep to prevent the loop from spinning at 100% CPU.
        thread::sleep(std::time::Duration::from_millis(1));
    }
}

impl Drains for WebsocketSink {
    fn flush(&mut self) -> std::io::Result<()> {
        // Data is sent immediately, so there's nothing to flush.
        Ok(())
    }
}

impl Stage for WebsocketSink {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Arc<RtPacket>>, StageError> {
        // Use PacketView to inspect the packet type without a deep copy.
        if let PacketView::Voltage { .. } = PacketView::from(&*packet) {
            // To serialize, we need an owned packet.
            // Try to consume the Arc. If we can't (i.e., it's part of a fan-out),
            // then perform an explicit deep clone.
            let owned_packet = match Arc::try_unwrap(packet) {
                Ok(rt_packet) => PacketOwned::from(rt_packet),
                Err(arc) => PacketOwned::from(arc.deep_clone()),
            };

            let json = serde_json::to_string(&owned_packet).unwrap();
            let mut clients = self.clients.lock().unwrap();
            // The `retain` method is used to keep only the clients that are still active.
            clients.retain(|tx| tx.send(json.clone()).is_ok());
        }
        // This is a sink, so we consume the packet and don't forward it.
        Ok(None)
    }

    fn as_drains(&mut self) -> Option<&mut dyn Drains> {
        Some(self)
    }
}