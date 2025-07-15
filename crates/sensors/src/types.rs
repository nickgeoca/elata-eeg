//! Common types and traits for sensor drivers

use std::error::Error;
use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use eeg_types::{BridgeMsg, SensorError as EegSensorError};

/// Configuration for ADC/sensor drivers
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdcConfig {
    /// Target sample rate in Hz
    pub sample_rate: u32,
    /// List of active channels (0-indexed)
    pub channels: Vec<u8>,
    /// Gain setting for all channels
    pub gain: f32,
    /// Type of board driver to use
    pub board_driver: DriverType,
    /// Number of samples to batch together
    pub batch_size: usize,
    /// Reference voltage for ADC conversion
    pub vref: f32,
}

pub use eeg_types::DriverType;

impl Default for AdcConfig {
    fn default() -> Self {
        Self {
            sample_rate: 250,
            channels: (0..8).collect(),
            gain: 1.0,
            board_driver: DriverType::MockEeg,
            batch_size: 1,
            vref: 4.5,
        }
    }
}

/// Status of a sensor driver
#[derive(Debug, Clone, PartialEq)]
pub enum DriverStatus {
    /// Driver is not initialized
    NotInitialized,
    /// Driver is initialized but not running
    Stopped,
    /// Driver is actively acquiring data
    Running,
    /// Driver is OK/ready
    Ok,
    /// Driver encountered an error
    Error(String),
}

/// Events emitted by sensor drivers
use eeg_types::Packet;

#[derive(Debug)]
pub enum DriverEvent {
    /// New data is available
    Data(Packet<i32>),
    /// Driver status changed
    StatusChange(DriverStatus),
    /// An error occurred
    Error(String),
}

/// Errors that can occur in sensor drivers
#[derive(Debug, Clone)]
pub enum DriverError {
    /// A sensor-specific error.
    SensorError(EegSensorError),
    /// Hardware communication error
    HardwareError(String),
    /// Invalid configuration
    ConfigurationError(String),
    /// SPI communication error
    SpiError(String),
    /// GPIO error
    GpioError(String),
    /// Timeout error
    TimeoutError(String),
    /// I/O error
    IoError(String),
    /// Driver not initialized
    NotInitialized,
    /// Driver not configured
    NotConfigured,
    /// Hardware not found
    HardwareNotFound(String),
    /// Acquisition error
    AcquisitionError(String),
    /// Generic error
    Other(String),
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DriverError::SensorError(e) => write!(f, "Sensor error: {}", e),
            DriverError::HardwareError(msg) => write!(f, "Hardware error: {}", msg),
            DriverError::ConfigurationError(msg) => write!(f, "Configuration error: {}", msg),
            DriverError::SpiError(msg) => write!(f, "SPI error: {}", msg),
            DriverError::GpioError(msg) => write!(f, "GPIO error: {}", msg),
            DriverError::TimeoutError(msg) => write!(f, "Timeout error: {}", msg),
            DriverError::IoError(msg) => write!(f, "I/O error: {}", msg),
            DriverError::NotInitialized => write!(f, "Driver not initialized"),
            DriverError::NotConfigured => write!(f, "Driver not configured"),
            DriverError::HardwareNotFound(msg) => write!(f, "Hardware not found: {}", msg),
            DriverError::AcquisitionError(msg) => write!(f, "Acquisition error: {}", msg),
            DriverError::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl Error for DriverError {}

/// Trait that all sensor drivers must implement
pub trait AdcDriver: Send + Sync + 'static {
    /// Start data acquisition (new synchronous method)
    fn acquire(
        &mut self,
        tx: Sender<BridgeMsg>,
        stop_flag: &AtomicBool,
    ) -> Result<(), EegSensorError>;

    /// Get current driver status
    fn get_status(&self) -> DriverStatus;

    /// Get current configuration
    fn get_config(&self) -> Result<AdcConfig, DriverError>;

    /// Shutdown the driver and clean up resources
    fn shutdown(&mut self) -> Result<(), DriverError>;
}

/// Factory function to create drivers based on configuration
pub fn create_driver(config: AdcConfig) -> Result<Box<dyn AdcDriver>, Box<dyn Error>> {
    match config.board_driver {
        DriverType::Ads1299 => {
            let driver = crate::ads1299::driver::Ads1299Driver::new(config)?;
            Ok(Box::new(driver))
        }
        DriverType::MockEeg => {
            let driver = crate::mock_eeg::driver::MockDriver::new(config)?;
            Ok(Box::new(driver))
        }
    }
}