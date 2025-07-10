//! Pipeline configuration types and serialization

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::stage::StageParams;
use crate::error::{PipelineError, PipelineResult};

/// Complete pipeline configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Configuration format version
    pub version: String,
    /// Pipeline metadata
    pub metadata: PipelineMetadata,
    /// Memory pool configurations for the data plane
    #[serde(default)]
    pub memory_pools: Vec<MemoryPoolConfig>,
    /// Stage definitions
    pub stages: Vec<StageConfig>,
    /// Connection configurations between stages
    #[serde(default)]
    pub connections: Vec<ConnectionConfig>,
}

/// Pipeline metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineMetadata {
    /// Pipeline name
    pub name: String,
    /// Pipeline description
    pub description: Option<String>,
    /// Pipeline version
    pub version: String,
    /// Author information
    pub author: Option<String>,
    /// Creation timestamp
    pub created_at: u64,
    /// Last modified timestamp
    pub modified_at: u64,
    /// Tags for categorization
    pub tags: Vec<String>,
}

/// Individual stage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageConfig {
    /// Unique stage name within the pipeline
    pub name: String,
    /// Stage type identifier
    #[serde(rename = "type")]
    pub stage_type: String,
    /// Stage parameters
    pub params: StageParams,
    /// Input stage names this stage depends on
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Optional stage description
    pub description: Option<String>,
    /// Whether this stage is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

/// Default value for stage enabled field
fn default_enabled() -> bool {
    true
}

/// Memory pool configuration for the data plane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPoolConfig {
    /// Unique name for this memory pool
    pub name: String,
    /// Data type this pool manages (e.g., "RawEegPacket", "VoltageEegPacket")
    pub data_type: String,
    /// Number of packets to pre-allocate in this pool
    pub capacity: usize,
    /// Size of each packet's data buffer in bytes
    pub packet_size: usize,
    /// Optional description
    pub description: Option<String>,
}

/// Connection configuration between stages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// Source stage name
    pub from: String,
    /// Destination stage name
    pub to: String,
    /// Output port name on source stage (default: "out")
    #[serde(default = "default_output_port")]
    pub from_port: String,
    /// Input port name on destination stage (default: "in")
    #[serde(default = "default_input_port")]
    pub to_port: String,
    /// Queue capacity for this connection (None for unbounded)
    pub queue_capacity: Option<usize>,
    /// Optional description
    pub description: Option<String>,
}

/// Default output port name
fn default_output_port() -> String {
    "out".to_string()
}

/// Default input port name
fn default_input_port() -> String {
    "in".to_string()
}

/// Sink configuration for terminal stages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinkConfig {
    /// Sink type (websocket, csv, etc.)
    #[serde(rename = "type")]
    pub sink_type: String,
    /// Sink parameters
    pub params: SinkParams,
    /// Data fields to include in output
    pub fields: Vec<String>,
    /// Output format (json, binary, csv, etc.)
    pub format: String,
}

/// Sink parameters
pub type SinkParams = HashMap<String, serde_json::Value>;

impl PipelineConfig {
    /// Create a new pipeline configuration
    pub fn new(name: String, description: Option<String>) -> Self {
        let now = current_timestamp();
        Self {
            version: "1.0".to_string(),
            metadata: PipelineMetadata {
                name,
                description,
                version: "1.0.0".to_string(),
                author: None,
                created_at: now,
                modified_at: now,
                tags: vec![],
            },
            memory_pools: vec![],
            stages: vec![],
            connections: vec![],
        }
    }

    /// Add a memory pool to the pipeline
    pub fn add_memory_pool(&mut self, pool: MemoryPoolConfig) -> PipelineResult<()> {
        // Check for duplicate pool names
        if self.memory_pools.iter().any(|p| p.name == pool.name) {
            return Err(PipelineError::InvalidConfiguration {
                message: format!("Memory pool name '{}' already exists", pool.name),
            });
        }

        self.memory_pools.push(pool);
        self.metadata.modified_at = current_timestamp();
        Ok(())
    }

