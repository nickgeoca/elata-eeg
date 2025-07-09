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
// --- New Data Plane Types ---

use crate::data::{Packet, VoltageEegPacket};
use crate::error::StageError;
use tokio::sync::mpsc::error::TrySendError;

// In a real implementation, MemoryPool would be a complex struct.
// For now, a type alias is sufficient for the code to compile.
pub type MemoryPool = ();

/// The trait for receiving a packet from an upstream stage.
#[async_trait]
pub trait Input<T>: Send + Sync {
    async fn recv(&mut self) -> Result<Option<Packet<T>>, StageError>;
    fn try_recv(&mut self) -> Result<Option<Packet<T>>, StageError>;
}

/// The trait for sending a packet to a downstream stage.
#[async_trait]
pub trait Output<T>: Send + Sync {
    async fn send(&mut self, packet: Packet<T>) -> Result<(), StageError>;
    fn try_send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>>;
}

/// The main trait for a data plane stage.
#[async_trait]
pub trait DataPlaneStage: Send + Sync {
    async fn run(&mut self, context: &mut StageContext) -> Result<(), StageError>;
}

/// A message sent from the Control Plane to a specific stage.
#[derive(Debug)]
pub enum ControlMsg {
    Pause,
    Resume,
    UpdateParam(String, serde_json::Value),
}

/// Contains everything a stage needs to run.
pub struct StageContext {
    pub memory_pools: HashMap<String, Arc<MemoryPool>>,
    // The generic type parameter for Input/Output will need to be handled
    // more robustly in a real implementation, likely with type erasure
    // (e.g., using `Box<dyn Any>`). For this specific test case, we can
    // hardcode it to the type we know `FilterStage` uses.
    pub inputs: HashMap<String, Box<dyn Input<VoltageEegPacket>>>,
    pub outputs: HashMap<String, Box<dyn Output<VoltageEegPacket>>>,
    pub control_rx: mpsc::UnboundedReceiver<ControlMsg>,
}


// --- Blanket Implementations for Tokio MPSC Channels ---

use tokio::sync::mpsc::{Receiver, Sender, UnboundedReceiver, UnboundedSender};

#[async_trait]
impl<T: Send + Sync + 'static> Input<T> for Receiver<Packet<T>> {
    async fn recv(&mut self) -> Result<Option<Packet<T>>, StageError> {
        Ok(self.recv().await)
    }

    fn try_recv(&mut self) -> Result<Option<Packet<T>>, StageError> {
        match self.try_recv() {
            Ok(p) => Ok(Some(p)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(StageError::QueueClosed),
        }
    }
}

#[async_trait]
impl<T: Send + Sync + 'static> Input<T> for UnboundedReceiver<Packet<T>> {
    async fn recv(&mut self) -> Result<Option<Packet<T>>, StageError> {
        Ok(self.recv().await)
    }

    fn try_recv(&mut self) -> Result<Option<Packet<T>>, StageError> {
        match self.try_recv() {
            Ok(p) => Ok(Some(p)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(StageError::QueueClosed),
        }
    }
}

#[async_trait]
impl<T: Send + Sync + 'static> Output<T> for Sender<Packet<T>> {
    async fn send(&mut self, packet: Packet<T>) -> Result<(), StageError> {
        mpsc::Sender::send(self, packet)
            .await
            .map_err(|_| StageError::Fatal("output channel closed".into()))
    }

    fn try_send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>> {
        mpsc::Sender::try_send(self, packet)
    }
}

#[async_trait]
impl<T: Send + Sync + 'static> Output<T> for UnboundedSender<Packet<T>> {
    async fn send(&mut self, packet: Packet<T>) -> Result<(), StageError> {
        mpsc::UnboundedSender::send(self, packet)
            .map_err(|_| StageError::Fatal("output channel closed".into()))
    }

    fn try_send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>> {
        mpsc::UnboundedSender::send(self, packet).map_err(|e| TrySendError::Closed(e.0))
    }
}


// --- Data Plane Factory and Registry ---

/// Stage factory trait for creating data plane stage instances from configuration.
///
/// This is the data plane counterpart to the control plane's `StageFactory`.
#[async_trait]
pub trait DataPlaneStageFactory: Send + Sync {
    /// Create a new data plane stage instance with the given parameters.
    async fn create_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn DataPlaneStage>>;

    /// Get the stage type this factory creates.
    fn stage_type(&self) -> &'static str;

    /// Get parameter schema for this stage type.
    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
}

/// Registry for data plane stage factories.
#[derive(Default)]
pub struct DataPlaneStageRegistry {
    factories: HashMap<String, Box<dyn DataPlaneStageFactory>>,
}

impl DataPlaneStageRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        let mut registry = Self {
            factories: HashMap::new(),
        };
        registry.populate();
        registry
    }

    /// Populates the registry with all statically registered factories.
    fn populate(&mut self) {
        for registrar in inventory::iter::<StaticStageRegistrar> {
            let factory = (registrar.factory_fn)();
            self.register_boxed(factory);
        }
    }

    /// Register a data plane stage factory.
    /// Register a data plane stage factory.
    pub fn register<F>(&mut self, factory: F)
    where
        F: DataPlaneStageFactory + 'static,
    {
        self.register_boxed(Box::new(factory));
    }

    /// Registers a boxed factory.
    fn register_boxed(&mut self, factory: Box<dyn DataPlaneStageFactory>) {
        let stage_type = factory.stage_type().to_string();
        self.factories.insert(stage_type, factory);
    }

    /// Create a stage instance from configuration.
    pub async fn create_stage(
        &self,
        stage_type: &str,
        params: &StageParams,
    ) -> PipelineResult<Box<dyn DataPlaneStage>> {
        let factory = self.factories.get(stage_type).ok_or_else(|| {
            crate::error::PipelineError::UnknownStageType {
                stage_type: stage_type.to_string(),
            }
        })?;

        factory.create_stage(params).await
    }

    /// Get all registered stage types.
    pub fn stage_types(&self) -> Vec<&str> {
        self.factories.keys().map(|s| s.as_str()).collect()
    }

    /// Get parameter schema for a stage type.
    pub fn parameter_schema(&self, stage_type: &str) -> Option<serde_json::Value> {
        self.factories.get(stage_type).map(|f| f.parameter_schema())
    }
}

/// A struct for statically registering a `DataPlaneStageFactory`.
///
/// This struct is collected by `inventory` at compile time.
pub struct StaticStageRegistrar {
    /// A function that creates a new instance of the factory.
    pub factory_fn: fn() -> Box<dyn DataPlaneStageFactory>,
}

inventory::collect!(StaticStageRegistrar);
