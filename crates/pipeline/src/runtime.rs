//! Pipeline runtime for executing pipeline graphs

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, error, debug};
use uuid::Uuid;

use crate::config::PipelineConfig;
use crate::data::PipelineData;
use crate::error::{PipelineError, PipelineResult, StageError};
use crate::graph::{GraphBuilder, GraphState, PipelineGraph};
use crate::stage::{
    StageInstance, StageState, StageMetric, PipelineStage, DataPlaneStageRegistry, ControlMsg,
    DataPlaneStageErased, ErasedStageContext, AnyMemoryPool
};

/// Pipeline runtime for executing pipeline graphs
pub struct PipelineRuntime {
    /// Runtime ID
    pub id: Uuid,
    /// Data plane stage registry for new architecture
    data_plane_registry: Arc<DataPlaneStageRegistry>,
    /// Currently loaded pipeline graph
    graph: Option<Arc<RwLock<PipelineGraph>>>,
    /// Running stage tasks
    stage_tasks: HashMap<String, JoinHandle<PipelineResult<()>>>,
    /// Stage channels for data flow
    stage_channels: HashMap<String, StageChannelSet>,
    /// Control channels for sending commands to stages
    control_channels: HashMap<String, mpsc::UnboundedSender<ControlMsg>>,
    /// Global cancellation token
    cancellation_token: CancellationToken,
    /// Runtime state with recording lock
    state: Arc<tokio::sync::RwLock<PipelineState>>,
    /// Runtime metrics
    metrics: Arc<tokio::sync::Mutex<RuntimeMetrics>>,
    /// Metrics sender for collecting stage metrics
    metrics_sender: mpsc::UnboundedSender<StageMetric>,
    /// Metrics receiver for collecting stage metrics
    metrics_receiver: mpsc::UnboundedReceiver<StageMetric>,
}

/// Pipeline state with recording lock functionality
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum PipelineState {
    /// Pipeline is idle - parameters can be changed
    Idle,
    /// Pipeline is loading a configuration
    Loading,
    /// Pipeline is starting up
    Starting,
    /// Pipeline is running - parameters are locked to prevent live edits
    Running,
    /// Pipeline is paused - parameters can be changed
    Paused,
    /// Pipeline is stopping
    Stopping,
    /// Pipeline has encountered an error
    Error(String),
}

/// Legacy runtime state for backward compatibility
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeState {
    /// Runtime is idle
    Idle,
    /// Runtime is loading a pipeline
    Loading,
    /// Runtime is starting pipeline execution
    Starting,
    /// Runtime is executing the pipeline
    Running,
    /// Runtime is stopping pipeline execution
    Stopping,
    /// Runtime has encountered an error
    Error(String),
}

/// Runtime metrics
#[derive(Debug, Clone, serde::Serialize)]
pub struct RuntimeMetrics {
    /// Total number of data items processed
    pub items_processed: u64,
    /// Total number of errors encountered
    pub error_count: u64,
    /// Runtime uptime in milliseconds
    pub uptime_ms: u64,
    /// Start time (as Unix timestamp in milliseconds)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time_ms: Option<u64>,
    /// Stage-specific metrics
    pub stage_metrics: HashMap<String, Vec<StageMetric>>,
    /// Queue length metrics per stage
    pub queue_lengths: HashMap<String, usize>,
    /// Memory pool usage metrics
    pub pool_usage: HashMap<String, PoolUsageMetric>,
}

/// Memory pool usage metrics
#[derive(Debug, Clone, serde::Serialize)]
pub struct PoolUsageMetric {
    /// Pool name
    pub name: String,
    /// Total capacity
    pub capacity: usize,
    /// Currently allocated packets
    pub allocated: usize,
    /// Available packets
    pub available: usize,
    /// Utilization percentage (0.0 - 1.0)
    pub utilization: f64,
}

/// Channel set for a stage (input and output channels)
#[derive(Debug)]
struct StageChannelSet {
    /// Input channels from upstream stages
    inputs: HashMap<String, mpsc::UnboundedReceiver<PipelineData>>,
    /// Output channels to downstream stages
    outputs: HashMap<String, mpsc::UnboundedSender<PipelineData>>,
}

/// Pipeline execution context passed to stages
pub struct ExecutionContext {
    /// Stage name
    pub stage_name: String,
    /// Input channels
    pub inputs: HashMap<String, mpsc::UnboundedReceiver<PipelineData>>,
    /// Output channels
    pub outputs: HashMap<String, mpsc::UnboundedSender<PipelineData>>,
    /// Cancellation token
    pub cancellation_token: CancellationToken,
    /// Metrics sender
    pub metrics_sender: mpsc::UnboundedSender<StageMetric>,
}

