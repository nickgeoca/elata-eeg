//! The multi-threaded pipeline executor.

use crate::data::RtPacket;
use crate::graph::{PipelineGraph, StageId, StageMode};
use crate::stage::{DefaultPolicy, ErrorAction, Stage, StagePolicy, StageState};
use flume::{Receiver, Selector, Sender};
use std::collections::HashMap;
use std::any::Any;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread;
use tracing::{error, info, warn};
use crate::control::ControlCommand;
use crate::error::PipelineError;

/// A handle to a running stage in the executor.
/// A handle to a running stage in the executor.
struct StageHandle {
    thread_handle: thread::JoinHandle<()>,
}

/// An internal representation of a stage within the executor.
struct Node {
    stage: Box<dyn Stage>,
    name: String,
    state: StageState,
    policy: Box<dyn StagePolicy>,
    mode: StageMode,
    producer_rx: Option<Receiver<Arc<RtPacket>>>,
}

/// The main executor for the pipeline.
///
/// This struct manages the thread pool and the communication channels between stages.
pub struct Executor {
    handles: Arc<Mutex<HashMap<StageId, StageHandle>>>,
    graph: PipelineGraph,
    stop_txs: Arc<Mutex<Vec<Sender<()>>>>,
    fatal_error_tx: Sender<Box<dyn Any + Send>>,
}

impl Executor {
    /// Creates a new executor from a pipeline graph.
    pub fn new(
        graph: PipelineGraph,
    ) -> (
        Self,
        Sender<Arc<RtPacket>>,
        Receiver<Box<dyn Any + Send>>,
        Sender<ControlCommand>,
    ) {
        let (input_tx, input_rx) = flume::unbounded();
        let (fatal_error_tx, fatal_error_rx) = flume::unbounded();
        let (control_tx, control_rx) = flume::unbounded();

        let mut executor = Self {
            handles: Arc::new(Mutex::new(HashMap::new())),
            graph,
            stop_txs: Arc::new(Mutex::new(Vec::new())),
            fatal_error_tx,
        };

        executor.wire_and_start(input_rx, control_rx);
        (executor, input_tx, fatal_error_rx, control_tx)
    }

