//! WebSocket sink stage for broadcasting EEG data.

use crate::config::StageConfig;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext};
use async_trait::async_trait;
use eeg_types::Packet;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{info, warn};

/// A factory for creating `WebsocketSink` stages.
#[derive(Default)]
pub struct WebsocketSinkFactory;

#[async_trait]
impl StageFactory<f32, f32> for WebsocketSinkFactory {
    async fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage<f32, f32>>, StageError> {
        let params: WebsocketSinkParams = serde_json::from_value(serde_json::Value::Object(
            config
                .params
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ))?;
        let addr = params
            .addr
            .parse::<SocketAddr>()
            .map_err(|e| StageError::BadParam(format!("Invalid address: {}", e)))?;

        let (tx, _) = broadcast::channel(100);
        let sink = WebsocketSink {
            id: config.name.clone(),
            tx: Some(tx.clone()),
        };

        tokio::spawn(accept_loop(addr, tx));

        Ok(Box::new(sink))
    }
}

/// A sink stage that broadcasts incoming data to connected WebSocket clients.
pub struct WebsocketSink {
    id: String,
    tx: Option<broadcast::Sender<String>>,
}

#[derive(Debug, Deserialize)]
struct WebsocketSinkParams {
    #[serde(default = "default_addr")]
    addr: String,
}

fn default_addr() -> String {
    "127.0.0.1:9001".to_string()
}

async fn accept_loop(addr: SocketAddr, tx: broadcast::Sender<String>) {
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");
    info!("WebSocket sink listening on: {}", addr);
    while let Ok((stream, _)) = listener.accept().await {
        let rx = tx.subscribe();
        tokio::spawn(handle_connection(stream, rx));
    }
}

async fn handle_connection(stream: TcpStream, mut rx: broadcast::Receiver<String>) {
    let mut ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            warn!("WebSocket handshake failed: {}", e);
            return;
        }
    };

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(msg) => {
                        if ws_stream.send(Message::Text(msg)).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            _ = ws_stream.next() => {
                break;
            }
        }
    }
}

#[async_trait]
impl Stage<f32, f32> for WebsocketSink {
    fn id(&self) -> &str {
        &self.id
    }

    async fn process(
        &mut self,
        packet: Packet<f32>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Packet<f32>>, StageError> {
        if let Some(tx) = &self.tx {
            let json = serde_json::to_string(&packet).unwrap();
            let _ = tx.send(json);
        }
        Ok(None)
    }
}