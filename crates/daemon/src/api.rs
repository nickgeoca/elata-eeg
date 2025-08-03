use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        FromRef, Path, State,
    },
    http::StatusCode,
    response::{sse::Event, IntoResponse, Json, Sse},
    routing::post,
    Router,
};
use futures::{
    stream::{self, Stream, StreamExt},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;
use eeg_types::{comms::BrokerMessage, data::SensorMeta};
use flume::Sender;
use pipeline::{control::ControlCommand, data::RtPacket, executor::Executor};
use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
// Shared application state
use crate::websocket_broker::WebSocketBroker;

#[derive(Clone)]
pub struct AppState {
    pub pipelines: Arc<Mutex<HashMap<String, PathBuf>>>,
    pub sse_tx: broadcast::Sender<String>,
    pub pipeline_handle: Arc<Mutex<Option<PipelineHandle>>>,
    pub source_meta_cache: Arc<Mutex<Option<SensorMeta>>>,
    pub broker: Arc<WebSocketBroker>,
    pub websocket_sender: broadcast::Sender<Arc<BrokerMessage>>,
}

impl FromRef<AppState> for Arc<WebSocketBroker> {
    fn from_ref(state: &AppState) -> Self {
        state.broker.clone()
    }
}

// Will hold the running pipeline's sender channel
pub struct PipelineHandle {
    pub id: String,
    pub executor: Option<Executor>,
    pub input_tx: Sender<Arc<RtPacket>>,
}

#[derive(Serialize, Deserialize)]
pub struct PipelineInfo {
    id: String,
    name: String,
    description: String,
}

/// Scans for pipeline YAML files and returns a list of them.
pub async fn list_pipelines_handler(State(state): State<AppState>) -> Json<Value> {
    let mut pipelines = vec![];
    let paths = [PathBuf::from("pipelines"), PathBuf::from("crates/daemon")];

    for path in paths.iter() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let file_path = entry.path();
                if file_path.extension().and_then(|s| s.to_str()) == Some("yaml") {
                    if let Some(id) = file_path.file_stem().and_then(|s| s.to_str()) {
                        let p_info = PipelineInfo {
                            id: id.to_string(),
                            name: id.replace('_', " ").to_string(),
                            description: format!("Source: {}", file_path.display()),
                        };
                        pipelines.push(p_info);
                        state
                            .pipelines
                            .lock()
                            .await
                            .insert(id.to_string(), file_path);
                    }
                }
            }
        }
    }
    Json(json!(pipelines))
}

use pipeline::{
    config::SystemConfig, control::PipelineEvent, graph::PipelineGraph, registry::StageRegistry,
};
pub async fn start_pipeline_handler(
    Path(pipeline_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("Request to start pipeline: {}", pipeline_id);

    let mut handle = state.pipeline_handle.lock().await;

    // Check if a pipeline is already running
    if let Some(existing_handle) = &*handle {
        if existing_handle.id == pipeline_id {
            tracing::info!("Pipeline '{}' is already running. Request is idempotent.", pipeline_id);
            return (StatusCode::OK, "Pipeline already running").into_response();
        } else {
            tracing::warn!(
                "Conflict: Pipeline '{}' is running, but request was for '{}'",
                existing_handle.id,
                pipeline_id
            );
            let body = json!({
                "error": "Conflict: A different pipeline is already running.",
                "running_pipeline_id": existing_handle.id
            });
            return (StatusCode::CONFLICT, Json(body)).into_response();
        }
    }

    // No pipeline is running, proceed to start one
    tracing::info!("No pipeline running. Starting '{}'", pipeline_id);

    let config_path = {
        let pipelines = state.pipelines.lock().await;
        match pipelines.get(&pipeline_id) {
            Some(path) => path.clone(),
            None => return (StatusCode::NOT_FOUND, "Pipeline not found").into_response(),
        }
    };

    let config_str = match fs::read_to_string(config_path) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read config: {}", e),
            )
                .into_response()
        }
    };

    let config: SystemConfig = match serde_yaml::from_str(&config_str) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to parse config: {}", e),
            )
                .into_response()
        }
    };

    let (event_tx, event_rx) = flume::bounded(100);
    let mut registry = StageRegistry::new();
    pipeline::stages::register_builtin_stages(&mut registry);

    let graph = match PipelineGraph::build(
        &config,
        &registry,
        event_tx,
        None,
        &None,
        Some(state.websocket_sender.clone()),
    ) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to build graph: {}", e),
            )
                .into_response()
        }
    };

    let (executor, input_tx, fatal_error_rx) = Executor::new(graph);

    *handle = Some(PipelineHandle {
        id: pipeline_id.clone(),
        executor: Some(executor),
        input_tx,
    });

    // Spawn a task to forward pipeline events to SSE clients
    let sse_tx_clone_for_event = state.sse_tx.clone();
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv_async().await {
            let event_json = match serde_json::to_string(&event) {
                Ok(json) => json,
                Err(e) => {
                    tracing::error!("Failed to serialize pipeline event: {}", e);
                    continue;
                }
            };

            if sse_tx_clone_for_event.send(event_json).is_err() {
                // This can happen if there are no subscribers, which is fine.
                tracing::debug!("No active SSE subscribers to send event to.");
            }
        }
    });

    // Spawn a task to listen for fatal errors
    let pipeline_handle_clone = state.pipeline_handle.clone();
    let sse_tx_clone_for_fatal = state.sse_tx.clone();
    tokio::spawn(async move {
        if let Ok(panic_payload) = fatal_error_rx.recv_async().await {
            tracing::error!("Fatal pipeline error detected. Shutting down.");

            // Attempt to extract a string from the panic payload
            let error_msg = if let Some(s) = panic_payload.downcast_ref::<&'static str>() {
                s.to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic payload".to_string()
            };

            // Stop the pipeline
            if let Some(mut handle) = pipeline_handle_clone.lock().await.take() {
                if let Some(executor) = handle.executor.take() {
                    executor.stop();
                }
            }

            // Broadcast the failure event
            let event = PipelineEvent::PipelineFailed {
                error: error_msg,
            };
            if let Ok(event_json) = serde_json::to_string(&event) {
                if sse_tx_clone_for_fatal.send(event_json).is_err() {
                    tracing::warn!("Failed to send pipeline failure SSE event.");
                }
            }
        }
    });

    (StatusCode::OK, "Pipeline started").into_response()
}

