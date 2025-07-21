//! The multi-threaded pipeline executor.

use crate::data::RtPacket;
use crate::graph::{PipelineGraph, StageId, StageMode};
use crate::stage::{DefaultPolicy, ErrorAction, Stage, StagePolicy, StageState};
use flume::{Receiver, Selector, Sender};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread;
use tracing::{error, info, warn};

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
    handles: HashMap<StageId, StageHandle>,
    graph: PipelineGraph,
    stop_txs: Vec<Sender<()>>,
}

impl Executor {
    /// Creates a new executor from a pipeline graph.
    pub fn new(graph: PipelineGraph) -> (Self, Sender<Arc<RtPacket>>) {
        let (input_tx, input_rx) = flume::unbounded();

        let mut executor = Self {
            handles: HashMap::new(),
            graph,
            stop_txs: Vec::new(),
        };

        executor.wire_and_start(input_rx);
        (executor, input_tx)
    }

    /// Wires the graph and starts the threads.
    fn wire_and_start(&mut self, source_rx: Receiver<Arc<RtPacket>>) {
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
            self.stop_txs.push(stop_tx);
            let _stage_config = self.graph.config.stages.iter().find(|s| &s.name == stage_id).unwrap().clone();
            let node = self.graph.nodes.remove(stage_id).unwrap();
            let context = self.graph.context.clone();
            let mode = node.mode;
            let producer_rx = node.producer_rx;

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

            let node_clone = node.clone();
            let thread_name = node.lock().unwrap().name.clone();
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
                                match input_rx.recv() {
                                    Ok(packet) => {
                                        for tx in &output_txs {
                                            if tx.send(packet.clone()).is_err() {
                                                // Downstream disconnected
                                            }
                                        }
                                    }
                                    Err(_) => {
                                        // Producer has shut down
                                        break;
                                    }
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
                                            match node_guard.stage.process(packet, &mut context_clone)
                                            {
                                                Ok(Some(output_packet)) => {
                                                    for tx in &output_txs {
                                                        if tx.send(output_packet.clone()).is_err()
                                                        {
                                                            // Downstream has disconnected, this is fine.
                                                        }
                                                    }
                                                }
                                                Ok(None) => { /* Packet was filtered */ }
                                                Err(e) => {
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
                                                            node_guard.state = StageState::Draining;
                                                        }
                                                        ErrorAction::SkipPacket => {
                                                            warn!(
                                                            "Skipping packet in stage '{}' due to error.",
                                                            node_guard.name
                                                        );
                                                        }
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

            self.handles.insert(
                stage_id.clone(),
                StageHandle { thread_handle },
            );
        }
    }

    /// Stops the executor, shutting down all stage threads.
    pub fn stop(mut self) {
        info!("Stopping multi-threaded executor...");
        for tx in self.stop_txs.drain(..) {
            let _ = tx.send(());
        }
        for (_name, handle) in self.handles.drain() {
            let _ = handle.thread_handle.join();
        }
        info!("All stage threads joined.");
    }
}