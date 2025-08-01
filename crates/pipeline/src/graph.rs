//! Pipeline graph construction and management.

use crate::allocator::{PacketAllocator, RecycledI32Vec, SharedPacketAllocator};
use crate::config::{SystemConfig, StageConfig};
use crate::control::ControlCommand;
use crate::data::{PacketOwned, RtPacket};
use crate::error::{PipelineError, StageError};
use crate::registry::StageRegistry;
use crate::stage::{DefaultPolicy, Stage, StageContext, StagePolicy, StageState};
use eeg_types::comms::BrokerMessage;
use flume::{Receiver, Sender};
use petgraph::algo::toposort;
use petgraph::graph::DiGraph;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use sensors::types::AdcDriver;

pub type StageId = String;

/// Defines the operational mode of a stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageMode {
    /// A standard stage that processes packets from an input channel.
    Pull,
    /// A source stage that produces packets asynchronously.
    Producer,
}

/// Represents a node in the pipeline graph.
pub struct PipelineNode {
    pub name: StageId,
    pub stage: Box<dyn Stage>,
    pub input_source: Option<StageId>,
    pub state: StageState,
    pub policy: Box<dyn StagePolicy>,
    pub mode: StageMode,
    pub producer_rx: Option<Receiver<Arc<RtPacket>>>,
}

/// Represents the entire pipeline as a graph of connected stages.
pub struct PipelineGraph {
    pub nodes: HashMap<StageId, PipelineNode>,
    pub context: StageContext,
    pub allocator: SharedPacketAllocator,
    pub config: SystemConfig,
    pub topo_dirty: bool,
}

impl PipelineGraph {
    /// Builds a new `PipelineGraph` from a configuration and a stage registry.
    pub fn build(
        config: &SystemConfig,
        registry: &StageRegistry,
        event_tx: flume::Sender<crate::control::PipelineEvent>,
        allocator: Option<SharedPacketAllocator>,
        driver: &Option<Arc<Mutex<Box<dyn AdcDriver>>>>,
        websocket_sender: Option<Sender<BrokerMessage>>,
    ) -> Result<Self, StageError> {
        let mut nodes = HashMap::new();
        let allocator = allocator
            .unwrap_or_else(|| Arc::new(PacketAllocator::with_capacity(16, 16, 16, 1024)));

        for stage_config in &config.stages {
            if nodes.contains_key(&stage_config.name) {
                return Err(StageError::BadConfig(format!(
                    "Duplicate stage name found: {}",
                    stage_config.name
                )));
            }

            let sample_rate = config
                .stages
                .iter()
                .find(|s| s.stage_type == "eeg_source")
                .and_then(|s| s.params.get("driver"))
                .and_then(|d| d.get("sample_rate"))
                .and_then(|sr| sr.as_f64())
                .unwrap_or(250.0);

            let sample_rate = config
                .stages
                .iter()
                .find(|s| s.stage_type == "eeg_source")
                .and_then(|s| s.params.get("driver"))
                .and_then(|d| d.get("sample_rate"))
                .and_then(|sr| sr.as_f64())
                .unwrap_or(250.0);

            let init_ctx = crate::stage::StageInitCtx {
                event_tx: &event_tx,
                allocator: &allocator,
                driver,
                sample_rate,
                websocket_sender: websocket_sender.clone(),
            };

            let (stage, producer_rx) = registry.create_stage(stage_config, &init_ctx)?;
            let mode = if producer_rx.is_some() {
                StageMode::Producer
            } else {
                StageMode::Pull
            };

            if stage_config.inputs.len() > 1 {
                return Err(StageError::BadConfig(format!(
                    "Stage '{}' has more than one input, which is not currently supported.",
                    stage_config.name
                )));
            }
            let input_source = stage_config
                .inputs
                .first()
                .map(|s| s.split('.').next().unwrap_or(s).to_string());

            let node = PipelineNode {
                name: stage_config.name.clone(),
                stage,
                input_source,
                state: StageState::Running,
                policy: Box::new(DefaultPolicy),
                mode,
                producer_rx,
            };
            nodes.insert(stage_config.name.clone(), node);
        }

        // Create a map of all available outputs for validation.
        let mut available_outputs = HashMap::new();
        for stage_config in &config.stages {
            for output_name in &stage_config.outputs {
                available_outputs.insert(
                    format!("{}.{}", stage_config.name, output_name),
                    stage_config.name.clone(),
                );
            }
            // For stages without explicit outputs, assume a default output stream.
            if stage_config.outputs.is_empty() {
                available_outputs.insert(stage_config.name.clone(), stage_config.name.clone());
            }
        }

        // Validate that all input sources exist.
        for stage_config in &config.stages {
            for input_name in &stage_config.inputs {
                if !available_outputs.contains_key(input_name) {
                    return Err(StageError::BadConfig(format!(
                        "Stage '{}' references an unknown input '{}'",
                        stage_config.name, input_name
                    )));
                }
            }
        }

        let context = StageContext::new(event_tx, allocator.clone());

        Ok(Self {
            nodes,
            context,
            allocator,
            config: config.clone(),
            topo_dirty: false,
        })
    }

