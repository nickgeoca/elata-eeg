//! Defines the control commands for the pipeline.

use crate::config::SystemConfig;
use std::any::Any;
use std::fmt::Debug;

/// A trait for custom commands that can be cloned.
pub trait CustomCommand: Any + Send + Debug {
    fn as_any(&self) -> &dyn Any;
    fn clone_box(&self) -> Box<dyn CustomCommand>;
}

impl<T: Any + Send + Clone + Debug> CustomCommand for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn CustomCommand> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn CustomCommand> {
    fn clone(&self) -> Box<dyn CustomCommand> {
        self.clone_box()
    }
}

/// Commands that can be sent to a running pipeline to alter its state.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Serialize, Deserialize)]
pub enum ControlCommand {
    /// Start the pipeline.
    Start,
    /// Pause data processing.
    Pause,
    /// Resume data processing.
    Resume,
    /// Initiate a graceful shutdown of the pipeline.
    Shutdown,
    /// Tell a sink stage to start recording.
    StartRecording,
    /// Tell a sink stage to stop recording.
    StopRecording,
    /// Replace the current system configuration with a new one.
    Reconfigure(SystemConfig),
    /// Set a parameter on a specific stage
    SetParameter {
        target_stage: String,
        parameters: Value,
    },
    /// (For testing) Set the state of a test stage.
    SetTestState(u32),
    /// A custom command for a specific stage.
    #[serde(skip)]
    Custom(Box<dyn CustomCommand>),
}

impl Debug for ControlCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ControlCommand::Start => write!(f, "Start"),
            ControlCommand::Pause => write!(f, "Pause"),
            ControlCommand::Resume => write!(f, "Resume"),
            ControlCommand::Shutdown => write!(f, "Shutdown"),
            ControlCommand::StartRecording => write!(f, "StartRecording"),
            ControlCommand::StopRecording => write!(f, "StopRecording"),
            ControlCommand::Reconfigure(config) => f.debug_tuple("Reconfigure").field(config).finish(),
            ControlCommand::SetParameter { target_stage, parameters } => f.debug_struct("SetParameter").field("target_stage", target_stage).field("parameters", parameters).finish(),
            ControlCommand::SetTestState(state) => f.debug_tuple("SetTestState").field(state).finish(),
            ControlCommand::Custom(cmd) => f.debug_tuple("Custom").field(cmd).finish(),
        }
    }
}

/// Events sent from the pipeline back to the control plane (e.g., the `device` crate).
#[derive(Debug, PartialEq)]
pub enum PipelineEvent {
    /// Acknowledges that the pipeline has completed its shutdown sequence.
    ShutdownAck,
    /// (For testing) Confirms a test stage's state has changed.
    TestStateChanged(u32),
}