    /// Add a stage to the pipeline
    pub fn add_stage(&mut self, stage: StageConfig) -> PipelineResult<()> {
        // Check for duplicate stage names
        if self.stages.iter().any(|s| s.name == stage.name) {
            return Err(PipelineError::InvalidConfiguration {
                message: format!("Stage name '{}' already exists", stage.name),
            });
        }

        self.stages.push(stage);
        self.metadata.modified_at = current_timestamp();
        Ok(())
    }

    /// Add a connection between stages
    pub fn add_connection(&mut self, connection: ConnectionConfig) -> PipelineResult<()> {
        // Check for duplicate connections
        if self.connections.iter().any(|c| {
            c.from == connection.from && c.to == connection.to &&
            c.from_port == connection.from_port && c.to_port == connection.to_port
        }) {
            return Err(PipelineError::InvalidConfiguration {
                message: format!(
                    "Connection from '{}:{}' to '{}:{}' already exists",
                    connection.from, connection.from_port,
                    connection.to, connection.to_port
                ),
            });
        }

        self.connections.push(connection);
        self.metadata.modified_at = current_timestamp();
        Ok(())
    }

    /// Remove a stage from the pipeline
    pub fn remove_stage(&mut self, name: &str) -> PipelineResult<()> {
        let initial_len = self.stages.len();
        self.stages.retain(|s| s.name != name);
        
        if self.stages.len() == initial_len {
            return Err(PipelineError::StageNotFound {
                name: name.to_string(),
            });
        }

        // Remove this stage from other stages' inputs
        for stage in &mut self.stages {
            stage.inputs.retain(|input| input != name);
        }

        self.metadata.modified_at = current_timestamp();
        Ok(())
    }

    /// Get a stage by name
    pub fn get_stage(&self, name: &str) -> Option<&StageConfig> {
        self.stages.iter().find(|s| s.name == name)
    }

    /// Get a mutable reference to a stage by name
    pub fn get_stage_mut(&mut self, name: &str) -> Option<&mut StageConfig> {
        self.stages.iter_mut().find(|s| s.name == name)
    }

    /// Get a memory pool by name
    pub fn get_memory_pool(&self, name: &str) -> Option<&MemoryPoolConfig> {
        self.memory_pools.iter().find(|p| p.name == name)
    }

    /// Get a mutable reference to a memory pool by name
    pub fn get_memory_pool_mut(&mut self, name: &str) -> Option<&mut MemoryPoolConfig> {
        self.memory_pools.iter_mut().find(|p| p.name == name)
    }

    /// Get connections for a specific stage (as source)
    pub fn get_outgoing_connections(&self, stage_name: &str) -> Vec<&ConnectionConfig> {
        self.connections.iter().filter(|c| c.from == stage_name).collect()
    }

    /// Get connections for a specific stage (as destination)
    pub fn get_incoming_connections(&self, stage_name: &str) -> Vec<&ConnectionConfig> {
        self.connections.iter().filter(|c| c.to == stage_name).collect()
    }

    /// Validate the pipeline configuration
    pub fn validate(&self) -> PipelineResult<()> {
        // Check for empty pipeline
        if self.stages.is_empty() {
            return Err(PipelineError::InvalidConfiguration {
                message: "Pipeline must contain at least one stage".to_string(),
            });
        }

        // Validate memory pools
        self.validate_memory_pools()?;

        // Check for circular dependencies
        self.check_circular_dependencies()?;

        // Check that all input references exist
        for stage in &self.stages {
            for input in &stage.inputs {
                if !self.stages.iter().any(|s| s.name == *input) {
                    return Err(PipelineError::InvalidConfiguration {
                        message: format!(
                            "Stage '{}' references non-existent input '{}'",
                            stage.name, input
                        ),
                    });
                }
            }
        }

        // Validate connections
        self.validate_connections()?;

        Ok(())
    }

