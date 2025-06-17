pub mod board_drivers;
pub mod eeg_system;

// Re-export the main types that users need
pub use eeg_system::EegSystem;
pub use board_drivers::types::{AdcConfig, DriverType, DriverStatus};
pub use board_drivers::{AdcData, DriverEvent, DriverError};

// Optionally expose lower-level access through a raw module
pub mod raw {
    pub use crate::board_drivers::*;
}