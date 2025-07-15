//! Core definitions for pipeline stages.

use crate::control::{ControlCommand, PipelineEvent};
use crate::error::StageError;
use async_trait::async_trait;
use tokio::sync::mpsc::Sender;
use eeg_types::Packet;

/// A context object passed to each stage's `process` method.
///
/// It provides access to shared resources and control mechanisms, like the
/// ability to emit events back to the main control loop.
#[derive(Clone)]
pub struct StageContext {
    pub event_tx: Sender<PipelineEvent>,
}

impl StageContext {
    pub fn new(event_tx: Sender<PipelineEvent>) -> Self {
        Self { event_tx }
    }

    /// Emits an event to the main control loop.
    pub async fn emit_event(&self, event: PipelineEvent) -> Result<(), StageError> {
        self.event_tx
            .send(event)
            .await
            .map_err(|e| StageError::SendError(e.to_string()))
    }
}

/// The core trait for a processing stage in the pipeline.
///
/// A stage is a component that receives packets of one type (`I`), processes
/// them, and outputs packets of another type (`O`). It can also respond to
/// control commands.
#[async_trait]
pub trait Stage<I = f32, O = f32>: Send + Sync {
    /// A unique identifier for this stage instance.
    fn id(&self) -> &str;

    /// Processes an input packet and returns an optional output packet.
    async fn process(
        &mut self,
        packet: Packet<I>,
        ctx: &mut StageContext,
    ) -> Result<Option<Packet<O>>, StageError>;

    /// Handles a control command sent to the pipeline.
    /// The default implementation does nothing, allowing stages to opt-in.
    async fn control(
        &mut self,
        _cmd: &ControlCommand,
        _ctx: &mut StageContext,
    ) -> Result<(), StageError> {
        Ok(())
    }

    /// Called when the stage is being shut down.
    async fn shutdown(&mut self, _ctx: &mut StageContext) -> Result<(), StageError> {
        Ok(()) // Default implementation does nothing.
    }
}
