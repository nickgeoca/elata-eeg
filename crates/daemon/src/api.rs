use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{sse::Event, IntoResponse, Json, Sse},
    routing::post,
    Router,
};
use futures::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use flume::Sender;
use pipeline::{control::ControlCommand, data::RtPacket, executor::Executor};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use tokio::sync::broadcast;

// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub pipelines: Arc<Mutex<HashMap<String, PathBuf>>>,
    pub sse_tx: broadcast::Sender<String>,
    pub pipeline_handle: Arc<Mutex<Option<PipelineHandle>>>,
}

// Will hold the running pipeline's sender channel
pub struct PipelineHandle {
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
                            .unwrap()
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
use std::thread;

pub async fn start_pipeline_handler(
    Path(pipeline_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("Starting pipeline: {}", pipeline_id);

    let config_path = {
        let pipelines = state.pipelines.lock().unwrap();
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

    let graph = match PipelineGraph::build(&config, &registry, event_tx, None) {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to build graph: {}", e),
            )
                .into_response()
        }
    };

    let (executor, input_tx) = Executor::new(graph);

    let mut handle = state.pipeline_handle.lock().unwrap();
    *handle = Some(PipelineHandle {
        executor: Some(executor),
        input_tx,
    });

    // Spawn a task to forward pipeline events to SSE clients
    let sse_tx = state.sse_tx.clone();
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv_async().await {
            let event_json = match serde_json::to_string(&event) {
                Ok(json) => json,
                Err(e) => {
                    tracing::error!("Failed to serialize pipeline event: {}", e);
                    continue;
                }
            };
            
            if let Err(e) = sse_tx.send(event_json) {
                tracing::warn!("Failed to send SSE event: {}", e);
                // If sending fails, it's likely because there are no subscribers
                // We can continue, as new subscribers will receive future events
            }
        }
    });

    (StatusCode::OK, "Pipeline started").into_response()
}

pub async fn stop_pipeline_handler(State(state): State<AppState>) -> impl IntoResponse {
    tracing::info!("Stopping pipeline");

    if let Some(mut handle) = state.pipeline_handle.lock().unwrap().take() {
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
        let pipelines = state.pipelines.lock().unwrap();
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

    if let Some(ref mut handle) = *state.pipeline_handle.lock().unwrap() {
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
    if let Some(handle) = &*state.pipeline_handle.lock().unwrap() {
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

use tokio_stream::StreamExt;

use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

pub async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, BroadcastStreamRecvError>>> {
    let stream =
        tokio_stream::wrappers::BroadcastStream::new(state.sse_tx.subscribe()).map(|res| {
            res.map(|msg| Event::default().data(msg))
        });

    Sse::new(Box::pin(stream))
}

use axum::routing::get;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/api/pipelines", get(list_pipelines_handler))
        .route("/api/pipelines/:id/start", post(start_pipeline_handler))
        .route("/api/pipelines/stop", post(stop_pipeline_handler))
        .route("/api/pipelines/:id", post(update_pipeline_handler))
        .route("/api/control", post(control_handler))
        .route("/api/state", get(state_handler))
        .route("/api/events", get(sse_handler))
        .with_state(state)
}