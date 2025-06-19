//! Built-in plugins for the EEG daemon
//!
//! This module contains plugins that are built into the daemon itself,
//! as opposed to external plugin crates.

pub mod brain_waves;

pub use brain_waves::{BrainWavesPlugin, BrainWavesConfig};