impl PipelineRuntime {
    /// Create a new pipeline runtime with data plane registry
    pub fn new(data_plane_registry: Arc<DataPlaneStageRegistry>) -> Self {
        let (metrics_sender, metrics_receiver) = mpsc::unbounded_channel();
        Self {
            id: Uuid::new_v4(),
            data_plane_registry,
            graph: None,
            stage_tasks: HashMap::new(),
            stage_channels: HashMap::new(),
            control_channels: HashMap::new(),
            cancellation_token: CancellationToken::new(),
            state: Arc::new(tokio::sync::RwLock::new(PipelineState::Idle)),
            metrics: Arc::new(tokio::sync::Mutex::new(RuntimeMetrics::new())),
            metrics_sender,
            metrics_receiver,
        }
    }

    /// Load a pipeline configuration
    pub async fn load_pipeline(&mut self, config: &PipelineConfig) -> PipelineResult<()> {
        let current_state = self.state.read().await.clone();
        if current_state == PipelineState::Running {
            return Err(PipelineError::AlreadyRunning);
        }

        *self.state.write().await = PipelineState::Loading;
        info!("Loading pipeline: {}", config.metadata.name);

        // Build the graph using the data plane registry
        let builder = GraphBuilder::new(self.data_plane_registry.clone());
        let graph = builder.build(config).await?;

        info!("Pipeline graph built with {} stages", graph.stages.len());
        debug!("Graph sources: {:?}", graph.sources);
        debug!("Graph sinks: {:?}", graph.sinks);

        // Store the graph
        self.graph = Some(Arc::new(RwLock::new(graph)));
        *self.state.write().await = PipelineState::Idle;

        Ok(())
    }

