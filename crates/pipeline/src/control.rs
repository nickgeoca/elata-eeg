//! Control plane WebSocket JSON RPC interface for pipeline management
//!
//! This module provides a WebSocket-based JSON RPC interface for controlling
//! the pipeline runtime from external clients (like the GUI). It supports:
//! - Pipeline state management (start, stop, pause, resume)
//! - Parameter updates with recording lock enforcement
//! - Real-time status and metrics reporting
//! - Stage-level control and monitoring

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::error::{PipelineError, PipelineResult};
use crate::runtime::{PipelineRuntime, PipelineState};

/// JSON RPC request structure
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
    pub id: Option<Value>,
}

/// JSON RPC response structure
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Option<Value>,
}

/// JSON RPC error structure
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Control plane command router
pub struct ControlPlaneRouter {
    /// Pipeline runtime instance
    runtime: Arc<RwLock<PipelineRuntime>>,
    /// Active WebSocket connections
    connections: Arc<RwLock<HashMap<Uuid, mpsc::UnboundedSender<Message>>>>,
    /// Metrics broadcast channel
    metrics_tx: mpsc::UnboundedSender<Value>,
    /// Status broadcast channel
    status_tx: mpsc::UnboundedSender<PipelineState>,
}

/// Parameters for pipeline control methods
#[derive(Debug, Deserialize)]
pub struct StartParams {
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateParamParams {
    pub stage_id: String,
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Deserialize)]
pub struct LoadPipelineParams {
    pub config: Value, // Will be deserialized to PipelineConfig
}

impl ControlPlaneRouter {
    /// Create a new control plane router
    pub fn new(runtime: Arc<RwLock<PipelineRuntime>>) -> Self {
        let (metrics_tx, _) = mpsc::unbounded_channel();
        let (status_tx, _) = mpsc::unbounded_channel();
        
        Self {
            runtime,
            connections: Arc::new(RwLock::new(HashMap::new())),
            metrics_tx,
            status_tx,
        }
    }

    /// Handle a new WebSocket connection
    pub async fn handle_connection(
        &self,
        stream: tokio::net::TcpStream,
    ) -> PipelineResult<()> {
        let ws_stream = accept_async(stream).await
            .map_err(|e| PipelineError::ChannelError(format!("WebSocket handshake failed: {}", e)))?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        let connection_id = Uuid::new_v4();
        
        info!("New control plane connection: {}", connection_id);

        // Create a channel for sending messages to this connection
        let (tx, mut rx) = mpsc::unbounded_channel();
        
        // Store the connection
        {
            let mut connections = self.connections.write().await;
            connections.insert(connection_id, tx);
        }

        // Spawn task to handle outgoing messages
        let connections_clone = self.connections.clone();
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                if let Err(e) = ws_sender.send(message).await {
                    warn!("Failed to send message to connection {}: {}", connection_id, e);
                    break;
                }
            }
            
