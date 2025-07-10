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

use crate::data::{MemoryPool, Packet, AnyPacketType};
use crate::error::StageError;
use tokio::sync::mpsc::error::TrySendError;
use std::sync::Mutex; // Added for Mutex

/// The trait for receiving a packet from an upstream stage.
#[async_trait]
pub trait Input<T: AnyPacketType>: Send + Sync {
    async fn recv(&mut self) -> Result<Option<Packet<T>>, StageError>;
    fn try_recv(&mut self) -> Result<Option<Packet<T>>, StageError>;
}

/// The trait for sending a packet to a downstream stage.
#[async_trait]
pub trait Output<T: AnyPacketType>: Send + Sync {
    async fn send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>>;
    fn try_send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>>;
}

/// The main trait for a data plane stage.
#[async_trait]
pub trait DataPlaneStage<InputPacket: AnyPacketType, OutputPacket: AnyPacketType>: Send + Sync {
    async fn run(&mut self, context: &mut StageContext<InputPacket, OutputPacket>) -> Result<(), StageError>;
}

/// Type-erased trait for DataPlaneStage to enable object-safe storage in registries.
/// This trait allows stages with different InputPacket and OutputPacket types to be
/// stored together in a HashMap while maintaining type safety through the factory pattern.
#[async_trait]
pub trait DataPlaneStageErased: Send + Sync {
    /// Run the stage with type-erased context.
    /// The concrete stage implementation will downcast the context to its expected types.
    async fn run_erased(&mut self, context: &mut dyn ErasedStageContext) -> Result<(), StageError>;
}

/// Type-erased context trait to enable object-safe stage execution.
pub trait ErasedStageContext: Send + Sync {
    /// Get the control message receiver
    fn control_rx(&mut self) -> &mut mpsc::UnboundedReceiver<ControlMsg>;
    
    /// Get memory pools as a type-erased map
    fn memory_pools(&mut self) -> &mut HashMap<String, Arc<Mutex<dyn AnyMemoryPool>>>;
    
    /// Attempt to downcast to a concrete StageContext type
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

// Note: We cannot provide a blanket implementation for DataPlaneStageErased
// because it would have unconstrained type parameters. Instead, each concrete
// stage type must implement DataPlaneStageErased manually or through a macro.

/// Implement ErasedStageContext for concrete StageContext
impl<I: AnyPacketType, O: AnyPacketType> ErasedStageContext for StageContext<I, O> {
    fn control_rx(&mut self) -> &mut mpsc::UnboundedReceiver<ControlMsg> {
        &mut self.control_rx
    }
    
    fn memory_pools(&mut self) -> &mut HashMap<String, Arc<Mutex<dyn AnyMemoryPool>>> {
        &mut self.memory_pools
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A message sent from the Control Plane to a specific stage.
#[derive(Debug)]
pub enum ControlMsg {
    Pause,
    Resume,
    UpdateParam(String, serde_json::Value),
}

/// Contains everything a stage needs to run.
pub struct StageContext<InputPacket: AnyPacketType, OutputPacket: AnyPacketType> {
    pub memory_pools: HashMap<String, Arc<Mutex<dyn AnyMemoryPool>>>,
    pub inputs: HashMap<String, Box<dyn Input<InputPacket>>>,
    pub outputs: HashMap<String, Box<dyn Output<OutputPacket>>>,
    pub control_rx: mpsc::UnboundedReceiver<ControlMsg>,
}

/// A trait for type-erasing `MemoryPool<T>` so it can be stored in a `HashMap`.
pub trait AnyMemoryPool: Send + Sync + 'static {
    // We don't need to expose acquire/try_acquire here, as stages will get
    // their specific pools from the context and call those methods directly.
    // This trait is primarily for storing different `MemoryPool<T>` types together.
    
    /// Downcast to Any for type recovery
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

impl<T: AnyPacketType> AnyMemoryPool for MemoryPool<T> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}


// --- Blanket Implementations for Tokio MPSC Channels ---

use tokio::sync::mpsc::{Receiver, Sender, UnboundedReceiver, UnboundedSender};

#[async_trait]
impl<T: AnyPacketType> Input<T> for Receiver<Packet<T>> {
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
impl<T: AnyPacketType> Input<T> for UnboundedReceiver<Packet<T>> {
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
impl<T: AnyPacketType> Output<T> for Sender<Packet<T>> {
    async fn send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>> {
        mpsc::Sender::send(self, packet)
            .await
            .map_err(|e| TrySendError::Closed(e.0))
    }