pub async fn stop_pipeline_handler(State(state): State<AppState>) -> impl IntoResponse {
    tracing::info!("Stopping pipeline");

    if let Some(mut handle) = state.pipeline_handle.lock().await.take() {
        if let Some(executor) = handle.executor.take() {
            executor.stop();
        }
        (StatusCode::OK, "Pipeline stopped").into_response()
    } else {
        (StatusCode::NOT_FOUND, "No pipeline running").into_response()
    }
}

#[derive(Deserialize)]
pub struct UpdatePipelinePayload {
    pub config: Value,
}

pub async fn update_pipeline_handler(
    Path(pipeline_id): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<UpdatePipelinePayload>,
) -> impl IntoResponse {
    tracing::info!("Updating pipeline: {}", pipeline_id);

    let path = {
        let pipelines = state.pipelines.lock().await;
        match pipelines.get(&pipeline_id) {
            Some(path) => path.clone(),
            None => return (StatusCode::NOT_FOUND, "Pipeline not found").into_response(),
        }
    };

    let yaml_str = match serde_yaml::to_string(&payload.config) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to serialize config: {}", e),
            )
                .into_response()
        }
    };

    match fs::write(path, yaml_str) {
        Ok(_) => (StatusCode::OK, "Pipeline updated").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write config: {}", e),
        )
            .into_response(),
    }
}


pub async fn control_handler(
    State(state): State<AppState>,
    Json(payload): Json<ControlCommand>,
) -> impl IntoResponse {
    tracing::info!("Received control command: {:?}", payload);

    if let Some(ref mut handle) = *state.pipeline_handle.lock().await {
        if let Some(ref mut executor) = handle.executor {
            // Forward the control command to the executor
            if let Err(e) = executor.handle_control_command(&payload) {
                tracing::error!("Failed to handle control command: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "Command failed").into_response();
            }
            (StatusCode::ACCEPTED, "Command received").into_response()
        } else {
            (StatusCode::NOT_FOUND, "No pipeline running").into_response()
        }
    } else {
        (StatusCode::NOT_FOUND, "No pipeline running").into_response()
    }
}

pub async fn state_handler(State(state): State<AppState>) -> impl IntoResponse {
    tracing::info!("Fetching current state");
    if let Some(handle) = &*state.pipeline_handle.lock().await {
        if let Some(executor) = &handle.executor {
            let config = executor.get_current_config();
            Json(json!(config)).into_response()
        } else {
            (StatusCode::NOT_FOUND, "No pipeline running").into_response()
        }
    } else {
        (StatusCode::NOT_FOUND, "No pipeline running").into_response()
    }
}


use tokio_stream::wrappers::BroadcastStream;

pub async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut initial_events = Vec::new();

    // Immediately send the current pipeline state if a pipeline is running
    if let Some(handle) = &*state.pipeline_handle.lock().await {
        if let Some(executor) = &handle.executor {
            let config = executor.get_current_config();
            let event = PipelineEvent::PipelineStarted {
                id: handle.id.clone(),
                config,
            };
            if let Ok(event_json) = serde_json::to_string(&event) {
                initial_events.push(Event::default().data(event_json));
            }
        }
    }

    // Also send the cached SourceReady event if it exists
    if let Some(meta) = &*state.source_meta_cache.lock().await {
        let event = PipelineEvent::SourceReady { meta: meta.clone() };
        if let Ok(event_json) = serde_json::to_string(&event) {
            initial_events.push(Event::default().data(event_json));
        }
    }

    let initial_stream = stream::iter(initial_events.into_iter().map(Ok));
    let live_stream = BroadcastStream::new(state.sse_tx.subscribe()).filter_map(|res| async {
        match res {
            Ok(msg) => Some(Ok(Event::default().data(msg))),
            Err(e) => {
                tracing::error!("SSE broadcast stream error: {}", e);
                None
            }
        }
    });

    let stream = initial_stream.chain(live_stream);

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keep-alive"),
    )
}

use axum::routing::get;
use std::convert::Infallible;

pub fn create_router() -> Router<AppState> {
    Router::new()
        .route("/api/pipelines", get(list_pipelines_handler))
        .route("/api/pipelines/:id/start", post(start_pipeline_handler))
        .route("/api/pipelines/stop", post(stop_pipeline_handler))
        .route("/api/pipelines/:id", post(update_pipeline_handler))
        .route("/api/control", post(control_handler))
        .route("/api/state", get(state_handler))
        .route("/api/events", get(sse_handler))
}