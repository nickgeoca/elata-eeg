//! Pipeline runtime for executing pipeline graphs.

use crate::control::{ControlCommand, PipelineEvent};
use crate::data::Packet;
use crate::error::PipelineError;
use crate::graph::PipelineGraph;
use crossbeam_channel::{Receiver, Sender};
use tracing::info;

/// A unified message type for the pipeline runtime.
/// This enum encapsulates both data packets and control commands,
/// allowing them to be sent through a single channel.
pub enum RuntimeMsg {
    Data(Packet),
    Ctrl(ControlCommand),
}

/// The synchronous pipeline execution loop.
///
/// This function takes ownership of the pipeline graph and runs a polling loop
/// to process incoming messages. It is designed to be executed in its own thread.
///
/// # Arguments
/// * `rx` - The receiver for `RuntimeMsg`.
/// * `event_tx` - The sender for `PipelineEvent`.
/// * `graph` - The `PipelineGraph` to execute.
///
/// # Returns
/// `Ok(())` on graceful shutdown, or a `PipelineError` if something goes wrong.
pub fn run(
    runtime_rx: Receiver<RuntimeMsg>,
    event_tx: Sender<PipelineEvent>,
    mut graph: PipelineGraph,
) -> Result<(), PipelineError> {
    graph.context.event_tx = event_tx.clone();
    let mut topo = graph.topology_sort(); // Pre-compute the execution order.

    // The main loop now blocks indefinitely on recv, which is what we want.
    // The responsibility of selecting between multiple event sources is now
    // in the main daemon loop.
    for msg in runtime_rx {
        match msg {
            RuntimeMsg::Ctrl(ControlCommand::Shutdown) => {
                info!("Shutdown command received, starting graceful drain...");
                break; // Exit loop to start draining
            }
            RuntimeMsg::Ctrl(cmd) => {
                info!("Received control command: {:?}", cmd);
                graph.handle_control_command(&cmd)?;
                if graph.topo_dirty {
                    info!("Topology may have changed, re-computing...");
                    topo = graph.topology_sort();
                    graph.topo_dirty = false;
                }
            }
            RuntimeMsg::Data(pkt) => {
                graph.push(pkt, &topo)?;
            }
        }
    }

    // --- Graceful Shutdown ---
    info!("Draining complete. Flushing sinks...");
    graph.flush()?; // Use the Drains trait
    info!("Sinks flushed. Sending ShutdownAck.");
    event_tx
        .send(PipelineEvent::ShutdownAck)
        .map_err(|e| PipelineError::SendError(e.to_string()))?;

    info!("Pipeline has shut down gracefully.");
    Ok(())
}