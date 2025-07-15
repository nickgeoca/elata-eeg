pub mod types;
pub mod ads1299;
pub mod mock_eeg;

// Re-export the main types that users need
pub use types::{AdcConfig, DriverType, DriverStatus, DriverEvent, DriverError, AdcDriver};

// Optionally expose lower-level access through a raw module
pub mod raw {
    pub mod ads1299 {
        pub use crate::ads1299::driver::Ads1299Driver;
    }
    pub mod mock_eeg {
        pub use crate::mock_eeg::driver::MockDriver;
    }
}