//! Pipeline runtime for executing pipeline graphs.

use crate::control::{ControlCommand, PipelineEvent};
use crate::error::PipelineError;
use crate::graph::PipelineGraph;
use std::any::Any;
use std::sync::mpsc::{Receiver, Sender, RecvTimeoutError, TryRecvError};
use std::time::Duration;
use tracing::info;

/// A unified message type for the pipeline runtime.
/// This enum encapsulates both data packets and control commands,
/// allowing them to be sent through a single channel.
pub enum RuntimeMsg {
    Data(Box<dyn Any + Send>),
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
    let mut draining = false;

    loop {
        // Block on the external channel with a timeout
        match runtime_rx.recv_timeout(Duration::from_millis(2)) {
            Ok(RuntimeMsg::Ctrl(ControlCommand::Shutdown)) => {
                info!("Shutdown command received, starting graceful drain...");
                draining = true;
            }
            Ok(RuntimeMsg::Ctrl(cmd)) => {
                info!("Received control command: {:?}", cmd);
                graph.handle_control_command(&cmd)?;
                if graph.topo_dirty {
                    info!("Topology may have changed, re-computing...");
                    topo = graph.topology_sort();
                    graph.topo_dirty = false;
                }
            }
            Ok(RuntimeMsg::Data(pkt)) => {
                if !draining {
                    graph.push(pkt, &topo)?;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // No message, continue.
            }
            Err(RecvTimeoutError::Disconnected) => {
                info!("All senders disconnected, initiating shutdown.");
                draining = true;
            }
        }

        // Since the internal event channel is removed, we no longer poll it.
        // All events are now sent directly to the external `event_tx`.

        // Graceful shutdown logic
        if draining {
            if let Err(TryRecvError::Empty) = runtime_rx.try_recv() {
                if graph.is_idle() {
                    info!("Draining complete. Flushing sinks...");
                    graph.flush()?; // Use the Drains trait
                    info!("Sinks flushed. Sending ShutdownAck.");
                    event_tx
                        .send(PipelineEvent::ShutdownAck)
                        .map_err(|e| PipelineError::SendError(e.to_string()))?;
                    break; // Exit the loop
                }
            }
        }

        // The loop is now blocked on recv_timeout, so this sleep is no longer needed.
    }

    info!("Pipeline has shut down gracefully.");
    Ok(())
}