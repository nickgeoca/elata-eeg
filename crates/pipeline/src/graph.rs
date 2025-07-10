//! Pipeline graph construction and management

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::{PipelineConfig, StageConfig, MemoryPoolConfig, ConnectionConfig};
use crate::stage::{StageInstance, StageState, DataPlaneStageRegistry};
use crate::error::{PipelineError, PipelineResult};
use crate::data::{MemoryPool, AnyPacketType, RawEegPacket, VoltageEegPacket};

/// Pipeline graph representing the dataflow structure
#[derive(Debug)]
pub struct PipelineGraph {
    /// Graph ID
    pub id: Uuid,
    /// Graph name
    pub name: String,
    /// Stage instances in the graph
    pub stages: HashMap<String, StageInstance>,
    /// Memory pools for the data plane
    pub memory_pools: HashMap<String, MemoryPoolConfig>,
    /// Explicit connections between stages
    pub connections: Vec<ConnectionConfig>,
    /// Adjacency list representing data flow edges
    pub edges: HashMap<String, Vec<String>>,
    /// Reverse adjacency list for dependency tracking
    pub reverse_edges: HashMap<String, Vec<String>>,
    /// Source stages (no inputs)
    pub sources: HashSet<String>,
    /// Sink stages (no outputs)
    pub sinks: HashSet<String>,
    /// Graph state
    pub state: GraphState,
}

/// Graph runtime state
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum GraphState {
    /// Graph is constructed but not running
    Idle,
    /// Graph is starting up
    Starting,
    /// Graph is running and processing data
    Running,
    /// Graph is stopping
    Stopping,
    /// Graph has encountered an error
    Error(String),
}

/// Channel connection between stages
#[derive(Debug)]
pub struct StageConnection {
    /// Source stage name
    pub from: String,
    /// Destination stage name
    pub to: String,
    /// Channel capacity (None for unbounded)
    pub capacity: Option<usize>,
}

/// Graph builder for constructing pipeline graphs from configuration
pub struct GraphBuilder {
    data_plane_registry: Arc<DataPlaneStageRegistry>,
}

impl GraphBuilder {
    /// Create a new graph builder with data plane registry
    pub fn new(data_plane_registry: Arc<DataPlaneStageRegistry>) -> Self {
        Self {
            data_plane_registry,
        }
    }

    /// Build a pipeline graph from configuration
    pub async fn build(&self, config: &PipelineConfig) -> PipelineResult<PipelineGraph> {
        // Validate configuration first
        config.validate()?;

        let mut graph = PipelineGraph::new(config.metadata.name.clone());

        // Create memory pools first
        graph.create_memory_pools(&config.memory_pools)?;

        // Create stage instances using the data plane registry
        for stage_config in &config.stages {
            if stage_config.enabled {
                let instance = self.create_stage_instance(stage_config).await?;
                graph.add_stage(instance)?;
            }
        }

        // Build edges from explicit connections or legacy inputs
        if !config.connections.is_empty() {
            // Use explicit connections
            for connection in &config.connections {
                graph.add_connection(connection.clone())?;
            }
        } else {
            // Fall back to legacy input-based connections
            for stage_config in &config.stages {
                if stage_config.enabled {
                    for input in &stage_config.inputs {
                        graph.add_edge(input.clone(), stage_config.name.clone())?;
                    }
                }
            }
        }

        // Identify sources and sinks
        graph.identify_sources_and_sinks();

        Ok(graph)
    }

    /// Create a stage instance from configuration
    async fn create_stage_instance(&self, config: &StageConfig) -> PipelineResult<StageInstance> {
        // Check if the stage type is registered in the data plane registry
        if self.data_plane_registry.stage_types().contains(&config.stage_type.as_str()) {
            // This is a data plane stage - create instance for metadata tracking
            let instance = StageInstance::new(
                config.name.clone(),
                config.stage_type.clone(),
                config.params.clone(),
                config.inputs.clone(),
            );
            return Ok(instance);
        }

        // Stage type not found in registry
        Err(PipelineError::UnknownStageType {
            stage_type: config.stage_type.clone(),
        })
    }
}