    /// Wires the graph and starts the threads.
    fn wire_and_start(&mut self, source_rx: Receiver<Arc<RtPacket>>, control_rx: Receiver<ControlCommand>) {
        let core_ids = core_affinity::get_core_ids().unwrap_or_default();
        let num_cores = core_ids.len();
        if num_cores < 4 {
            warn!("Not enough cores for optimal performance. Found {}.", num_cores);
        }

        let (acquire_cores, dsp_cores, sink_cores) = if num_cores >= 4 {
            (vec![core_ids[0]], vec![core_ids[1], core_ids[2]], vec![core_ids[3]])
        } else {
            (core_ids.clone(), core_ids.clone(), core_ids.clone())
        };


        let topo = self.graph.topology_sort();
        let mut rxs = HashMap::new();
        let mut txs = HashMap::new();

        // Create channels for all edges
        for stage_id in &topo {
            let (tx, rx) = flume::bounded(4); // Default back-pressure
            txs.insert(stage_id.clone(), tx);
            rxs.insert(stage_id.clone(), rx);
        }

        for stage_id in &topo {
            let cores = if stage_id.contains("acquire") {
                acquire_cores.clone()
            } else if stage_id.contains("sink") {
                sink_cores.clone()
            } else {
                dsp_cores.clone()
            };
            let (stop_tx, stop_rx) = flume::bounded(1);
            self.stop_txs.lock().unwrap().push(stop_tx);
            let _stage_config = self.graph.config.stages.iter().find(|s| &s.name == stage_id).unwrap().clone();
            let node = self.graph.nodes.remove(stage_id).unwrap();
            let context = self.graph.context.clone();
            let mode = node.mode;
            let producer_rx = node.producer_rx;
            let fatal_error_tx = self.fatal_error_tx.clone();

            let input_rx = if mode == StageMode::Producer {
                producer_rx.expect("Producer stage must have a receiver")
            } else if self
                .graph
                .config
                .stages
                .iter()
                .find(|s| &s.name == stage_id)
                .unwrap()
                .inputs
                .is_empty()
            {
                // This is a source node, fed by the executor's main input channel.
                let (tx, rx) = flume::bounded(4);
                let source_rx_clone = source_rx.clone();
                thread::spawn(move || {
                    while let Ok(pkt) = source_rx_clone.recv() {
                        if tx.send(pkt).is_err() {
                            break;
                        }
                    }
                });
                rx
            } else {
                // This is a non-source node. It receives from its own channel.
                rxs.remove(stage_id).unwrap()
            };

            let output_txs: Vec<_> = self
                .graph
                .config
                .stages
                .iter()
                .filter(|s| s.inputs.iter().any(|input| input.starts_with(stage_id)))
                .filter_map(|s| txs.get(&s.name).cloned())
                .collect();

            info!("Wiring stage '{}' to outputs: {:?}", stage_id, output_txs.iter().map(|_| "downstream").collect::<Vec<_>>());

            let node = Arc::new(Mutex::new(Node {
                stage: node.stage,
                name: stage_id.clone(),
                state: StageState::Running,
                policy: Box::new(DefaultPolicy), // TODO: Make this configurable
                mode,
                producer_rx: None, // The receiver is moved to the thread
            }));
            let context = Arc::new(Mutex::new(context));

            let thread_name = node.lock().unwrap().name.clone();
            let control_rx_clone = control_rx.clone();
            let thread_handle = thread::Builder::new()
                .name(thread_name)
                .spawn(move || {
                    if let Some(core) = cores.first() {
                        core_affinity::set_for_current(*core);
                    }
                    info!(
                        "Stage thread '{}' started on core {:?}.",
                        node.lock().unwrap().name,
                        cores.first()
                    );

                    match mode {
                        StageMode::Producer => {
                            info!(
                                "Starting producer loop for stage '{}'",
                                node.lock().unwrap().name
                            );
                            loop {
                                let should_halt = Arc::new(AtomicBool::new(false));
                                let should_halt_clone = should_halt.clone();
                                let node_clone = node.clone();
                                let mut context_clone = context.lock().unwrap();

                                Selector::new()
                                    .recv(&input_rx, |msg| {
                                        if let Ok(packet) = msg {
                                            for tx in &output_txs {
                                                if tx.send(packet.clone()).is_err() {
                                                    // Downstream disconnected, this is fine.
                                                }
                                            }
                                        } else {
                                            // Channel is disconnected
                                            should_halt_clone.store(true, Ordering::SeqCst);
                                        }
                                    })
                                    .recv(&stop_rx, move |_| {
                                        should_halt.store(true, Ordering::SeqCst);
                                    })
                                    .recv(&control_rx_clone, move |msg| {
                                        if let Ok(cmd) = msg {
                                            let mut node_guard = node_clone.lock().unwrap();
                                            if let Err(e) = node_guard.stage.control(&cmd, &mut context_clone) {
                                                error!("Error handling control command: {}", e);
                                            }
                                        }
                                    })
                                    .wait();

                                if should_halt_clone.load(Ordering::SeqCst) {
                                    break;
                                }
                            }
                        }
                        StageMode::Pull => {
                            info!(
                                "Starting pull loop for stage '{}'",
                                node.lock().unwrap().name
                            );
                            loop {
                                let mut node_guard = node.lock().unwrap();
                                if node_guard.state == StageState::Halted {
                                    break;
                                }

                                if node_guard.state == StageState::Draining && input_rx.is_empty()
                                {
                                    info!(
                                        "Stage '{}' has drained its queue and is halting.",
                                        node_guard.name
                                    );
                                    node_guard.state = StageState::Halted;
                                    continue;
                                }
                                drop(node_guard);

                                let should_halt = Arc::new(AtomicBool::new(false));

                                let should_halt_input = should_halt.clone();
                                let should_halt_stop = should_halt.clone();

                                let node_clone = node.clone();
                                let mut context_clone = context.lock().unwrap();

                                Selector::new()
                                    .recv(&input_rx, |msg| {
                                        let mut node_guard = node_clone.lock().unwrap();
                                        if let Ok(packet) = msg {
                                            if node_guard.state != StageState::Running {
                                                return; // Don't process new packets if not running
                                            }
                                            let result = std::panic::catch_unwind(
                                                std::panic::AssertUnwindSafe(|| {
                                                    node_guard
                                                        .stage
                                                        .process(packet, &mut context_clone)
                                                }),
                                            );

                                            match result {
                                                Ok(Ok(Some(output_packet))) => {
                                                    for tx in &output_txs {
                                                        if tx.send(output_packet.clone()).is_err()
                                                        {
                                                            // Downstream has disconnected, this is fine.
                                                        }
                                                    }
                                                }
                                                Ok(Ok(None)) => { /* Packet was filtered */ }
                                                Ok(Err(e)) => {
                                                    // Stage returned an error
                                                    error!(
                                                        "Error in stage '{}': {}",
                                                        node_guard.name, e
                                                    );
                                                    match node_guard.policy.on_error() {
                                                        ErrorAction::Fatal => {
                                                            error!(
                                                            "Fatal error in stage '{}'. Shutting down.",
                                                            node_guard.name
                                                        );
                                                            should_halt_input
                                                                .store(true, Ordering::SeqCst);
                                                        }
                                                        ErrorAction::DrainThenStop => {
                                                            warn!(
                                                            "Stage '{}' is draining due to an error.",
                                                            node_guard.name
                                                        );
                                                            node_guard.state =
                                                                StageState::Draining;
                                                        }
                                                        ErrorAction::SkipPacket => {
                                                            warn!(
                                                            "Skipping packet in stage '{}' due to error.",
                                                            node_guard.name
                                                        );
                                                        }
                                                    }
                                                }
                                                Err(panic_payload) => {
                                                    // Stage panicked
                                                    error!(
                                                        "PANIC in stage '{}'. Shutting down.",
                                                        node_guard.name
                                                    );
                                                    should_halt_input
                                                        .store(true, Ordering::SeqCst);
                                                    // Send the panic payload to the main thread
                                                    if fatal_error_tx.send(panic_payload).is_err() {
                                                        error!("Fatal error channel disconnected. Cannot report panic.");
                                                    }
                                                }
                                            }
                                        } else {
                                            // Channel is disconnected
                                            should_halt_input.store(true, Ordering::SeqCst);
                                        }
                                    })
                                    .recv(&stop_rx, move |_| {
                                        should_halt_stop.store(true, Ordering::SeqCst);
                                    })
                                    .wait();

                                if should_halt.load(Ordering::SeqCst) {
                                    node.lock().unwrap().state = StageState::Halted;
                                }
                            }
                        }
                    }
                    info!("Stage thread '{}' finished.", node.lock().unwrap().name);
                })
                .unwrap();

            self.handles.lock().unwrap().insert(
                stage_id.clone(),
                StageHandle { thread_handle },
            );
        }
    }

    /// Stops the executor, shutting down all stage threads.
    pub fn stop(self) {
        info!("Stopping multi-threaded executor...");
        for tx in self.stop_txs.lock().unwrap().iter() {
            let _ = tx.send(());
        }

        if let Ok(handles_mutex) = Arc::try_unwrap(self.handles) {
            let handles = handles_mutex.into_inner().unwrap();
            for (stage_id, handle) in handles {
                info!("Waiting for stage '{}' to shut down...", stage_id);
                if let Err(e) = handle.thread_handle.join() {
                    error!("Stage '{}' panicked during shutdown: {:?}", stage_id, e);
                }
            }
        } else {
            error!("Could not get exclusive access to stage handles for shutdown.");
        }
    }

    pub fn get_current_config(&self) -> crate::config::SystemConfig {
        self.graph.get_current_config()
    }

    /// Handles a control command for the pipeline.
    pub fn handle_control_command(&mut self, cmd: &ControlCommand) -> Result<(), PipelineError> {
        self.graph.handle_control_command(cmd)
    }
}