    /// Validate memory pool configurations
    fn validate_memory_pools(&self) -> PipelineResult<()> {
        for pool in &self.memory_pools {
            if pool.capacity == 0 {
                return Err(PipelineError::InvalidConfiguration {
                    message: format!("Memory pool '{}' must have capacity > 0", pool.name),
                });
            }
            if pool.packet_size == 0 {
                return Err(PipelineError::InvalidConfiguration {
                    message: format!("Memory pool '{}' must have packet_size > 0", pool.name),
                });
            }
        }
        Ok(())
    }

    /// Validate connection configurations
    fn validate_connections(&self) -> PipelineResult<()> {
        for connection in &self.connections {
            // Check that source stage exists
            if !self.stages.iter().any(|s| s.name == connection.from) {
                return Err(PipelineError::InvalidConfiguration {
                    message: format!(
                        "Connection references non-existent source stage '{}'",
                        connection.from
                    ),
                });
            }

            // Check that destination stage exists
            if !self.stages.iter().any(|s| s.name == connection.to) {
                return Err(PipelineError::InvalidConfiguration {
                    message: format!(
                        "Connection references non-existent destination stage '{}'",
                        connection.to
                    ),
                });
            }

            // Validate queue capacity if specified
            if let Some(capacity) = connection.queue_capacity {
                if capacity == 0 {
                    return Err(PipelineError::InvalidConfiguration {
                        message: format!(
                            "Connection from '{}' to '{}' has invalid queue capacity (must be > 0)",
                            connection.from, connection.to
                        ),
                    });
                }
            }
        }
        Ok(())
    }

    /// Check for circular dependencies using depth-first search
    fn check_circular_dependencies(&self) -> PipelineResult<()> {
        let mut visited = HashMap::new();
        let mut rec_stack = HashMap::new();

        for stage in &self.stages {
            if !visited.get(&stage.name).unwrap_or(&false) {
                if self.has_cycle(&stage.name, &mut visited, &mut rec_stack)? {
                    return Err(PipelineError::CircularDependency);
                }
            }
        }

        Ok(())
    }

    /// Recursive helper for cycle detection
    fn has_cycle(
        &self,
        stage_name: &str,
        visited: &mut HashMap<String, bool>,
        rec_stack: &mut HashMap<String, bool>,
    ) -> PipelineResult<bool> {
        visited.insert(stage_name.to_string(), true);
        rec_stack.insert(stage_name.to_string(), true);

        let stage = self.get_stage(stage_name)
            .ok_or_else(|| PipelineError::StageNotFound {
                name: stage_name.to_string(),
            })?;

        for input in &stage.inputs {
            if !visited.get(input).unwrap_or(&false) {
                if self.has_cycle(input, visited, rec_stack)? {
                    return Ok(true);
                }
            } else if *rec_stack.get(input).unwrap_or(&false) {
                return Ok(true);
            }
        }

        rec_stack.insert(stage_name.to_string(), false);
        Ok(false)
    }

    /// Get stages in topological order (dependencies first)
    pub fn topological_order(&self) -> PipelineResult<Vec<&StageConfig>> {
        let mut result = Vec::new();
        let mut visited = HashMap::new();
        let mut temp_mark = HashMap::new();

        for stage in &self.stages {
            if !visited.get(&stage.name).unwrap_or(&false) {
                self.topological_visit(stage, &mut result, &mut visited, &mut temp_mark)?;
            }
        }

        result.reverse();
        Ok(result)
    }

