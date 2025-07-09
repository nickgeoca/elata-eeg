//! Pipeline runtime for executing pipeline graphs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, error, debug};
use uuid::Uuid;

use crate::config::PipelineConfig;
use crate::data::PipelineData;
use crate::error::{PipelineError, PipelineResult};
use crate::graph::{GraphBuilder, GraphState, PipelineGraph};
use crate::stage::{StageInstance, StageRegistry, StageState, StageMetric, PipelineStage};

/// Pipeline runtime for executing pipeline graphs
pub struct PipelineRuntime {
    /// Runtime ID
    pub id: Uuid,
    /// Stage registry for creating stage instances
    registry: Arc<StageRegistry>,
    /// Currently loaded pipeline graph
    graph: Option<Arc<RwLock<PipelineGraph>>>,
    /// Running stage tasks
    stage_tasks: HashMap<String, JoinHandle<PipelineResult<()>>>,
    /// Stage channels for data flow
    stage_channels: HashMap<String, StageChannelSet>,
    /// Global cancellation token
    cancellation_token: CancellationToken,
    /// Runtime state
    state: RuntimeState,
    /// Runtime metrics
    metrics: Arc<tokio::sync::Mutex<RuntimeMetrics>>,
    /// Metrics sender for collecting stage metrics
    metrics_sender: mpsc::UnboundedSender<StageMetric>,
    /// Metrics receiver for collecting stage metrics
    metrics_receiver: mpsc::UnboundedReceiver<StageMetric>,
}

/// Runtime state
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
#[derive(Debug, Clone)]
pub struct RuntimeMetrics {
    /// Total number of data items processed
    pub items_processed: u64,
    /// Total number of errors encountered
    pub error_count: u64,
    /// Runtime uptime in milliseconds
    pub uptime_ms: u64,
    /// Start time
    pub start_time: Option<std::time::Instant>,
    /// Stage-specific metrics
    pub stage_metrics: HashMap<String, Vec<StageMetric>>,
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
    /// Create a new pipeline runtime
    pub fn new(registry: Arc<StageRegistry>) -> Self {
        let (metrics_sender, metrics_receiver) = mpsc::unbounded_channel();
        Self {
            id: Uuid::new_v4(),
            registry,
            graph: None,
            stage_tasks: HashMap::new(),
            stage_channels: HashMap::new(),
            cancellation_token: CancellationToken::new(),
            state: RuntimeState::Idle,
            metrics: Arc::new(tokio::sync::Mutex::new(RuntimeMetrics::new())),
            metrics_sender,
            metrics_receiver,
        }
    }

    /// Load a pipeline configuration
    pub async fn load_pipeline(&mut self, config: &PipelineConfig) -> PipelineResult<()> {
        if self.state == RuntimeState::Running {
            return Err(PipelineError::AlreadyRunning);
        }

        self.state = RuntimeState::Loading;
        info!("Loading pipeline: {}", config.metadata.name);

        // Build the graph
        let builder = GraphBuilder::new(self.registry.clone());
        let graph = builder.build(config).await?;

        info!("Pipeline graph built with {} stages", graph.stages.len());
        debug!("Graph sources: {:?}", graph.sources);
        debug!("Graph sinks: {:?}", graph.sinks);

        // Store the graph
        self.graph = Some(Arc::new(RwLock::new(graph)));
        self.state = RuntimeState::Idle;

        Ok(())
    }

    /// Start pipeline execution
    pub async fn start(&mut self) -> PipelineResult<()> {
        if self.state == RuntimeState::Running {
            return Err(PipelineError::AlreadyRunning);
        }

        let graph = self.graph.clone()
            .ok_or_else(|| PipelineError::InvalidConfiguration {
                message: "No pipeline loaded".to_string(),
            })?;

        self.state = RuntimeState::Starting;
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

        self.state = RuntimeState::Running;
        {
            let mut metrics = self.metrics.lock().await;
            metrics.start_time = Some(std::time::Instant::now());
        }
        info!("Pipeline started successfully");

        Ok(())
    }

