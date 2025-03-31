pub mod board_driver;
pub mod dsp;
pub mod eeg_system;

// Re-export the main types that users need
pub use eeg_system::EegSystem;
pub use board_driver::types::{AdcConfig, DriverType, DriverStatus};
use serde::{Serialize, Deserialize};

/// Processed EEG data structure
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessedData {
    pub timestamp: u64,
    pub raw_samples: Vec<Vec<i32>>,
    pub processed_voltage_samples: Vec<Vec<f32>>,
    /// Optional error message if processing failed
    pub error: Option<String>,
}

impl Default for ProcessedData {
    fn default() -> Self {
        Self {
            timestamp: 0,
            raw_samples: Vec::new(),
            processed_voltage_samples: Vec::new(),
            error: None,
        }
    }
}

// Optionally expose lower-level access through a raw module
pub mod raw {
    pub use crate::board_driver::*;
    pub use crate::dsp::*;
}