    /// Recursive helper for topological sort
    fn topological_visit<'a>(
        &'a self,
        stage: &'a StageConfig,
        result: &mut Vec<&'a StageConfig>,
        visited: &mut HashMap<String, bool>,
        temp_mark: &mut HashMap<String, bool>,
    ) -> PipelineResult<()> {
        if *temp_mark.get(&stage.name).unwrap_or(&false) {
            return Err(PipelineError::CircularDependency);
        }

        if !visited.get(&stage.name).unwrap_or(&false) {
            temp_mark.insert(stage.name.clone(), true);

            for input_name in &stage.inputs {
                let input_stage = self.get_stage(input_name)
                    .ok_or_else(|| PipelineError::StageNotFound {
                        name: input_name.clone(),
                    })?;
                self.topological_visit(input_stage, result, visited, temp_mark)?;
            }

            temp_mark.insert(stage.name.clone(), false);
            visited.insert(stage.name.clone(), true);
            result.push(stage);
        }

        Ok(())
    }

    /// Load pipeline configuration from JSON
    pub fn from_json(json: &str) -> PipelineResult<Self> {
        let config: PipelineConfig = serde_json::from_str(json)?;
        config.validate()?;
        Ok(config)
    }

    /// Save pipeline configuration to JSON
    pub fn to_json(&self) -> PipelineResult<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Save pipeline configuration to compact JSON
    pub fn to_json_compact(&self) -> PipelineResult<String> {
        Ok(serde_json::to_string(self)?)
    }
}

impl StageConfig {
    /// Create a new stage configuration
    pub fn new(name: String, stage_type: String, params: StageParams) -> Self {
        Self {
            name,
            stage_type,
            params,
            inputs: vec![],
            description: None,
            enabled: true,
        }
    }

    /// Add an input dependency
    pub fn add_input(&mut self, input: String) {
        if !self.inputs.contains(&input) {
            self.inputs.push(input);
        }
    }

    /// Remove an input dependency
    pub fn remove_input(&mut self, input: &str) {
        self.inputs.retain(|i| i != input);
    }

    /// Set stage description
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Set stage enabled state
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Get current timestamp in microseconds
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_config_creation() {
        let config = PipelineConfig::new(
            "test_pipeline".to_string(),
            Some("Test pipeline".to_string()),
        );

        assert_eq!(config.metadata.name, "test_pipeline");
        assert_eq!(config.metadata.description, Some("Test pipeline".to_string()));
        assert_eq!(config.stages.len(), 0);
    }

    #[test]
    fn test_stage_addition() {
        let mut config = PipelineConfig::new("test".to_string(), None);
        let stage = StageConfig::new(
            "stage1".to_string(),
            "test_type".to_string(),
            HashMap::new(),
        );

        assert!(config.add_stage(stage).is_ok());
        assert_eq!(config.stages.len(), 1);

        // Test duplicate name rejection
        let duplicate = StageConfig::new(
            "stage1".to_string(),
            "test_type".to_string(),
            HashMap::new(),
        );
        assert!(config.add_stage(duplicate).is_err());
    }

    #[test]
    fn test_circular_dependency_detection() {
        let mut config = PipelineConfig::new("test".to_string(), None);

        let mut stage1 = StageConfig::new("stage1".to_string(), "test".to_string(), HashMap::new());
        stage1.add_input("stage2".to_string());

        let mut stage2 = StageConfig::new("stage2".to_string(), "test".to_string(), HashMap::new());
        stage2.add_input("stage1".to_string());

        config.add_stage(stage1).unwrap();
        config.add_stage(stage2).unwrap();

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_topological_order() {
        let mut config = PipelineConfig::new("test".to_string(), None);

        let stage1 = StageConfig::new("stage1".to_string(), "test".to_string(), HashMap::new());
        
        let mut stage2 = StageConfig::new("stage2".to_string(), "test".to_string(), HashMap::new());
        stage2.add_input("stage1".to_string());

        let mut stage3 = StageConfig::new("stage3".to_string(), "test".to_string(), HashMap::new());
        stage3.add_input("stage2".to_string());

        config.add_stage(stage1).unwrap();
        config.add_stage(stage2).unwrap();
        config.add_stage(stage3).unwrap();

        let order = config.topological_order().unwrap();
        assert_eq!(order.len(), 3);
        assert_eq!(order[0].name, "stage1");
        assert_eq!(order[1].name, "stage2");
        assert_eq!(order[2].name, "stage3");
    }
}