            // Remove connection when done
            let mut connections = connections_clone.write().await;
            connections.remove(&connection_id);
            info!("Control plane connection closed: {}", connection_id);
        });

        // Handle incoming messages
        let router = self.clone();
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Err(e) = router.handle_message(&text, connection_id).await {
                        error!("Error handling message from {}: {}", connection_id, e);
                    }
                }
                Ok(Message::Close(_)) => {
                    info!("Connection {} closed by client", connection_id);
                    break;
                }
                Err(e) => {
                    error!("WebSocket error for connection {}: {}", connection_id, e);
                    break;
                }
                _ => {} // Ignore other message types
            }
        }

        Ok(())
    }

    /// Handle a JSON RPC message
    async fn handle_message(&self, text: &str, connection_id: Uuid) -> PipelineResult<()> {
        debug!("Received message from {}: {}", connection_id, text);

        let request: JsonRpcRequest = serde_json::from_str(text)
            .map_err(|e| PipelineError::SerializationError(e))?;

        let response = self.process_request(request).await;
        let response_text = serde_json::to_string(&response)
            .map_err(|e| PipelineError::SerializationError(e))?;

        self.send_to_connection(connection_id, Message::Text(response_text)).await?;
        Ok(())
    }

    /// Process a JSON RPC request
    async fn process_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let result = match request.method.as_str() {
            "pipeline.start" => self.handle_start(request.params).await,
            "pipeline.stop" => self.handle_stop().await,
            "pipeline.pause" => self.handle_pause().await,
            "pipeline.resume" => self.handle_resume().await,
            "pipeline.load" => self.handle_load_pipeline(request.params).await,
            "pipeline.status" => self.handle_get_status().await,
            "pipeline.metrics" => self.handle_get_metrics().await,
            "stage.update_param" => self.handle_update_param(request.params).await,
            _ => Err(PipelineError::InvalidInput {
                message: format!("Unknown method: {}", request.method),
            }),
        };

        match result {
            Ok(value) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: Some(value),
                error: None,
                id: request.id,
            },
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(JsonRpcError {
                    code: self.error_to_code(&e),
                    message: e.to_string(),
                    data: None,
                }),
                id: request.id,
            },
        }
    }

    /// Handle pipeline start command
    async fn handle_start(&self, params: Option<Value>) -> PipelineResult<Value> {
        let _params: StartParams = if let Some(p) = params {
            serde_json::from_value(p)?
        } else {
            StartParams { force: false }
        };

        let mut runtime = self.runtime.write().await;
        runtime.start().await?;
        
        // Broadcast status change
        let _ = self.status_tx.send(PipelineState::Running);
        
        Ok(json!({"status": "started"}))
    }

    /// Handle pipeline stop command
    async fn handle_stop(&self) -> PipelineResult<Value> {
        let mut runtime = self.runtime.write().await;
        runtime.stop().await?;
        
        // Broadcast status change
        let _ = self.status_tx.send(PipelineState::Idle);
        
        Ok(json!({"status": "stopped"}))
    }

    /// Handle pipeline pause command
    async fn handle_pause(&self) -> PipelineResult<Value> {
        let mut runtime = self.runtime.write().await;
        runtime.pause().await?;
        
        // Broadcast status change
        let _ = self.status_tx.send(PipelineState::Paused);
        
        Ok(json!({"status": "paused"}))
    }

    /// Handle pipeline resume command
    async fn handle_resume(&self) -> PipelineResult<Value> {
        let mut runtime = self.runtime.write().await;
        runtime.resume().await?;
        
        // Broadcast status change
        let _ = self.status_tx.send(PipelineState::Running);
        
        Ok(json!({"status": "resumed"}))
    }

    /// Handle load pipeline command
    async fn handle_load_pipeline(&self, params: Option<Value>) -> PipelineResult<Value> {
        let params: LoadPipelineParams = serde_json::from_value(params
            .ok_or_else(|| PipelineError::InvalidInput {
                message: "Missing config parameter".to_string(),
            })?)?;

        let config = serde_json::from_value(params.config)?;
        let mut runtime = self.runtime.write().await;
        runtime.load_pipeline(&config).await?;
        
        Ok(json!({"status": "loaded"}))
    }

    /// Handle get status command
    async fn handle_get_status(&self) -> PipelineResult<Value> {
        let runtime = self.runtime.read().await;
        let state = runtime.state().await;
        let graph = runtime.graph().await;
        
        let graph_info = if let Some(graph) = graph {
            let graph_guard = graph.read().await;
            json!({
                "stage_count": graph_guard.stages.len(),
                "sources": graph_guard.sources,
                "sinks": graph_guard.sinks,
                "state": graph_guard.state
            })
        } else {
            json!(null)
        };

        Ok(json!({
            "state": state,
            "graph": graph_info
        }))
    }

    /// Handle get metrics command
    async fn handle_get_metrics(&self) -> PipelineResult<Value> {
        let runtime = self.runtime.read().await;
        let metrics = runtime.metrics().await;
        Ok(serde_json::to_value(metrics)?)
    }

    /// Handle stage parameter update command
    async fn handle_update_param(&self, params: Option<Value>) -> PipelineResult<Value> {
        let params: UpdateParamParams = serde_json::from_value(params
            .ok_or_else(|| PipelineError::InvalidInput {
                message: "Missing parameters".to_string(),
            })?)?;

        let runtime = self.runtime.read().await;
        runtime.update_stage_parameter(&params.stage_id, params.key.clone(), params.value).await?;
        
        Ok(json!({
            "status": "updated",
            "stage_id": params.stage_id,
            "key": params.key
        }))
    }

    /// Send a message to a specific connection
    async fn send_to_connection(&self, connection_id: Uuid, message: Message) -> PipelineResult<()> {
        let connections = self.connections.read().await;
        if let Some(tx) = connections.get(&connection_id) {
            tx.send(message)
                .map_err(|_| PipelineError::ChannelError("Failed to send message".to_string()))?;
        }
        Ok(())
    }

    /// Broadcast a message to all connections
    pub async fn broadcast(&self, message: Message) {
        let connections = self.connections.read().await;
        for tx in connections.values() {
            let _ = tx.send(message.clone());
        }
    }

    /// Convert pipeline error to JSON RPC error code
    fn error_to_code(&self, error: &PipelineError) -> i32 {
        match error {
            PipelineError::InvalidInput { .. } => -32602, // Invalid params
            PipelineError::InvalidConfiguration { .. } => -32600, // Invalid request
            PipelineError::AlreadyRunning => -32001, // Custom: Already running
            PipelineError::NotRunning => -32002, // Custom: Not running
            PipelineError::InvalidState(_) => -32003, // Custom: Invalid state
            PipelineError::StageNotFound { .. } => -32004, // Custom: Stage not found
            _ => -32603, // Internal error
        }
    }
}

impl Clone for ControlPlaneRouter {
    fn clone(&self) -> Self {
        Self {
            runtime: self.runtime.clone(),
            connections: self.connections.clone(),
            metrics_tx: self.metrics_tx.clone(),
            status_tx: self.status_tx.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::DataPlaneStageRegistry;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_json_rpc_parsing() {
        let request_text = r#"{"jsonrpc":"2.0","method":"pipeline.status","id":1}"#;
        let request: JsonRpcRequest = serde_json::from_str(request_text).unwrap();
        
        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.method, "pipeline.status");
        assert_eq!(request.id, Some(json!(1)));
    }

    #[tokio::test]
    async fn test_control_plane_creation() {
        let registry = Arc::new(DataPlaneStageRegistry::new());
        let runtime = Arc::new(RwLock::new(PipelineRuntime::new(registry)));
        let router = ControlPlaneRouter::new(runtime);
        
        // Test that router was created successfully
        assert_eq!(router.connections.read().await.len(), 0);
    }
}