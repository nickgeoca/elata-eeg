//! EEG Daemon Plugins
//!
//! This module serves as the central integration point for all EEG plugins,
//! whether they are built-in or external crates.

// Built-in plugins are declared as modules
pub mod brain_waves;

// Re-export all plugin types for easy access by the supervisor.
// This abstracts away whether a plugin is built-in or external.
pub use brain_waves::BrainWavesPlugin;