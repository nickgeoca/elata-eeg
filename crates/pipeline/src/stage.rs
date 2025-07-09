//! Core pipeline stage trait and types

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::PipelineResult;
use crate::data::PipelineData;

/// Core trait that all pipeline stages must implement
#[async_trait]
pub trait PipelineStage: Send + Sync {
    /// Process a single input and produce an output
    async fn process(&mut self, input: PipelineData) -> PipelineResult<PipelineData>;

    /// Get the unique name/identifier for this stage type
    fn stage_type(&self) -> &'static str;

    /// Get a human-readable description of what this stage does
    fn description(&self) -> &'static str {
        "Pipeline stage"
    }

    /// Get the version of this stage implementation
    fn version(&self) -> &'static str {
        "1.0.0"
    }

    /// Initialize the stage (called before processing starts)
    async fn initialize(&mut self) -> PipelineResult<()> {
        Ok(())
    }

    /// Cleanup the stage (called after processing stops)
    async fn cleanup(&mut self) -> PipelineResult<()> {
        Ok(())
    }

    /// Get stage-specific metrics
    fn get_metrics(&self) -> Vec<StageMetric> {
        vec![]
    }

    /// Validate stage parameters
    fn validate_params(&self, params: &StageParams) -> PipelineResult<()> {
        let _ = params;
        Ok(())
    }
}

/// Stage parameters as a flexible key-value map
pub type StageParams = HashMap<String, serde_json::Value>;

/// Stage instance with runtime information
#[derive(Debug, Clone)]
pub struct StageInstance {
    /// Unique instance ID
    pub id: Uuid,
    /// Stage name in the pipeline
    pub name: String,
    /// Stage type identifier
    pub stage_type: String,
    /// Stage parameters
    pub params: StageParams,
    /// Input stage names this stage depends on
    pub inputs: Vec<String>,
    /// Whether this stage is currently locked (in use)
    pub locked: bool,
    /// Runtime state
    pub state: StageState,
}

/// Stage runtime state
#[derive(Debug, Clone, PartialEq)]
pub enum StageState {
    /// Stage is not running
    Idle,
    /// Stage is initializing
    Initializing,
    /// Stage is running and processing data
    Running,
    /// Stage is shutting down
    Stopping,
    /// Stage has encountered an error
    Error(String),
}

/// Stage metrics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageMetric {
    /// Metric name
    pub name: String,
    /// Metric value
    pub value: f64,
    /// Metric unit (e.g., "events/sec", "ms", "bytes")
    pub unit: String,
    /// Optional description
    pub description: Option<String>,
    /// Timestamp when metric was collected
    pub timestamp: u64,
}

/// Stage factory trait for creating stage instances from configuration
#[async_trait]
pub trait StageFactory: Send + Sync {
    /// Create a new stage instance with the given parameters
    async fn create_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn PipelineStage>>;

    /// Get the stage type this factory creates
    fn stage_type(&self) -> &'static str;

    /// Get parameter schema for this stage type
    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
}

/// Registry for stage factories
#[derive(Default)]
pub struct StageRegistry {
    factories: HashMap<String, Box<dyn StageFactory>>,
}

impl StageRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Register a stage factory
    pub fn register<F>(&mut self, factory: F)
    where
        F: StageFactory + 'static,
    {
        let stage_type = factory.stage_type().to_string();
        self.factories.insert(stage_type, Box::new(factory));
    }

    /// Create a stage instance from configuration
    pub async fn create_stage(&self, stage_type: &str, params: &StageParams) -> PipelineResult<Box<dyn PipelineStage>> {
        let factory = self.factories.get(stage_type)
            .ok_or_else(|| crate::error::PipelineError::UnknownStageType {
                stage_type: stage_type.to_string(),
            })?;

        factory.create_stage(params).await
    }

    /// Get all registered stage types
    pub fn stage_types(&self) -> Vec<&str> {
        self.factories.keys().map(|s| s.as_str()).collect()
    }

    /// Get parameter schema for a stage type
    pub fn parameter_schema(&self, stage_type: &str) -> Option<serde_json::Value> {
        self.factories.get(stage_type).map(|f| f.parameter_schema())
    }
}

/// Channel handle for stage communication
#[derive(Debug)]
pub struct StageChannel<T> {
    /// Sender for this channel
    pub sender: mpsc::UnboundedSender<T>,
    /// Receiver for this channel
    pub receiver: mpsc::UnboundedReceiver<T>,
}

impl<T> StageChannel<T> {
    /// Create a new unbounded channel
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self { sender, receiver }
    }

    /// Create a new bounded channel with the specified capacity
    pub fn bounded(capacity: usize) -> (mpsc::Sender<T>, mpsc::Receiver<T>) {
        mpsc::channel(capacity)
    }
}

impl<T> Default for StageChannel<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl StageInstance {
    /// Create a new stage instance
    pub fn new(name: String, stage_type: String, params: StageParams, inputs: Vec<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            stage_type,
            params,
            inputs,
            locked: false,
            state: StageState::Idle,
        }
    }

    /// Lock this stage instance
    pub fn lock(&mut self) {
        self.locked = true;
    }

    /// Unlock this stage instance
    pub fn unlock(&mut self) {
        self.locked = false;
    }

    /// Check if this stage is locked
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Update the stage state
    pub fn set_state(&mut self, state: StageState) {
        self.state = state;
    }

    /// Get the current timestamp in microseconds
    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }
}

impl StageMetric {
    /// Create a new stage metric
    pub fn new(name: String, value: f64, unit: String) -> Self {
        Self {
            name,
            value,
            unit,
            description: None,
            timestamp: StageInstance::current_timestamp(),
        }
    }

    /// Create a new stage metric with description
    pub fn with_description(name: String, value: f64, unit: String, description: String) -> Self {
        Self {
            name,
            value,
            unit,
            description: Some(description),
            timestamp: StageInstance::current_timestamp(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_instance_creation() {
        let params = HashMap::new();
        let inputs = vec!["input1".to_string()];
        let instance = StageInstance::new(
            "test_stage".to_string(),
            "test_type".to_string(),
            params,
            inputs.clone(),
        );

        assert_eq!(instance.name, "test_stage");
        assert_eq!(instance.stage_type, "test_type");
        assert_eq!(instance.inputs, inputs);
        assert!(!instance.is_locked());
        assert_eq!(instance.state, StageState::Idle);
    }

    #[test]
    fn test_stage_locking() {
        let mut instance = StageInstance::new(
            "test".to_string(),
            "test".to_string(),
            HashMap::new(),
            vec![],
        );

        assert!(!instance.is_locked());
        instance.lock();
        assert!(instance.is_locked());
        instance.unlock();
        assert!(!instance.is_locked());
    }

    #[test]
    fn test_stage_registry() {
        let registry = StageRegistry::new();
        assert_eq!(registry.stage_types().len(), 0);
    }
}