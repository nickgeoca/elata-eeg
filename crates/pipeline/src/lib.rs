//! Pipeline Graph Architecture for EEG Data Processing
//!
//! This crate implements a dataflow graph (DAG) architecture for EEG data processing,
//! replacing the event-bus-based plugin system with explicit pipeline stages and
//! data flow contracts.

pub mod stage;
pub mod graph;
pub mod config;
pub mod runtime;
pub mod stages;
pub mod error;
pub mod data;
pub mod queue;
pub mod control;
#[macro_use]
pub mod macros;

#[cfg(test)]
mod tests;

// Re-export commonly used types
pub use stage::*;
pub use graph::*;
pub use config::*;
pub use runtime::*;
pub use stages::*;
pub use error::*;
pub use data::*;