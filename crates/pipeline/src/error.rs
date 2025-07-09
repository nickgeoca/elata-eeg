//! Error types for the pipeline system

use thiserror::Error;

/// Pipeline-specific error types
#[derive(Error, Debug)]
pub enum PipelineError {
    #[error("Stage not found: {name}")]
    StageNotFound { name: String },

    #[error("Circular dependency detected in pipeline graph")]
    CircularDependency,

    #[error("Invalid stage configuration: {message}")]
    InvalidConfiguration { message: String },

    #[error("Stage type not registered: {stage_type}")]
    UnknownStageType { stage_type: String },

    #[error("Channel error: {0}")]
    ChannelError(String),

    #[error("Runtime error in stage '{stage_name}': {message}")]
    RuntimeError { stage_name: String, message: String },

    #[error("Pipeline is already running")]
    AlreadyRunning,

    #[error("Pipeline is not running")]
    NotRunning,

    #[error("Stage '{stage_name}' is locked and cannot be modified")]
    StageLocked { stage_name: String },

    #[error("Processing error: {message}")]
    ProcessingError { message: String },

    #[error("Invalid input: {message}")]
    InvalidInput { message: String },

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Generic error: {0}")]
    Other(#[from] anyhow::Error),
}

/// Result type for pipeline operations
pub type PipelineResult<T> = Result<T, PipelineError>;