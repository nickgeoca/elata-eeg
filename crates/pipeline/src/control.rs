//! Defines the control commands for the pipeline.

use crate::config::SystemConfig;

/// Commands that can be sent to a running pipeline to alter its state.
#[derive(Debug, Clone)]
pub enum ControlCommand {
    /// Pause data processing.
    Pause,
    /// Resume data processing.
    Resume,
    /// Initiate a graceful shutdown of the pipeline.
    Shutdown,
    /// Replace the current system configuration with a new one.
    Reconfigure(SystemConfig),
    /// (For testing) Set the state of a test stage.
    SetTestState(u32),
}

/// Events sent from the pipeline back to the control plane (e.g., the `device` crate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineEvent {
    /// Acknowledges that the pipeline has completed its shutdown sequence.
    ShutdownAck,
    /// (For testing) Confirms a test stage's state has changed.
    TestStateChanged(u32),
}