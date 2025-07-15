//! Core definitions for pipeline stages.

use crate::control::{ControlCommand, PipelineEvent};
use crate::data::Packet;
use crate::error::StageError;
use crossbeam_channel::Sender;

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
    pub fn emit_event(&self, event: PipelineEvent) -> Result<(), StageError> {
        self.event_tx
            .send(event)
            .map_err(|e| StageError::SendError(e.to_string()))
    }
}

/// The core trait for a processing stage in the pipeline.
///
/// A stage is a component that receives packets of one type (`I`), processes
/// them, and outputs packets of another type (`O`). It can also respond to
pub trait Stage: Send + Sync {
    /// A unique identifier for this stage instance.
    fn id(&self) -> &str;

    /// Processes an input packet and returns an optional output packet.
    fn process(
        &mut self,
        packet: Packet,
        ctx: &mut StageContext,
    ) -> Result<Option<Packet>, StageError>;

    /// Handles a control command sent to the pipeline.
    /// The default implementation does nothing, allowing stages to opt-in.
    fn control(
        &mut self,
        _cmd: &ControlCommand,
        _ctx: &mut StageContext,
    ) -> Result<(), StageError> {
        Ok(())
    }

    /// Called when the stage is being shut down.
    fn shutdown(&mut self, _ctx: &mut StageContext) -> Result<(), StageError> {
        Ok(()) // Default implementation does nothing.
    }

    /// Returns this stage as a mutable `Drains` trait object if it implements it.
    fn as_drains(&mut self) -> Option<&mut dyn Drains> {
        None
    }
}

/// A trait for stages that need to flush internal buffers before shutdown.
///
/// This is typically implemented by "sink" stages that write to files or network
/// sockets, ensuring that all data is persisted before the pipeline terminates.
pub trait Drains {
    /// Flushes any internal buffers to their destination.
    fn flush(&mut self) -> std::io::Result<()>;
}

// Implement `Stage` for `Box<dyn Stage>` to allow for dynamic dispatch.
impl<T: Stage + ?Sized> Stage for Box<T> {
    fn id(&self) -> &str {
        (**self).id()
    }

    fn process(
        &mut self,
        packet: Packet,
        ctx: &mut StageContext,
    ) -> Result<Option<Packet>, StageError> {
        (**self).process(packet, ctx)
    }

    fn control(
        &mut self,
        cmd: &ControlCommand,
        ctx: &mut StageContext,
    ) -> Result<(), StageError> {
        (**self).control(cmd, ctx)
    }

    fn shutdown(&mut self, ctx: &mut StageContext) -> Result<(), StageError> {
        (**self).shutdown(ctx)
    }

    fn as_drains(&mut self) -> Option<&mut dyn Drains> {
        (**self).as_drains()
    }
}