    /// Start pipeline execution
    pub async fn start(&mut self) -> PipelineResult<()> {
        let current_state = self.state.read().await.clone();
        if current_state == PipelineState::Running {
            return Err(PipelineError::AlreadyRunning);
        }

        let graph = self.graph.clone()
            .ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "No pipeline loaded".to_string(),
            })?;

        *self.state.write().await = PipelineState::Starting;
        info!("Starting pipeline execution");

        // Reset cancellation token
        self.cancellation_token = CancellationToken::new();

        // Setup channels between stages
        info!("Setting up channels between stages");
        self.setup_stage_channels(&graph).await?;

        // Start stage tasks
        info!("Starting stage tasks");
        self.start_stage_tasks(&graph).await?;

        // Start metrics collection task
        self.start_metrics_collection_task();

        // Update graph state
        {
            let mut graph_guard = graph.write().await;
            graph_guard.set_state(GraphState::Running);
            graph_guard.set_all_stage_states(StageState::Running);
        }

        *self.state.write().await = PipelineState::Running;
        {
            let mut metrics = self.metrics.lock().await;
            metrics.start_time_ms = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64
            );
        }
        info!("Pipeline started successfully - Recording Lock ACTIVE");

        Ok(())
    }

    /// Stop pipeline execution
    pub async fn stop(&mut self) -> PipelineResult<()> {
        let current_state = self.state.read().await.clone();
        if current_state != PipelineState::Running && current_state != PipelineState::Paused {
            return Err(PipelineError::NotRunning);
        }

        *self.state.write().await = PipelineState::Stopping;
        info!("Stopping pipeline execution");

        // Signal cancellation
        self.cancellation_token.cancel();

        // Update graph state
        if let Some(graph) = &self.graph {
            let mut graph_guard = graph.write().await;
            graph_guard.set_state(GraphState::Stopping);
            graph_guard.set_all_stage_states(StageState::Stopping);
        }

        // Wait for all stage tasks to complete
        let mut tasks = std::mem::take(&mut self.stage_tasks);
        for (stage_name, task) in tasks.drain() {
            match task.await {
                Ok(Ok(())) => {
                    debug!("Stage '{}' stopped successfully", stage_name);
                }
                Ok(Err(e)) => {
                    warn!("Stage '{}' stopped with error: {}", stage_name, e);
                }
                Err(e) => {
                    error!("Failed to join stage '{}' task: {}", stage_name, e);
                }
            }
        }

        // Clear channels
        self.stage_channels.clear();
        self.control_channels.clear();

        // Update graph state
        if let Some(graph) = &self.graph {
            let mut graph_guard = graph.write().await;
            graph_guard.set_state(GraphState::Idle);
            graph_guard.set_all_stage_states(StageState::Idle);
            graph_guard.unlock_all_stages();
        }

        *self.state.write().await = PipelineState::Idle;
        info!("Pipeline stopped successfully - Recording Lock RELEASED");

        Ok(())
    }

    /// Pause pipeline execution (allows parameter changes)
    pub async fn pause(&mut self) -> PipelineResult<()> {
        let current_state = self.state.read().await.clone();
        if current_state != PipelineState::Running {
            return Err(PipelineError::InvalidState(
                "Pipeline must be running to pause".to_string()
            ));
        }

        *self.state.write().await = PipelineState::Paused;
        info!("Pipeline paused - Recording Lock RELEASED");

        // Send pause command to all stages
        for (stage_name, control_tx) in &self.control_channels {
            if let Err(e) = control_tx.send(ControlMsg::Pause) {
                warn!("Failed to send pause command to stage '{}': {}", stage_name, e);
            }
        }

        Ok(())
    }

    /// Resume pipeline execution (re-enables recording lock)
    pub async fn resume(&mut self) -> PipelineResult<()> {
        let current_state = self.state.read().await.clone();
        if current_state != PipelineState::Paused {
            return Err(PipelineError::InvalidState(
                "Pipeline must be paused to resume".to_string()
            ));
        }

        *self.state.write().await = PipelineState::Running;
        info!("Pipeline resumed - Recording Lock ACTIVE");

        // Send resume command to all stages
        for (stage_name, control_tx) in &self.control_channels {
            if let Err(e) = control_tx.send(ControlMsg::Resume) {
                warn!("Failed to send resume command to stage '{}': {}", stage_name, e);
            }
        }

        Ok(())
    }

    /// Update a parameter on a specific stage (respects recording lock)
    pub async fn update_stage_parameter(
        &self,
        stage_id: &str,
        key: String,
        value: serde_json::Value,
    ) -> PipelineResult<()> {
        let current_state = self.state.read().await.clone();
        
        // Enforce recording lock - only allow parameter updates when not running
        if current_state == PipelineState::Running {
            return Err(PipelineError::InvalidState(
                "Cannot update parameters while pipeline is running (Recording Lock active). Pause the pipeline first.".to_string()
            ));
        }

        // Find the control channel for the target stage
        let control_tx = self.control_channels.get(stage_id)
            .ok_or_else(|| PipelineError::StageNotFound {
                name: stage_id.to_string(),
            })?;

        // Send the parameter update command
        control_tx.send(ControlMsg::UpdateParam(key.clone(), value.clone()))
            .map_err(|_| PipelineError::ChannelError(
                format!("Failed to send parameter update to stage '{}'", stage_id)
            ))?;

        info!("Parameter '{}' updated for stage '{}'", key, stage_id);
        Ok(())
    }

    /// Get current pipeline state
    pub async fn state(&self) -> PipelineState {
        self.state.read().await.clone()
    }

    /// Get current pipeline state (non-async for compatibility)
    pub fn state_sync(&self) -> Arc<tokio::sync::RwLock<PipelineState>> {
        self.state.clone()
    }

    /// Get runtime metrics
    pub async fn metrics(&self) -> RuntimeMetrics {
        let mut metrics = self.metrics.lock().await.clone();
        metrics.update_uptime();
        metrics
    }

    /// Get pipeline graph (read-only)
    pub async fn graph(&self) -> Option<Arc<RwLock<PipelineGraph>>> {
        self.graph.clone()
    }

    /// Setup channels between stages
    async fn setup_stage_channels(&mut self, graph: &Arc<RwLock<PipelineGraph>>) -> PipelineResult<()> {
        let graph_guard = graph.read().await;
        
        // Create channels for each edge in the graph
        let mut all_senders: HashMap<String, HashMap<String, mpsc::UnboundedSender<PipelineData>>> = HashMap::new();
        let mut all_receivers: HashMap<String, HashMap<String, mpsc::UnboundedReceiver<PipelineData>>> = HashMap::new();

        // Initialize channel maps for each stage
        for stage_name in graph_guard.stages.keys() {
            all_senders.insert(stage_name.clone(), HashMap::new());
            all_receivers.insert(stage_name.clone(), HashMap::new());
        }

        // Create channels for each edge
        for (from_stage, to_stages) in &graph_guard.edges {
            for to_stage in to_stages {
                let (sender, receiver) = mpsc::unbounded_channel();
                
                // Store sender with the source stage
                all_senders.get_mut(from_stage)
                    .unwrap()
                    .insert(to_stage.clone(), sender);
                
                // Store receiver with the destination stage
                all_receivers.get_mut(to_stage)
                    .unwrap()
                    .insert(from_stage.clone(), receiver);
            }
        }

        // Create StageChannelSet for each stage
        for stage_name in graph_guard.stages.keys() {
            let inputs = all_receivers.remove(stage_name).unwrap_or_default();
            let outputs = all_senders.remove(stage_name).unwrap_or_default();
            
            self.stage_channels.insert(stage_name.clone(), StageChannelSet {
                inputs,
                outputs,
            });
        }

        Ok(())
    }

    /// Start all stage tasks
    async fn start_stage_tasks(&mut self, graph: &Arc<RwLock<PipelineGraph>>) -> PipelineResult<()> {
        let graph_guard = graph.read().await;
        
        // Get stages in topological order
        let stage_order = graph_guard.topological_order()?;
        info!("Stage execution order: {:?}", stage_order);
        
        for stage_name in stage_order {
            info!("Starting stage task: {}", stage_name);
            let stage_instance = graph_guard.get_stage(stage_name)
                .ok_or_else(|| PipelineError::StageNotFound {
                    name: stage_name.to_string(),
                })?;

            // Create the actual stage implementation using data plane registry
            let mut stage = self.data_plane_registry.create_erased_stage(&stage_instance.stage_type, &stage_instance.params).await?;

            // Get channels for this stage
            let channels = self.stage_channels.remove(stage_name)
                .ok_or_else(|| PipelineError::ChannelError(
                    format!("No channels found for stage '{}'", stage_name)
                ))?;

            // Create execution context
            let context = ExecutionContext {
                stage_name: stage_name.to_string(),
                inputs: channels.inputs,
                outputs: channels.outputs,
                cancellation_token: self.cancellation_token.clone(),
                metrics_sender: self.metrics_sender.clone(),
            };

            // Start the stage task
            let task = tokio::spawn(async move {
                Self::run_stage(stage, context).await
            });

            self.stage_tasks.insert(stage_name.to_string(), task);
        }

        Ok(())
    }

    /// Run a single stage using the new data plane architecture
    async fn run_stage(
        mut stage: Box<dyn DataPlaneStageErased>,
        context: ExecutionContext,
    ) -> PipelineResult<()> {
        info!("Starting stage: {}", context.stage_name);

        // Create a type-erased context that implements ErasedStageContext
        let mut erased_context = TypeErasedStageContext {
            control_rx: context.control_rx,
            memory_pools: context.memory_pools,
            inputs: context.inputs,
            outputs: context.outputs,
            cancellation_token: context.cancellation_token,
            metrics_sender: context.metrics_sender,
            stage_name: context.stage_name,
        };

        // Run the stage with the erased context
        loop {
            tokio::select! {
                // Check for cancellation
                _ = erased_context.cancellation_token.cancelled() => {
                    info!("Stage '{}' received cancellation signal", erased_context.stage_name);
                    break;
                }
                
                // Run the stage
                result = stage.run_erased(&mut erased_context) => {
                    match result {
                        Ok(()) => {
                            // Stage completed normally, continue
                        }
                        Err(StageError::QueueClosed) => {
                            info!("Stage '{}' input queue closed, shutting down", erased_context.stage_name);
                            break;
                        }
                        Err(e) => {
                            error!("Stage '{}' encountered error: {}", erased_context.stage_name, e);
                            // Send error metric
                            let _ = erased_context.metrics_sender.send(StageMetric {
                                name: "errors".to_string(),
                                value: 1.0,
                                unit: "count".to_string(),
                                description: Some("Number of errors encountered".to_string()),
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_micros() as u64,
                            });
                            return Err(PipelineError::StageError(e));
                        }
                    }
                }
            }
        }

        info!("Stage '{}' shut down", erased_context.stage_name);
        Ok(())
    }
}

