//! Pipeline graph construction and management.

use crate::config::SystemConfig;
use crate::control::ControlCommand;
use crate::error::StageError;
use crate::registry::StageRegistry;
use crate::stage::{Stage, StageContext};
use eeg_types::Packet;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

/// Represents a node in the pipeline graph, containing a stage instance and its I/O channels.
pub struct PipelineNode<T> {
    pub name: String,
    pub stage: Arc<Mutex<Box<dyn Stage<T, T>>>>,
    pub rx: Option<broadcast::Receiver<Packet<T>>>,
    pub tx: broadcast::Sender<Packet<T>>,
}

impl<T> Drop for PipelineNode<T> {
    fn drop(&mut self) {
        tracing::debug!("Dropping PipelineNode: {}", self.name);
    }
}
/// Represents the entire pipeline as a graph of connected stages.
pub struct PipelineGraph<T> {
    pub nodes: HashMap<String, Arc<PipelineNode<T>>>,
    pub entry_points: Vec<String>,
    pub context: StageContext,
    _marker: PhantomData<T>,
}

impl<T> Drop for PipelineGraph<T> {
    fn drop(&mut self) {
        tracing::debug!("Dropping PipelineGraph");
    }
}

impl<T: Clone + Send + 'static> PipelineGraph<T> {
    /// Builds a new `PipelineGraph` from a configuration and a stage registry.
    pub async fn build(
        config: &SystemConfig,
        registry: &StageRegistry<T, T>,
        context: StageContext,
    ) -> Result<Self, StageError> {
        let mut nodes: HashMap<String, Arc<PipelineNode<T>>> = HashMap::new();
        let mut entry_points = Vec::new();

        // First pass: Create all output channels and store them.
        let output_bus: HashMap<String, broadcast::Sender<Packet<T>>> = config
            .stages
            .iter()
            .map(|stage_config| {
                let (tx, _) = broadcast::channel(1024);
                (stage_config.name.clone(), tx)
            })
            .collect();

        // Second pass: Create nodes and wire them up.
        for stage_config in &config.stages {
            if nodes.contains_key(&stage_config.name) {
                return Err(StageError::BadConfig(format!(
                    "Duplicate stage name found: {}",
                    stage_config.name
                )));
            }

            let stage = registry.create_stage(stage_config).await?;
            let tx = output_bus.get(&stage_config.name).unwrap().clone();

            let rx = if let Some(source_name) = stage_config.inputs.first() {
                let source_tx = output_bus.get(source_name).ok_or_else(|| {
                    StageError::BadConfig(format!(
                        "Stage '{}' references an unknown input '{}'",
                        stage_config.name, source_name
                    ))
                })?;
                Some(source_tx.subscribe())
            } else {
                entry_points.push(stage_config.name.clone());
                None
            };

            let node = Arc::new(PipelineNode {
                name: stage_config.name.clone(),
                stage: Arc::new(Mutex::new(stage)),
                rx,
                tx,
            });
            nodes.insert(stage_config.name.clone(), node);
        }

        Ok(Self {
            nodes,
            entry_points,
            context,
            _marker: PhantomData,
        })
    }

    pub async fn forward_control_command(&mut self, cmd: ControlCommand) {
        for node in self.nodes.values() {
            let stage = node.stage.clone();
            let mut context = self.context.clone();
            let mut stage = stage.lock().await;
            if let Err(e) = stage.control(&cmd, &mut context).await {
                tracing::error!("Error sending control command to stage {}: {}", stage.id(), e);
            }
        }
    }
}