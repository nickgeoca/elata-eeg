//! WebSocket sink stage for broadcasting EEG data
//! 
//! This stage receives voltage EEG packets and broadcasts them over WebSocket connections.
//! It implements the unified DataPlaneStage pattern for type-safe packet processing.

use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};
use tracing::{debug, info, trace};

use crate::data::{Packet, VoltageEegPacket};
use crate::error::{PipelineResult, StageError};
use crate::stage::{
    DataPlaneStage, DataPlaneStageFactory, DataPlaneStageErased, ErasedDataPlaneStageFactory,
    ErasedStageContext, StageContext, StageParams, StaticStageRegistrar, Input
};
use crate::ctrl_loop;

/// Configuration for WebSocket sink stage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketSinkConfig {
    /// WebSocket server bind address
    pub bind_address: String,
    /// WebSocket server port
    pub port: u16,
    /// Data format for broadcasting (json, binary)
    pub format: String,
    /// Maximum number of concurrent connections
    pub max_connections: usize,
    /// Buffer size for broadcast channel
    pub buffer_size: usize,
}

impl Default for WebSocketSinkConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".to_string(),
            port: 8080,
            format: "json".to_string(),
            max_connections: 100,
            buffer_size: 1000,
        }
    }
}

/// WebSocket sink stage implementation
pub struct WebSocketSinkStage {
    config: Arc<Mutex<WebSocketSinkConfig>>,
    packets_processed: AtomicU64,
    bytes_sent: AtomicU64,
    connections_count: Arc<AtomicU64>,
    is_running: AtomicBool,
    enabled: AtomicBool,
    broadcast_tx: Arc<Mutex<Option<broadcast::Sender<Vec<u8>>>>>,
    // Cached handles to avoid HashMap lookups in the hot path
    input_rx: Option<Box<dyn Input<VoltageEegPacket>>>,
}

impl WebSocketSinkStage {
    pub fn new(config: WebSocketSinkConfig) -> Self {
        Self {
            config: Arc::new(Mutex::new(config)),
            packets_processed: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            connections_count: Arc::new(AtomicU64::new(0)),
            is_running: AtomicBool::new(false),
            enabled: AtomicBool::new(true),
            broadcast_tx: Arc::new(Mutex::new(None)),
            input_rx: None,
        }
    }

    /// Start the WebSocket server
    async fn start_server(&self) -> Result<(), StageError> {
        let config = self.config.lock().unwrap().clone();
        let bind_addr = format!("{}:{}", config.bind_address, config.port);
        
        let listener = TcpListener::bind(&bind_addr).await
            .map_err(|e| StageError::Io(format!("Failed to bind WebSocket server to {}: {}", bind_addr, e)))?;
        
        let (tx, _) = broadcast::channel(config.buffer_size);
        *self.broadcast_tx.lock().unwrap() = Some(tx.clone());
        
        info!("WebSocket server listening on {}", bind_addr);
        
        let connections_count = Arc::clone(&self.connections_count);
        let max_connections = config.max_connections;
        
        tokio::spawn(async move {
            while let Ok((stream, addr)) = listener.accept().await {
                let current_connections = connections_count.load(Ordering::Relaxed);
                if current_connections >= max_connections as u64 {
                    debug!("Rejecting connection from {} - max connections reached", addr);
                    continue;
                }
                
                let rx = tx.subscribe();
                let connections_count_clone = Arc::clone(&connections_count);
                
                tokio::spawn(async move {
                    connections_count_clone.fetch_add(1, Ordering::Relaxed);
                    debug!("New WebSocket connection from {}", addr);
                    
                    if let Err(e) = handle_websocket_connection(stream, rx).await {
                        debug!("WebSocket connection error for {}: {}", addr, e);
                    }
                    
                    connections_count_clone.fetch_sub(1, Ordering::Relaxed);
                    debug!("WebSocket connection closed for {}", addr);
                });
            }
        });
        
        Ok(())
    }