/// Type-erased context implementation for the runtime
struct TypeErasedStageContext {
    control_rx: mpsc::UnboundedReceiver<ControlMsg>,
    memory_pools: HashMap<String, Arc<Mutex<dyn AnyMemoryPool>>>,
    inputs: HashMap<String, Box<dyn std::any::Any + Send>>,
    outputs: HashMap<String, Box<dyn std::any::Any + Send>>,
    cancellation_token: CancellationToken,
    metrics_sender: mpsc::UnboundedSender<StageMetric>,
    stage_name: String,
}

impl ErasedStageContext for TypeErasedStageContext {
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

impl PipelineRuntime {
    /// Start metrics collection task
    fn start_metrics_collection_task(&mut self) {
        let mut metrics_receiver = std::mem::replace(&mut self.metrics_receiver, mpsc::unbounded_channel().1);
        let cancellation_token = self.cancellation_token.clone();
        let metrics = self.metrics.clone();
        
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancellation_token.cancelled() => {
                        break;
                    }
                    metric = metrics_receiver.recv() => {
                        match metric {
                            Some(metric) => {
                                // Update the shared metrics
                                let mut metrics_guard = metrics.lock().await;
                                match metric.name.as_str() {
                                    "items_processed" => {
                                        metrics_guard.items_processed += metric.value as u64;
                                    }
                                    "errors" => {
                                        metrics_guard.error_count += metric.value as u64;
                                    }
                                    _ => {
                                        // Log other metrics for debugging
                                        tracing::debug!("Received metric: {} = {} {}", metric.name, metric.value, metric.unit);
                                    }
                                }
                            }
                            None => {
                                // Channel closed
                                break;
                            }
                        }
                    }
                }
            }
        });
    }

    /// Receive input data from any available input channel
    async fn receive_input_data(
        inputs: &mut HashMap<String, mpsc::UnboundedReceiver<PipelineData>>,
    ) -> Option<PipelineData> {
        if inputs.is_empty() {
            // Source stage - generate a trigger signal
            debug!("Source stage: generating trigger");
            return Some(PipelineData::Trigger);
        }

        // Try to receive from any input channel (round-robin style)
        for (input_name, receiver) in inputs.iter_mut() {
            match receiver.try_recv() {
                Ok(data) => {
                    debug!("Received data from input '{}'", input_name);
                    return Some(data);
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    // No data available on this channel, try next
                    continue;
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    warn!("Input channel '{}' disconnected", input_name);
                    // Channel is closed, but continue with other channels
                    continue;
                }
            }
        }

        None
    }

    /// Send output data to all connected downstream stages
    async fn send_output_data(
        outputs: &HashMap<String, mpsc::UnboundedSender<PipelineData>>,
        data: PipelineData,
    ) -> PipelineResult<()> {
        if outputs.is_empty() {
            // Sink stage - data processing is complete
            return Ok(());
        }

        // Clone data for each output channel (except the last one)
        let output_count = outputs.len();
        let mut outputs_iter = outputs.iter();

        // Send to all but the last output (requiring clones)
        for (i, (output_name, sender)) in outputs_iter.by_ref().enumerate() {
            if i < output_count - 1 {
                // Clone the data for all but the last output (PipelineData implements Clone)
                let cloned_data = data.clone();
                if let Err(e) = sender.send(cloned_data) {
                    warn!("Failed to send data to output '{}': {}", output_name, e);
                }
            } else {
                // Send the original data to the last output (no clone needed)
                if let Err(e) = sender.send(data) {
                    warn!("Failed to send data to output '{}': {}", output_name, e);
                }
                break;
            }
        }

        Ok(())
    }

