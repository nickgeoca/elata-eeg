//! Pipeline graph construction and management.

use crate::config::{SystemConfig, StageConfig};
use crate::control::ControlCommand;
use crate::error::{PipelineError, StageError};
use crate::registry::StageRegistry;
use crate::stage::{Stage, StageContext};
use crate::data::Packet;
use petgraph::algo::toposort;
use petgraph::graph::DiGraph;
use std::collections::HashMap;

pub type StageId = String;

/// Represents a node in the pipeline graph.
pub struct PipelineNode {
    pub name: StageId,
    pub stage: Box<dyn Stage>,
    pub input_source: Option<StageId>,
}

/// Represents the entire pipeline as a graph of connected stages.
pub struct PipelineGraph {
    pub nodes: HashMap<StageId, PipelineNode>,
    pub context: StageContext,
    config: SystemConfig,
    pub topo_dirty: bool,
}

impl PipelineGraph {
    /// Builds a new `PipelineGraph` from a configuration and a stage registry.
    pub fn build(
        config: &SystemConfig,
        registry: &StageRegistry,
        context: StageContext,
    ) -> Result<Self, StageError> {
        let mut nodes = HashMap::new();

        for stage_config in &config.stages {
            if nodes.contains_key(&stage_config.name) {
                return Err(StageError::BadConfig(format!(
                    "Duplicate stage name found: {}",
                    stage_config.name
                )));
            }

            let stage = registry.create_stage(stage_config)?;
            let input_source = stage_config.inputs.first().cloned();

            let node = PipelineNode {
                name: stage_config.name.clone(),
                stage,
                input_source,
            };
            nodes.insert(stage_config.name.clone(), node);
        }

        // Validate that all input sources exist.
        for stage_config in &config.stages {
            if let Some(source_name) = stage_config.inputs.first() {
                if !nodes.contains_key(source_name) {
                    return Err(StageError::BadConfig(format!(
                        "Stage '{}' references an unknown input '{}'",
                        stage_config.name, source_name
                    )));
                }
            }
        }

        Ok(Self {
            nodes,
            context,
            config: config.clone(),
            topo_dirty: false,
        })
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
    pub fn push(&mut self, pkt: Packet, topo: &[StageId]) -> Result<(), PipelineError> {
        let mut outputs: HashMap<StageId, Option<Packet>> = HashMap::new();

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
        outputs.insert(source_stage_name, Some(pkt));

        for stage_id in topo {
            if let Some(node) = self.nodes.get_mut(stage_id) {
                // Determine the input for the current stage.
                let input_packet = if let Some(source_id) = &node.input_source {
                    // This stage takes input from a predecessor.
                    outputs.remove(source_id).flatten()
                } else {
                    // This is a source stage, its input comes from the initial push.
                    outputs.remove(stage_id).flatten()
                };

                // If there's a packet to process, run the stage.
                if let Some(packet) = input_packet {
                    let result = node.stage.process(packet, &mut self.context)?;
                    outputs.insert(stage_id.clone(), result);
                }
            }
        }
        Ok(())
    }

    /// Forwards a control command to all stages in the graph.
    pub fn handle_control_command(&mut self, cmd: &ControlCommand) -> Result<(), PipelineError> {
        // If a command that can mutate the graph is received, mark the topology as dirty.
        if matches!(cmd, ControlCommand::Reconfigure(_)) {
            self.topo_dirty = true;
            // Note: Full reconfiguration logic would go here. For now, we just set the flag.
        }

        for node in self.nodes.values_mut() {
            node.stage.control(cmd, &mut self.context)?;
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
}