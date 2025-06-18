//! Elata EMU v1 product-specific integration
//!
//! This module contains the product-specific code for the Elata EMU v1 device,
//! including hardware configuration, board integration, and EEG system orchestration.

pub mod eeg_system;

// Re-export the main EEG system for convenience
pub use eeg_system::EegSystem;