    /// Convert voltage packet to broadcast data
    fn packet_to_broadcast_data(&self, packet: &Packet<VoltageEegPacket>) -> Vec<u8> {
        let config = self.config.lock().unwrap();
        
        match config.format.as_str() {
            "json" => {
                let json_data = serde_json::json!({
                    "timestamp": packet.header.timestamp,
                    "batch_size": packet.header.batch_size,
                    "samples": packet.samples.samples,
                    "format": "voltage"
                });
                serde_json::to_vec(&json_data).unwrap_or_default()
            }
            "binary" => {
                // Simple binary format: timestamp (8 bytes) + batch_size (8 bytes) + samples
                let mut data = Vec::new();
                data.extend_from_slice(&packet.header.timestamp.to_le_bytes());
                data.extend_from_slice(&(packet.header.batch_size as u64).to_le_bytes());
                data.extend_from_slice(&(packet.samples.samples.len() as u32).to_le_bytes());
                for sample in &packet.samples.samples {
                    data.extend_from_slice(&sample.to_le_bytes());
                }
                data
            }
            _ => Vec::new(),
        }
    }

    /// Gets mutable references to the input handle, initializing it on the first call
    #[cold]
    #[inline(always)]
    fn lazy_io<'a>(
        input_rx: &'a mut Option<Box<dyn Input<VoltageEegPacket>>>,
        ctx: &'a mut StageContext<VoltageEegPacket, VoltageEegPacket>,
    ) -> Result<&'a mut Box<dyn Input<VoltageEegPacket>>, StageError> {
        if input_rx.is_none() {
            *input_rx = Some(ctx.inputs.remove("input").ok_or_else(|| {
                StageError::Fatal("WebSocket sink stage requires an 'input' connection".into())
            })?);
        }
        Ok(input_rx.as_mut().unwrap())
    }

    /// Process packets from input
    async fn process_packets(&mut self, ctx: &mut StageContext<VoltageEegPacket, VoltageEegPacket>) -> Result<(), StageError> {
        let mut processed_count = 0u32;
        let yield_threshold = 100u32; // Process 100 packets before yielding

        loop {
            let input = Self::lazy_io(&mut self.input_rx, ctx)?;
            
            match input.try_recv() {
                Ok(Some(packet)) => {
                    trace!("Processing packet with {} samples", packet.samples.samples.len());
                    
                    // Convert packet to broadcast data
                    let broadcast_data = self.packet_to_broadcast_data(&packet);
                    
                    // Broadcast to all connected clients
                    if let Some(tx) = self.broadcast_tx.lock().unwrap().as_ref() {
                        if let Err(_) = tx.send(broadcast_data.clone()) {
                            debug!("No active WebSocket connections to broadcast to");
                        } else {
                            self.bytes_sent.fetch_add(broadcast_data.len() as u64, Ordering::Relaxed);
                        }
                    }
                    
                    self.packets_processed.fetch_add(1, Ordering::Relaxed);
                    
                    // Be a good citizen and yield to the scheduler periodically
                    processed_count += 1;
                    if processed_count >= yield_threshold {
                        processed_count = 0;
                        tokio::task::yield_now().await;
                    }
                }
                Ok(None) => {
                    // No packet available, yield and continue
                    tokio::task::yield_now().await;
                }
                Err(StageError::QueueClosed) => {
                    debug!("Input stream closed");
                    break;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
        
        Ok(())
    }

    /// Updates a stage parameter based on a key-value pair from the control plane.
    fn update_param(&mut self, key: &str, val: Value) -> Result<(), StageError> {
        match key {
            "bind_address" => {
                let mut config = self.config.lock().unwrap();
                config.bind_address = val.as_str().unwrap_or(&config.bind_address).to_string();
                info!("Updated bind_address to: {}", config.bind_address);
            }
            "port" => {
                let mut config = self.config.lock().unwrap();
                config.port = val.as_u64().unwrap_or(config.port as u64) as u16;
                info!("Updated port to: {}", config.port);
            }
            "format" => {
                let mut config = self.config.lock().unwrap();
                config.format = val.as_str().unwrap_or(&config.format).to_string();
                info!("Updated format to: {}", config.format);
            }
            "max_connections" => {
                let mut config = self.config.lock().unwrap();
                config.max_connections = val.as_u64().unwrap_or(config.max_connections as u64) as usize;
                info!("Updated max_connections to: {}", config.max_connections);
            }
            _ => return Err(StageError::BadParam(key.into())),
        }
        Ok(())
    }
}

#[async_trait]
impl DataPlaneStage<VoltageEegPacket, VoltageEegPacket> for WebSocketSinkStage {
    async fn run(&mut self, ctx: &mut StageContext<VoltageEegPacket, VoltageEegPacket>) -> Result<(), StageError> {
        self.is_running.store(true, Ordering::Relaxed);
        
        // Start WebSocket server
        self.start_server().await?;
        
        info!("WebSocket sink stage started");
        
        loop {
            // First, handle any incoming control messages
            ctrl_loop!(self, ctx);
            
            // Then, enter the packet processing loop
            self.process_packets(ctx).await?;
        }
    }
}

#[async_trait]
impl DataPlaneStageErased for WebSocketSinkStage {
    async fn run_erased(&mut self, context: &mut dyn ErasedStageContext) -> Result<(), StageError> {
        // Downcast the erased context back to the concrete type
        let concrete_context = context
            .as_any_mut()
            .downcast_mut::<StageContext<VoltageEegPacket, VoltageEegPacket>>()
            .ok_or_else(|| StageError::Fatal("Context type mismatch for WebSocketSinkStage".into()))?;
        
        self.run(concrete_context).await
    }
}

/// Handle individual WebSocket connection
async fn handle_websocket_connection(
    stream: TcpStream,
    mut rx: broadcast::Receiver<Vec<u8>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws_stream = accept_async(stream).await?;
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    
    // Handle incoming messages (if any) and outgoing broadcasts
    // Use a channel to coordinate between the two tasks
    let (pong_tx, mut pong_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    
    tokio::select! {
        // Handle incoming WebSocket messages (mostly just ping/pong)
        _ = async {
            while let Some(msg) = ws_receiver.next().await {
                match msg {
                    Ok(Message::Ping(data)) => {
                        // Send pong data through channel instead of directly
                        if pong_tx.send(data).is_err() {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(_) => break,
                    _ => {} // Ignore other message types
                }
            }
        } => {}
        
        // Handle outgoing broadcasts and pong responses
        _ = async {
            loop {
                tokio::select! {
                    // Handle broadcast data
                    data = rx.recv() => {
                        match data {
                            Ok(data) => {
                                if let Err(_) = ws_sender.send(Message::Binary(data)).await {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    // Handle pong responses
                    Some(pong_data) = pong_rx.recv() => {
                        if let Err(_) = ws_sender.send(Message::Pong(pong_data)).await {
                            break;
                        }
                    }
                    else => break,
                }
            }
        } => {}
    }
    
    Ok(())
}

/// Factory for creating WebSocket sink stages
pub struct WebSocketSinkStageFactory {
    config: WebSocketSinkConfig,
}

impl WebSocketSinkStageFactory {
    pub fn new() -> Self {
        Self {
            config: WebSocketSinkConfig::default(),
        }
    }

    pub fn with_config(config: WebSocketSinkConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl DataPlaneStageFactory<VoltageEegPacket, VoltageEegPacket> for WebSocketSinkStageFactory {
    async fn create_stage(&self, _params: &StageParams) -> PipelineResult<Box<dyn DataPlaneStage<VoltageEegPacket, VoltageEegPacket>>> {
        Ok(Box::new(WebSocketSinkStage::new(self.config.clone())))
    }

    fn stage_type(&self) -> &'static str {
        "websocket_sink"
    }

    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "bind_address": {
                    "type": "string",
                    "description": "WebSocket server bind address",
                    "default": "127.0.0.1"
                },
                "port": {
                    "type": "integer",
                    "description": "WebSocket server port",
                    "default": 8080,
                    "minimum": 1,
                    "maximum": 65535
                },
                "format": {
                    "type": "string",
                    "description": "Data format for broadcasting",
                    "enum": ["json", "binary"],
                    "default": "json"
                },
                "max_connections": {
                    "type": "integer",
                    "description": "Maximum number of concurrent connections",
                    "default": 100,
                    "minimum": 1
                },
                "buffer_size": {
                    "type": "integer",
                    "description": "Buffer size for broadcast channel",
                    "default": 1000,
                    "minimum": 1
                }
            }
        })
    }
}

#[async_trait]
impl ErasedDataPlaneStageFactory for WebSocketSinkStageFactory {
    async fn create_erased_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn DataPlaneStageErased>> {
        let stage = WebSocketSinkStage::new(self.config.clone());
        Ok(Box::new(stage))
    }

    fn stage_type(&self) -> &'static str {
        DataPlaneStageFactory::<VoltageEegPacket, VoltageEegPacket>::stage_type(self)
    }

    fn parameter_schema(&self) -> serde_json::Value {
        DataPlaneStageFactory::<VoltageEegPacket, VoltageEegPacket>::parameter_schema(self)
    }
}

// Register the stage with the static registry
inventory::submit! {
    StaticStageRegistrar {
        factory_fn: || Box::new(WebSocketSinkStageFactory::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{PacketHeader};
    use tokio::sync::mpsc;
    use std::pin::Pin;
    use std::future::Future;

    struct MockInput {
        packets: Vec<Packet<VoltageEegPacket>>,
        index: usize,
    }

    impl MockInput {
        fn new(packets: Vec<Packet<VoltageEegPacket>>) -> Self {
            Self { packets, index: 0 }
        }
    }

    #[async_trait]
    impl Input<VoltageEegPacket> for MockInput {
        async fn recv(&mut self) -> Result<Option<Packet<VoltageEegPacket>>, StageError> {
            if self.index < self.packets.len() {
                let packet = Packet::new_for_test(VoltageEegPacket {
                    samples: vec![1.0, 2.0, 3.0],
                });
                self.index += 1;
                Ok(Some(packet))
            } else {
                Ok(None)
            }
        }

        fn try_recv(&mut self) -> Result<Option<Packet<VoltageEegPacket>>, StageError> {
            if self.index < self.packets.len() {
                let packet = Packet::new_for_test(VoltageEegPacket {
                    samples: vec![1.0, 2.0, 3.0],
                });
                self.index += 1;
                Ok(Some(packet))
            } else {
                Ok(None)
            }
        }
    }

    #[tokio::test]
    async fn test_websocket_sink_factory_creation() {
        let factory = WebSocketSinkStageFactory::new();
        assert_eq!(crate::stage::DataPlaneStageFactory::stage_type(&factory), "websocket_sink");
        
        let params = StageParams::default();
        let stage = factory.create_stage(&params).await.unwrap();
        
        // Basic test that stage was created
        // Note: DataPlaneStage trait doesn't have stage_type method, so we'll skip this assertion
        // assert!(stage.stage_type() == "websocket_sink");
    }

    #[tokio::test]
    async fn test_websocket_sink_parameter_schema() {
        let factory = WebSocketSinkStageFactory::new();
        let schema = crate::stage::DataPlaneStageFactory::parameter_schema(&factory);
        
        assert!(schema.is_object());
        let properties = schema.get("properties").unwrap();
        assert!(properties.get("bind_address").is_some());
        assert!(properties.get("port").is_some());
        assert!(properties.get("format").is_some());
        assert!(properties.get("max_connections").is_some());
        assert!(properties.get("buffer_size").is_some());
    }

    #[test]
    fn test_packet_to_broadcast_data_json() {
        let stage = WebSocketSinkStage::new(WebSocketSinkConfig {
            format: "json".to_string(),
            ..Default::default()
        });
        
        let packet = Packet::new_test(
            PacketHeader {
                batch_size: 3,
                timestamp: 12345,
            },
            VoltageEegPacket {
                samples: vec![1.0, 2.0, 3.0],
            }
        );
        
        let data = stage.packet_to_broadcast_data(&packet);
        assert!(!data.is_empty());
        
        // Verify it's valid JSON
        let json: serde_json::Value = serde_json::from_slice(&data).unwrap();
        assert_eq!(json["timestamp"], 12345);
        assert_eq!(json["batch_size"], 3);
        assert_eq!(json["format"], "voltage");
    }

    #[test]
    fn test_packet_to_broadcast_data_binary() {
        let stage = WebSocketSinkStage::new(WebSocketSinkConfig {
            format: "binary".to_string(),
            ..Default::default()
        });
        
        let packet = Packet::new_test(
            PacketHeader {
                batch_size: 3,
                timestamp: 12345,
            },
            VoltageEegPacket {
                samples: vec![1.0, 2.0, 3.0],
            }
        );
        
        let data = stage.packet_to_broadcast_data(&packet);
        assert!(!data.is_empty());
        
        // Verify binary format structure
        assert!(data.len() >= 8 + 8 + 4 + 3 * 4); // timestamp + batch_size + sample_count + samples
    }
}