    /// Stop pipeline execution
    pub async fn stop(&mut self) -> PipelineResult<()> {
        if self.state != RuntimeState::Running {
            return Err(PipelineError::NotRunning);
        }

        self.state = RuntimeState::Stopping;
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

        // Update graph state
        if let Some(graph) = &self.graph {
            let mut graph_guard = graph.write().await;
            graph_guard.set_state(GraphState::Idle);
            graph_guard.set_all_stage_states(StageState::Idle);
            graph_guard.unlock_all_stages();
        }

        self.state = RuntimeState::Idle;
        info!("Pipeline stopped successfully");

        Ok(())
    }

    /// Get current runtime state
    pub fn state(&self) -> &RuntimeState {
        &self.state
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

            // Create the actual stage implementation
            let mut stage = self.registry.create_stage(&stage_instance.stage_type, &stage_instance.params).await?;

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

    /// Run a single stage
    async fn run_stage(
        mut stage: Box<dyn PipelineStage>,
        mut context: ExecutionContext,
    ) -> PipelineResult<()> {
        info!("Starting stage: {}", context.stage_name);

        // Initialize the stage
        stage.initialize().await?;

        // Main processing loop
        loop {
            tokio::select! {
                // Check for cancellation
                _ = context.cancellation_token.cancelled() => {
                    info!("Stage '{}' received cancellation signal", context.stage_name);
                    break;
                }
                
                // Process input data from any available input channel
                input_data = Self::receive_input_data(&mut context.inputs) => {
                    match input_data {
                        Some(data) => {
                            // Process the data through the stage
                            match stage.process(data).await {
                                Ok(output) => {
                                    // Send metrics about processed item
                                    let _ = context.metrics_sender.send(StageMetric {
                                        name: "items_processed".to_string(),
                                        value: 1.0,
                                        unit: "count".to_string(),
                                        description: Some("Number of items processed".to_string()),
                                        timestamp: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                    });
                                    
                                    // Send output to all connected downstream stages
                                    if let Err(e) = Self::send_output_data(&context.outputs, output).await {
                                        error!("Failed to send output from stage '{}': {}", context.stage_name, e);
                                        // Continue processing - don't fail the entire stage for send errors
                                    }
                                }
                                Err(e) => {
                                    error!("Stage '{}' processing error: {}", context.stage_name, e);
                                    // Send error metric
                                    let _ = context.metrics_sender.send(StageMetric {
                                        name: "errors".to_string(),
                                        value: 1.0,
                                        unit: "count".to_string(),
                                        description: Some("Number of processing errors".to_string()),
                                        timestamp: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                    });
                                    // For now, continue processing. In the future, we might want
                                    // configurable error handling (fail-fast vs continue)
                                }
                            }
                        }
                        None => {
                            // No input data available, yield to prevent busy waiting
                            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                        }
                    }
                }
            }
        }

        // Cleanup the stage
        stage.cleanup().await?;
        info!("Stage '{}' stopped", context.stage_name);

        Ok(())
    }

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
}

impl RuntimeMetrics {
    /// Create new runtime metrics
    fn new() -> Self {
        Self {
            items_processed: 0,
            error_count: 0,
            uptime_ms: 0,
            start_time: None,
            stage_metrics: HashMap::new(),
        }
    }

    /// Update uptime based on start time
    pub fn update_uptime(&mut self) {
        if let Some(start_time) = self.start_time {
            self.uptime_ms = start_time.elapsed().as_millis() as u64;
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
        let registry = Arc::new(StageRegistry::new());
        let runtime = PipelineRuntime::new(registry);
        
        assert_eq!(runtime.state(), &RuntimeState::Idle);
        assert!(runtime.graph().await.is_none());
    }

    #[tokio::test]
    async fn test_pipeline_loading() {
        let registry = Arc::new(StageRegistry::new());
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