    pub fn get_input_for_stage(&self, stage_id: &StageId) -> Option<StageId> {
        self.config
            .stages
            .iter()
            .find(|s| &s.name == stage_id)
            .and_then(|s| s.inputs.first().cloned())
    }

    /// Computes a topological sort of the stage graph for execution order.
    /// NOTE: This requires the `petgraph` crate.
    pub fn topology_sort(&self) -> Vec<StageId> {
        let mut graph = DiGraph::<&StageConfig, ()>::new();
        let mut node_map = HashMap::new();

        for stage_config in &self.config.stages {
            let idx = graph.add_node(stage_config);
            node_map.insert(&stage_config.name, idx);
        }

        for stage_config in &self.config.stages {
            if let Some(input_name) = stage_config.inputs.first() {
                if let (Some(&from_idx), Some(&to_idx)) =
                    (node_map.get(input_name), node_map.get(&stage_config.name))
                {
                    graph.add_edge(from_idx, to_idx, ());
                }
            }
        }

        match toposort(&graph, None) {
            Ok(nodes) => nodes
                .into_iter()
                .map(|idx| graph[idx].name.clone())
                .collect(),
            Err(cycle) => {
                let cycle_node_name = &graph[cycle.node_id()].name;
                panic!("Pipeline has a cycle involving stage: {}", cycle_node_name);
            }
        }
    }

    /// Pushes a data packet through the pipeline using the pre-computed topological order.
    pub fn push(&mut self, pkt: PacketOwned, topo: &[StageId]) -> Result<(), PipelineError> {
        let mut outputs: HashMap<StageId, Option<Arc<RtPacket>>> = HashMap::new();
        static mut PACKET_COUNT: u64 = 0;

        // Find the source stage (the one with no inputs) and insert the initial packet.
        let source_stage_name = self
            .config
            .stages
            .iter()
            .find(|s| s.inputs.is_empty())
            .map(|s| s.name.clone())
            .or_else(|| topo.first().cloned())
            .ok_or(PipelineError::InvalidConfiguration {
                message: "No source stage found in the pipeline".to_string(),
            })?;

        let runtime_packet = match pkt {
            PacketOwned::RawI32(data) => {
                let mut initial_packet = RecycledI32Vec::new(self.allocator.clone());
                initial_packet.extend(data.samples.iter());
                let packet_data = crate::data::PacketData {
                    header: data.header,
                    samples: initial_packet,
                };
                RtPacket::RawI32(packet_data)
            }
            PacketOwned::Voltage(data) => {
                // This is a fallback for now.
                let mut initial_packet = crate::allocator::RecycledF32Vec::new(self.allocator.clone());
                initial_packet.extend(data.samples.iter());
                let packet_data = crate::data::PacketData {
                    header: data.header,
                    samples: initial_packet,
                };
                RtPacket::Voltage(packet_data)
            }
            PacketOwned::RawAndVoltage(data) => {
                // This is a fallback for now.
                let mut initial_packet =
                    crate::allocator::RecycledI32F32TupleVec::new(self.allocator.clone());
                initial_packet.extend(data.samples.iter());
                let packet_data = crate::data::PacketData {
                    header: data.header,
                    samples: initial_packet,
                };
                RtPacket::RawAndVoltage(packet_data)
            }
        };

        outputs.insert(source_stage_name, Some(Arc::new(runtime_packet)));

        for stage_id in topo {
            let input_name = self.get_input_for_stage(stage_id);
            if let Some(node) = self.nodes.get_mut(stage_id) {
                // Determine the input for the current stage.
                let input_packet = if let Some(input_name) = input_name {
                    // This stage takes input from a predecessor.
                    // We clone the Arc, which is a cheap reference count bump.
                    outputs.get(&input_name).and_then(|p| p.clone())
                } else {
                    // This is a source stage, its input comes from the initial push.
                    outputs.get(stage_id).and_then(|p| p.clone())
                };

                // If there's a packet to process, run the stage.
                if let Some(packet) = input_packet {
                    if node.state == StageState::Halted {
                        continue; // Skip halted stages
                    }

                    let result = node.stage.process(packet, &mut self.context);
                    match result {
                        Ok(output) => {
                            if let Some(ref packet) = output {
                                let source_id = match &**packet {
                                    RtPacket::RawI32(d) => &d.header.source_id,
                                    RtPacket::Voltage(d) => &d.header.source_id,
                                    RtPacket::RawAndVoltage(d) => &d.header.source_id,
                                };
                                outputs.insert(source_id.clone(), Some(packet.clone()));
                            }
                            outputs.insert(stage_id.clone(), output);
                        }
                        Err(e) => {
                            let action = node.policy.on_error();
                            // Emit an ErrorOccurred event
                            if let Err(e) = self.context.event_tx.send(crate::control::PipelineEvent::ErrorOccurred {
                                stage_id: node.name.clone(),
                                error_message: e.to_string(),
                            }) {
                                tracing::error!("Failed to send ErrorOccurred event: {}", e);
                            }
                            
                            match action {
                                crate::stage::ErrorAction::Fatal => return Err(e.into()),
                                crate::stage::ErrorAction::SkipPacket => {
                                    // TODO: Add logging
                                    continue;
                                }
                                crate::stage::ErrorAction::DrainThenStop => {
                                    node.state = StageState::Draining;
                                    // In a single-threaded model, we can just halt immediately
                                    // after the current push finishes.
                                }
                            }
                        }
                    }
                }
            }
        }

        // After processing, transition Draining stages to Halted.
        // In a multi-threaded executor, this logic would be more complex.
        for node in self.nodes.values_mut() {
            if node.state == StageState::Draining {
                node.state = StageState::Halted;
            }
        }

        // Increment packet count and emit DataFlowing event every 1000 packets
        unsafe {
            PACKET_COUNT += 1;
            if PACKET_COUNT % 1000 == 0 {
                if let Err(e) = self.context.event_tx.send(crate::control::PipelineEvent::DataFlowing {
                    packet_count: PACKET_COUNT,
                }) {
                    tracing::error!("Failed to send DataFlowing event: {}", e);
                }
            }
        }
        
        Ok(())
    }

