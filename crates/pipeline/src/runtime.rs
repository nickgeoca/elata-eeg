//! Pipeline runtime for executing pipeline graphs.

use crate::config::SystemConfig;
use crate::control::{ControlCommand, PipelineEvent};
use crate::error::StageError;
use crate::graph::PipelineGraph;
use crate::registry::StageRegistry;
use crate::stage::StageContext;
use tokio::sync::mpsc::{Receiver, Sender};
use eeg_types::Packet;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// Runs the pipeline in a synchronous loop, processing data and control messages.
///
/// This function is the core of the synchronous pipeline execution model. It builds a
/// pipeline graph from the provided configuration and then enters a `select!` loop
/// to react to incoming data from the sensor bridge and control commands.
///
/// # Arguments
/// * `config` - The system configuration for building the pipeline.
/// * `registry` - The registry of available pipeline stages.
/// * `data_rx` - The channel for receiving `BridgeMsg` from the sensor bridge.
/// * `control_rx` - The channel for receiving `ControlCommand` messages.
///
/// # Returns
/// `Ok(())` if the pipeline shuts down gracefully, or a `StageError` if an
/// unrecoverable error occurs.
pub async fn run<T>(
    config: SystemConfig,
    registry: Arc<StageRegistry<T, T>>,
    mut data_rx: Receiver<Packet<T>>,
    mut control_rx: Receiver<ControlCommand>,
    event_tx: Sender<PipelineEvent>,
) -> Result<(), StageError>
where
    T: Clone + Send + 'static,
{
    info!("Initializing synchronous pipeline runtime...");
    let context = StageContext::new(event_tx.clone());
    let mut graph = PipelineGraph::build(&config, &registry, context)
        .await
        .map_err(|e| StageError::BadConfig(e.to_string()))?;
    info!("Pipeline graph built successfully.");

    let mut running = true;
    loop {
        tokio::select! {
            Some(cmd) = control_rx.recv() => {
                if matches!(cmd, ControlCommand::Shutdown) {
                    info!("Shutdown command received, exiting pipeline loop.");
                    break;
                }
                handle_control_cmd(cmd, &mut graph).await;
            },
            Some(packet) = data_rx.recv() => {
                handle_data_msg(packet, &mut graph).await;
            },
            else => {
                info!("All channels closed, exiting pipeline loop.");
                break;
            }
        }
    }

    info!("Pipeline has shut down. Sending acknowledgment.");
    event_tx
        .send(PipelineEvent::ShutdownAck)
        .await
        .map_err(|e| StageError::SendError(e.to_string()))
}

async fn handle_data_msg<T: Clone + Send + 'static>(
    packet: Packet<T>,
    graph: &mut PipelineGraph<T>,
) {

    info!("Received data packet with timestamp: {}", packet.header.ts_ns);

    // This is a simplified approach where we just kick off the process at the entry points.
    // A more robust runtime would manage a processing loop for all stages.
    for entry_point_name in &graph.entry_points {
        if let Some(node) = graph.nodes.get(entry_point_name) {
            let mut stage = node.stage.lock().await;
            let mut context = graph.context.clone();
            match stage.process(packet.clone(), &mut context).await {
                Ok(Some(output_packet)) => {
                    if node.tx.send(output_packet).is_err() {
                        warn!(
                            "Entry point stage {} has no subscribers, data will be dropped.",
                            node.name
                        );
                    }
                }
                Ok(None) => {
                    // Stage consumed the packet, nothing to forward.
                }
                Err(e) => {
                    warn!(
                        "Error processing packet in entry point stage {}: {}",
                        node.name, e
                    );
                }
            }
        }
    }
}

async fn handle_control_cmd<T: Clone + Send + 'static>(
    cmd: ControlCommand,
    graph: &mut PipelineGraph<T>,
) {
    info!("Forwarding control command to pipeline graph: {:?}", cmd);
    graph.forward_control_command(cmd).await;
}