impl PipelineGraph {
    /// Create a new empty pipeline graph
    pub fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            stages: HashMap::new(),
            memory_pools: HashMap::new(),
            connections: Vec::new(),
            edges: HashMap::new(),
            reverse_edges: HashMap::new(),
            sources: HashSet::new(),
            sinks: HashSet::new(),
            state: GraphState::Idle,
        }
    }

    /// Add a stage to the graph
    pub fn add_stage(&mut self, stage: StageInstance) -> PipelineResult<()> {
        if self.stages.contains_key(&stage.name) {
            return Err(PipelineError::InvalidConfiguration {
                message: format!("Stage '{}' already exists in graph", stage.name),
            });
        }

        self.stages.insert(stage.name.clone(), stage);
        Ok(())
    }

    /// Remove a stage from the graph
    pub fn remove_stage(&mut self, name: &str) -> PipelineResult<()> {
        if !self.stages.contains_key(name) {
            return Err(PipelineError::StageNotFound {
                name: name.to_string(),
            });
        }

        // Remove all edges involving this stage
        self.edges.remove(name);
        self.reverse_edges.remove(name);

        // Remove from other stages' edge lists
        for edges in self.edges.values_mut() {
            edges.retain(|target| target != name);
        }
        for edges in self.reverse_edges.values_mut() {
            edges.retain(|source| source != name);
        }

        // Remove from sources and sinks
        self.sources.remove(name);
        self.sinks.remove(name);

        // Remove the stage
        self.stages.remove(name);

        Ok(())
    }

    /// Add an edge between two stages
    pub fn add_edge(&mut self, from: String, to: String) -> PipelineResult<()> {
        // Verify both stages exist
        if !self.stages.contains_key(&from) {
            return Err(PipelineError::StageNotFound { name: from });
        }
        if !self.stages.contains_key(&to) {
            return Err(PipelineError::StageNotFound { name: to });
        }

        // Add to forward edges
        self.edges.entry(from.clone()).or_insert_with(Vec::new).push(to.clone());

        // Add to reverse edges
        self.reverse_edges.entry(to).or_insert_with(Vec::new).push(from);

        Ok(())
    }

    /// Remove an edge between two stages
    pub fn remove_edge(&mut self, from: &str, to: &str) -> PipelineResult<()> {
        if let Some(edges) = self.edges.get_mut(from) {
            edges.retain(|target| target != to);
        }

        if let Some(edges) = self.reverse_edges.get_mut(to) {
            edges.retain(|source| source != from);
        }

        Ok(())
    }

    /// Get stages in topological order
    pub fn topological_order(&self) -> PipelineResult<Vec<&str>> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut temp_mark = HashSet::new();

        for stage_name in self.stages.keys() {
            if !visited.contains(stage_name) {
                self.topological_visit(stage_name, &mut result, &mut visited, &mut temp_mark)?;
            }
        }

        result.reverse();
        Ok(result)
    }

    /// Recursive helper for topological sort
    fn topological_visit<'a>(
        &'a self,
        stage_name: &'a str,
        result: &mut Vec<&'a str>,
        visited: &mut HashSet<String>,
        temp_mark: &mut HashSet<String>,
    ) -> PipelineResult<()> {
        if temp_mark.contains(stage_name) {
            return Err(PipelineError::CircularDependency);
        }

        if !visited.contains(stage_name) {
            temp_mark.insert(stage_name.to_string());

            if let Some(inputs) = self.reverse_edges.get(stage_name) {
                for input in inputs {
                    self.topological_visit(input, result, visited, temp_mark)?;
                }
            }

            temp_mark.remove(stage_name);
            visited.insert(stage_name.to_string());
            result.push(stage_name);
        }

        Ok(())
    }

    /// Identify source and sink stages
    pub fn identify_sources_and_sinks(&mut self) {
        self.sources.clear();
        self.sinks.clear();

        for stage_name in self.stages.keys() {
            // Source: no inputs
            if self.reverse_edges.get(stage_name).map_or(true, |inputs| inputs.is_empty()) {
                self.sources.insert(stage_name.clone());
            }

            // Sink: no outputs
            if self.edges.get(stage_name).map_or(true, |outputs| outputs.is_empty()) {
                self.sinks.insert(stage_name.clone());
            }
        }
    }

    /// Get all stages that depend on the given stage
    pub fn get_dependents(&self, stage_name: &str) -> Vec<&str> {
        self.edges.get(stage_name)
            .map(|deps| deps.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    /// Get all stages that the given stage depends on
    pub fn get_dependencies(&self, stage_name: &str) -> Vec<&str> {
        self.reverse_edges.get(stage_name)
            .map(|deps| deps.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    /// Check if the graph has cycles
    pub fn has_cycles(&self) -> bool {
        self.topological_order().is_err()
    }

    /// Get graph statistics
    pub fn stats(&self) -> GraphStats {
        GraphStats {
            stage_count: self.stages.len(),
            edge_count: self.edges.values().map(|v| v.len()).sum(),
            source_count: self.sources.len(),
            sink_count: self.sinks.len(),
            has_cycles: self.has_cycles(),
        }
    }

    /// Lock all stages in the graph
    pub fn lock_all_stages(&mut self) {
        for stage in self.stages.values_mut() {
            stage.lock();
        }
    }

    /// Unlock all stages in the graph
    pub fn unlock_all_stages(&mut self) {
        for stage in self.stages.values_mut() {
            stage.unlock();
        }
    }

    /// Set the state of all stages
    pub fn set_all_stage_states(&mut self, state: StageState) {
        for stage in self.stages.values_mut() {
            stage.set_state(state.clone());
        }
    }

    /// Get stage by name
    pub fn get_stage(&self, name: &str) -> Option<&StageInstance> {
        self.stages.get(name)
    }

    /// Get mutable stage by name
    pub fn get_stage_mut(&mut self, name: &str) -> Option<&mut StageInstance> {
        self.stages.get_mut(name)
    }

    /// Set graph state
    pub fn set_state(&mut self, state: GraphState) {
        self.state = state;
    }

    /// Create memory pools from configuration
    pub fn create_memory_pools(&mut self, pool_configs: &[MemoryPoolConfig]) -> PipelineResult<()> {
        for pool_config in pool_configs {
            if self.memory_pools.contains_key(&pool_config.name) {
                return Err(PipelineError::InvalidConfiguration {
                    message: format!("Memory pool '{}' already exists", pool_config.name),
                });
            }
            self.memory_pools.insert(pool_config.name.clone(), pool_config.clone());
        }
        Ok(())
    }

    /// Add a connection between stages
    pub fn add_connection(&mut self, connection: ConnectionConfig) -> PipelineResult<()> {
        // Verify both stages exist
        if !self.stages.contains_key(&connection.from) {
            return Err(PipelineError::StageNotFound { name: connection.from.clone() });
        }
        if !self.stages.contains_key(&connection.to) {
            return Err(PipelineError::StageNotFound { name: connection.to.clone() });
        }

        // Add to connections list
        self.connections.push(connection.clone());

        // Also add to legacy edges for compatibility
        self.edges.entry(connection.from.clone()).or_insert_with(Vec::new).push(connection.to.clone());
        self.reverse_edges.entry(connection.to).or_insert_with(Vec::new).push(connection.from);

        Ok(())
    }

    /// Get memory pool configuration by name
    pub fn get_memory_pool(&self, name: &str) -> Option<&MemoryPoolConfig> {
        self.memory_pools.get(name)
    }

    /// Get all connections for a stage (outgoing)
    pub fn get_stage_connections(&self, stage_name: &str) -> Vec<&ConnectionConfig> {
        self.connections.iter().filter(|c| c.from == stage_name).collect()
    }

    /// Get all incoming connections for a stage
    pub fn get_stage_inputs(&self, stage_name: &str) -> Vec<&ConnectionConfig> {
        self.connections.iter().filter(|c| c.to == stage_name).collect()
    }
}

/// Graph statistics
#[derive(Debug, Clone)]
pub struct GraphStats {
    /// Number of stages in the graph
    pub stage_count: usize,
    /// Number of edges in the graph
    pub edge_count: usize,
    /// Number of source stages
    pub source_count: usize,
    /// Number of sink stages
    pub sink_count: usize,
    /// Whether the graph has cycles
    pub has_cycles: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stage::StageParams;

    fn create_test_stage(name: &str, stage_type: &str, inputs: Vec<String>) -> StageInstance {
        StageInstance::new(
            name.to_string(),
            stage_type.to_string(),
            StageParams::new(),
            inputs,
        )
    }

    #[test]
    fn test_graph_creation() {
        let graph = PipelineGraph::new("test_graph".to_string());
        assert_eq!(graph.name, "test_graph");
        assert_eq!(graph.stages.len(), 0);
        assert_eq!(graph.state, GraphState::Idle);
    }

    #[test]
    fn test_stage_addition() {
        let mut graph = PipelineGraph::new("test".to_string());
        let stage = create_test_stage("stage1", "test_type", vec![]);

        assert!(graph.add_stage(stage).is_ok());
        assert_eq!(graph.stages.len(), 1);
        assert!(graph.stages.contains_key("stage1"));
    }

    #[test]
    fn test_edge_addition() {
        let mut graph = PipelineGraph::new("test".to_string());
        
        let stage1 = create_test_stage("stage1", "test", vec![]);
        let stage2 = create_test_stage("stage2", "test", vec!["stage1".to_string()]);

        graph.add_stage(stage1).unwrap();
        graph.add_stage(stage2).unwrap();
        graph.add_edge("stage1".to_string(), "stage2".to_string()).unwrap();

        assert_eq!(graph.edges.get("stage1").unwrap(), &vec!["stage2"]);
        assert_eq!(graph.reverse_edges.get("stage2").unwrap(), &vec!["stage1"]);
    }

    #[test]
    fn test_sources_and_sinks_identification() {
        let mut graph = PipelineGraph::new("test".to_string());
        
        let stage1 = create_test_stage("stage1", "test", vec![]);
        let stage2 = create_test_stage("stage2", "test", vec!["stage1".to_string()]);
        let stage3 = create_test_stage("stage3", "test", vec!["stage2".to_string()]);

        graph.add_stage(stage1).unwrap();
        graph.add_stage(stage2).unwrap();
        graph.add_stage(stage3).unwrap();
        graph.add_edge("stage1".to_string(), "stage2".to_string()).unwrap();
        graph.add_edge("stage2".to_string(), "stage3".to_string()).unwrap();

        graph.identify_sources_and_sinks();

        assert!(graph.sources.contains("stage1"));
        assert!(graph.sinks.contains("stage3"));
        assert_eq!(graph.sources.len(), 1);
        assert_eq!(graph.sinks.len(), 1);
    }

    #[test]
    fn test_topological_order() {
        let mut graph = PipelineGraph::new("test".to_string());
        
        let stage1 = create_test_stage("stage1", "test", vec![]);
        let stage2 = create_test_stage("stage2", "test", vec!["stage1".to_string()]);
        let stage3 = create_test_stage("stage3", "test", vec!["stage2".to_string()]);

        graph.add_stage(stage1).unwrap();
        graph.add_stage(stage2).unwrap();
        graph.add_stage(stage3).unwrap();
        graph.add_edge("stage1".to_string(), "stage2".to_string()).unwrap();
        graph.add_edge("stage2".to_string(), "stage3".to_string()).unwrap();

        let order = graph.topological_order().unwrap();
        assert_eq!(order, vec!["stage1", "stage2", "stage3"]);
    }
}