    fn try_send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>> {
        mpsc::Sender::try_send(self, packet)
    }
}

#[async_trait]
impl<T: AnyPacketType> Output<T> for UnboundedSender<Packet<T>> {
    async fn send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>> {
        mpsc::UnboundedSender::send(self, packet).map_err(|e| TrySendError::Closed(e.0))
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
pub trait DataPlaneStageFactory<InputPacket: AnyPacketType, OutputPacket: AnyPacketType>: Send + Sync {
    /// Create a new data plane stage instance with the given parameters.
    async fn create_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn DataPlaneStage<InputPacket, OutputPacket>>>;

    /// Get the stage type this factory creates.
    fn stage_type(&self) -> &'static str;

    /// Get parameter schema for this stage type.
    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
}

/// Type-erased factory trait that is object-safe and can be stored in registries.
/// This trait provides a bridge between concrete DataPlaneStageFactory<I, O> implementations
/// and the registry that needs to store different factory types together.
#[async_trait]
pub trait ErasedDataPlaneStageFactory: Send + Sync {
    /// Create a new data plane stage instance with type erasure.
    /// Returns a Box<dyn DataPlaneStageErased> that can be stored and executed
    /// without knowing the concrete InputPacket and OutputPacket types.
    async fn create_erased_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn DataPlaneStageErased>>;

    /// Get the stage type this factory creates.
    fn stage_type(&self) -> &'static str;

    /// Get parameter schema for this stage type.
    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
}

// Note: We cannot provide a blanket implementation for ErasedDataPlaneStageFactory
// because it would have unconstrained type parameters. Instead, each concrete
// factory type must implement ErasedDataPlaneStageFactory manually or through a macro.

/// Registry for data plane stage factories.
#[derive(Default)]
pub struct DataPlaneStageRegistry {
    factories: HashMap<String, Box<dyn ErasedDataPlaneStageFactory>>,
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
    pub fn register<F, InputPacket, OutputPacket>(&mut self, factory: F)
    where
        F: DataPlaneStageFactory<InputPacket, OutputPacket> + ErasedDataPlaneStageFactory + 'static,
        InputPacket: AnyPacketType,
        OutputPacket: AnyPacketType,
    {
        let stage_type = DataPlaneStageFactory::stage_type(&factory).to_string();
        self.factories.insert(stage_type, Box::new(factory));
    }

    /// Registers a boxed erased factory.
    fn register_boxed(&mut self, factory: Box<dyn ErasedDataPlaneStageFactory>) {
        let stage_type = factory.stage_type().to_string();
        self.factories.insert(stage_type, factory);
    }

    /// Create a stage instance from configuration, returning a type-erased stage.
    /// This is the primary method for creating stages from the registry.
    pub async fn create_erased_stage(
        &self,
        stage_type: &str,
        params: &StageParams,
    ) -> PipelineResult<Box<dyn DataPlaneStageErased>> {
        let factory = self.factories.get(stage_type).ok_or_else(|| {
            crate::error::PipelineError::UnknownStageType {
                stage_type: stage_type.to_string(),
            }
        })?;

        factory.create_erased_stage(params).await
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
pub struct StaticStageRegistrar {
    pub factory_fn: fn() -> Box<dyn ErasedDataPlaneStageFactory>,
}

inventory::collect!(StaticStageRegistrar);