impl RuntimeMetrics {
    /// Create new runtime metrics
    fn new() -> Self {
        Self {
            items_processed: 0,
            error_count: 0,
            uptime_ms: 0,
            start_time_ms: None,
            stage_metrics: HashMap::new(),
            queue_lengths: HashMap::new(),
            pool_usage: HashMap::new(),
        }
    }

    /// Update uptime based on start time
    pub fn update_uptime(&mut self) {
        if let Some(start_time_ms) = self.start_time_ms {
            let current_time_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            self.uptime_ms = current_time_ms.saturating_sub(start_time_ms);
        }
    }

    /// Add stage metrics
    pub fn add_stage_metrics(&mut self, stage_name: String, metrics: Vec<StageMetric>) {
        self.stage_metrics.insert(stage_name, metrics);
    }
}

impl Default for RuntimeMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PipelineConfig, StageConfig};
    use crate::stage::StageParams;

    #[tokio::test]
    async fn test_runtime_creation() {
        let registry = Arc::new(DataPlaneStageRegistry::new());
        let runtime = PipelineRuntime::new(registry);
        
        assert_eq!(runtime.state().await, PipelineState::Idle);
        assert!(runtime.graph().await.is_none());
    }

    #[tokio::test]
    async fn test_pipeline_loading() {
        let registry = Arc::new(DataPlaneStageRegistry::new());
        let mut runtime = PipelineRuntime::new(registry);
        
        let config = PipelineConfig::new(
            "test_pipeline".to_string(),
            Some("Test pipeline".to_string()),
        );
        
        // This will fail because we don't have any registered stage types
        // but it tests the loading mechanism
        let result = runtime.load_pipeline(&config).await;
        assert!(result.is_ok() || matches!(result, Err(PipelineError::UnknownStageType { .. })));
    }
}
}