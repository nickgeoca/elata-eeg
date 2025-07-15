use warp::ws::{Message, WebSocket};
use warp::Filter;
use serde::Serialize;
use futures_util::{StreamExt, SinkExt};
use tokio::sync::mpsc;
use pipeline::config::SystemConfig;
use pipeline::control::ControlCommand;

fn with_mpsc_tx<T: Send + 'static>(
    tx: mpsc::Sender<T>,
) -> impl Filter<Extract = (mpsc::Sender<T>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || tx.clone())
}

#[derive(Serialize)]
pub struct CommandResponse {
    pub status: String,
    pub message: String,
}

use eeg_types::{SensorError, WsControlCommand};
use tokio::sync::broadcast;

/// Handles WebSocket connections for pipeline control.
///
/// This function manages the lifecycle of a WebSocket connection, listening for
/// incoming messages that can alter the state of the pipeline. It can deserialize
/// `SystemConfig` objects and send them to the pipeline as `ControlCommand::Reconfigure`.
pub async fn handle_control_websocket(
    ws: WebSocket,
    control_tx: mpsc::Sender<ControlCommand>,
    mut error_rx: broadcast::Receiver<SensorError>,
) {
    let (mut ws_tx, mut ws_rx) = ws.split();
    tracing::info!("Control WebSocket client connected");

    loop {
        tokio::select! {
            Some(result) = ws_rx.next() => {
                let msg = match result {
                    Ok(msg) => msg,
                    Err(e) => {
                        tracing::error!("Control WebSocket error: {}", e);
                        break;
                    }
                };

                if msg.is_close() {
                    break;
                }

                if let Ok(text) = msg.to_str() {
                    // Try to deserialize as WsControlCommand first
                    if let Ok(ws_cmd) = serde_json::from_str::<WsControlCommand>(text) {
                        let cmd = match ws_cmd {
                            WsControlCommand::SetTestState { value } => {
                                ControlCommand::SetTestState(value)
                            }
                        };
                        if control_tx.send(cmd).await.is_err() {
                            tracing::error!("Failed to send command to pipeline");
                            break;
                        }
                    } else if let Ok(new_config) = serde_json::from_str::<SystemConfig>(text) {
                        // Fallback to SystemConfig for reconfiguration
                        tracing::info!("Received new system configuration via WebSocket");
                        let cmd = ControlCommand::Reconfigure(new_config);
                        if control_tx.send(cmd).await.is_err() {
                            tracing::error!("Failed to send reconfigure command to pipeline");
                            break;
                        }
                        let response = CommandResponse {
                            status: "ok".to_string(),
                            message: "Configuration sent to pipeline".to_string(),
                        };
                        if let Ok(json) = serde_json::to_string(&response) {
                            if ws_tx.send(Message::text(json)).await.is_err() {
                                break;
                            }
                        }
                    } else {
                        tracing::warn!("Failed to deserialize command or config");
                        let response = CommandResponse {
                            status: "error".to_string(),
                            message: "Invalid command or SystemConfig format".to_string(),
                        };
                        if let Ok(json) = serde_json::to_string(&response) {
                            if ws_tx.send(Message::text(json)).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            },
            Ok(error) = error_rx.recv() => {
                let response = CommandResponse {
                    status: "error".to_string(),
                    message: error.to_string(),
                };
                if let Ok(json) = serde_json::to_string(&response) {
                    if ws_tx.send(Message::text(json)).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    tracing::info!("Control WebSocket client disconnected");
}

// Set up WebSocket routes and server
pub fn setup_websocket_routes(
    control_tx: mpsc::Sender<ControlCommand>,
    error_tx: broadcast::Sender<SensorError>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let control_route = warp::path("control")
        .and(warp::ws())
        .and(with_mpsc_tx(control_tx))
        .and(warp::any().map(move || error_tx.subscribe()))
        .map(|ws: warp::ws::Ws, tx: mpsc::Sender<ControlCommand>, rx: broadcast::Receiver<SensorError>| {
            ws.on_upgrade(move |socket| handle_control_websocket(socket, tx, rx))
        });

    control_route
}