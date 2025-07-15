//! WebSocket sink stage for broadcasting EEG data.

use crate::config::StageConfig;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Drains, Stage, StageContext};
use crate::data::Packet;
use serde::Deserialize;
use std::any::Any;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use tungstenite::{accept, Message};
use tracing::{info, warn};

/// A factory for creating `WebsocketSink` stages.
#[derive(Default)]
pub struct WebsocketSinkFactory;

impl StageFactory for WebsocketSinkFactory {
    fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage>, StageError> {
        let params: WebsocketSinkParams = serde_json::from_value(serde_json::Value::Object(
            config.params.clone().into_iter().collect(),
        ))?;
        let addr = params
            .addr
            .parse::<SocketAddr>()
            .map_err(|e| StageError::BadParam(format!("Invalid address: {}", e)))?;

        let clients = Arc::new(Mutex::new(Vec::new()));
        let sink = WebsocketSink {
            id: config.name.clone(),
            clients: clients.clone(),
        };

        thread::spawn(move || accept_loop(addr, clients));

        Ok(Box::new(sink))
    }
}

/// A sink stage that broadcasts incoming data to connected WebSocket clients.
pub struct WebsocketSink {
    id: String,
    clients: Arc<Mutex<Vec<mpsc::Sender<String>>>>,
}

#[derive(Debug, Deserialize)]
struct WebsocketSinkParams {
    #[serde(default = "default_addr")]
    addr: String,
}

fn default_addr() -> String {
    "127.0.0.1:9001".to_string()
}

fn accept_loop(addr: SocketAddr, clients: Arc<Mutex<Vec<mpsc::Sender<String>>>>) {
    let listener = TcpListener::bind(&addr).expect("Failed to bind");
    info!("WebSocket sink listening on: {}", addr);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let (tx, rx) = mpsc::channel();
                clients.lock().unwrap().push(tx);
                thread::spawn(move || handle_connection(stream, rx));
            }
            Err(e) => {
                warn!("Connection failed: {}", e);
            }
        }
    }
}

fn handle_connection(stream: TcpStream, rx: mpsc::Receiver<String>) {
    let mut websocket = match accept(stream) {
        Ok(ws) => ws,
        Err(e) => {
            warn!("WebSocket handshake failed: {}", e);
            return;
        }
    };

    for msg in rx {
        if websocket.send(Message::Text(msg)).is_err() {
            break;
        }
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
        packet: Box<dyn Any + Send>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Box<dyn Any + Send>>, StageError> {
        if let Some(packet) = packet.downcast_ref::<Packet<f32>>() {
            let json = serde_json::to_string(packet).unwrap();
            let mut clients = self.clients.lock().unwrap();
            // The `retain` method is used to keep only the clients that are still active.
            clients.retain(|tx| tx.send(json.clone()).is_ok());
        }
        Ok(None)
    }

    fn as_drains(&mut self) -> Option<&mut dyn Drains> {
        Some(self)
    }
}