    /// Forwards a control command to all stages in the graph.
    pub fn handle_control_command(&mut self, cmd: &ControlCommand) -> Result<(), PipelineError> {
        match cmd {
            ControlCommand::Reconfigure(new_config) => {
                self.topo_dirty = true;
                self.config = new_config.clone();

                for stage_config in &new_config.stages {
                    if let Some(node) = self.nodes.get_mut(&stage_config.name) {
                        // This is a simplified diff. A real implementation would be more robust.
                        let params_value = serde_json::to_value(&stage_config.params)?;
                        node.stage
                            .reconfigure(&params_value, &mut self.context)?;
                    }
                }
                
                // Emit a ConfigUpdated event
                if let Err(e) = self.context.event_tx.send(crate::control::PipelineEvent::ConfigUpdated {
                    config: new_config.clone(),
                }) {
                    tracing::error!("Failed to send ConfigUpdated event: {}", e);
                }
            }
            ControlCommand::SetParameter { target_stage, parameters } => {
                // Forward the command to the target stage
                if let Some(node) = self.nodes.get_mut(target_stage) {
                    node.stage.control(cmd, &mut self.context)?;
                    
                    // Emit a ParameterChanged event
                    for (param_id, value) in parameters.as_object().unwrap_or(&serde_json::Map::new()) {
                        if let Err(e) = self.context.event_tx.send(crate::control::PipelineEvent::ParameterChanged {
                            stage_id: target_stage.clone(),
                            parameter_id: param_id.clone(),
                            value: value.clone(),
                        }) {
                            tracing::error!("Failed to send ParameterChanged event: {}", e);
                        }
                    }
                }
            }
            ControlCommand::Start => {
                for node in self.nodes.values_mut() {
                    node.stage.control(cmd, &mut self.context)?;
                    
                    // Emit a StageStarted event
                    if let Err(e) = self.context.event_tx.send(crate::control::PipelineEvent::StageStarted {
                        stage_id: node.name.clone(),
                    }) {
                        tracing::error!("Failed to send StageStarted event: {}", e);
                    }
                }
            }
            ControlCommand::Shutdown => {
                for node in self.nodes.values_mut() {
                    node.stage.control(cmd, &mut self.context)?;
                    
                    // Emit a StageStopped event
                    if let Err(e) = self.context.event_tx.send(crate::control::PipelineEvent::StageStopped {
                        stage_id: node.name.clone(),
                    }) {
                        tracing::error!("Failed to send StageStopped event: {}", e);
                    }
                }
            }
            _ => {
                for node in self.nodes.values_mut() {
                    node.stage.control(cmd, &mut self.context)?;
                }
            }
        }
        Ok(())
    }

    /// Checks if the pipeline is idle. In a synchronous model, it's always idle
    /// between `push` calls.
    pub fn is_idle(&self) -> bool {
        true
    }

    /// Flushes all sink stages that implement the `Drains` trait.
    /// TODO: This requires a mechanism to downcast `Stage` to `Drains`.
    pub fn flush(&mut self) -> Result<(), PipelineError> {
        for node in self.nodes.values_mut() {
            if let Some(drains) = node.stage.as_drains() {
                drains.flush().map_err(|e| PipelineError::RuntimeError {
                    stage_name: node.name.clone(),
                    message: format!("IO error during flush: {}", e),
                })?;
            }
        }
        Ok(())
    }

    pub fn get_current_config(&self) -> SystemConfig {
        self.config.clone()
    }
}