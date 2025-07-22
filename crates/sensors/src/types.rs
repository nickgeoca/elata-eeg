//! Common types and traits for sensor drivers

use std::error::Error;
use std::fmt;
use std::sync::atomic::AtomicBool;

use eeg_types::SensorError as EegSensorError;

/// Configuration for ADC/sensor drivers
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChipConfig {
    /// List of active channels for this chip (0-indexed for this chip)
    pub channels: Vec<u8>,
    /// Gain setting for all channels on this chip
    pub gain: f32,
    /// SPI bus for this chip
    #[serde(default = "default_spi_bus")]
    pub spi_bus: u8,
    /// SPI chip select for this chip
    #[serde(default = "default_cs_pin")]
    pub cs_pin: u8,
}

fn default_spi_bus() -> u8 { 0 }
fn default_cs_pin() -> u8 { 0 }

impl Default for ChipConfig {
    fn default() -> Self {
        Self {
            channels: (0..8).collect(),
            gain: 1.0,
            spi_bus: default_spi_bus(),
            cs_pin: default_cs_pin(),
        }
    }
}

/// Configuration for ADC/sensor drivers
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdcConfig {
    /// Target sample rate in Hz
    pub sample_rate: u32,
    /// Reference voltage for ADC conversion
    pub vref: f32,
    /// Data ready pin for the first chip in the daisy chain
    #[serde(default = "default_drdy_pin")]
    pub drdy_pin: u8,
    /// Configuration for each chip on the board.
    pub chips: Vec<ChipConfig>,
}

fn default_drdy_pin() -> u8 { 25 }

impl Default for AdcConfig {
    fn default() -> Self {
        Self {
            sample_rate: 250,
            vref: 4.5,
            chips: vec![ChipConfig::default()],
            drdy_pin: default_drdy_pin(),
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

impl From<rppal::gpio::Error> for DriverError {
    fn from(e: rppal::gpio::Error) -> Self {
        DriverError::GpioError(e.to_string())
    }
}

impl From<rppal::spi::Error> for DriverError {
    fn from(e: rppal::spi::Error) -> Self {
        DriverError::SpiError(e.to_string())
    }
}

/// Trait that all sensor drivers must implement
pub trait AdcDriver: Send + Sync + 'static {
    /// Initialize the driver and underlying hardware.
    fn initialize(&mut self) -> Result<(), DriverError>;

    /// Acquire a batch of raw i32 samples from the sensor.
    /// This is a blocking call that should wait for data to be ready.
    ///
    /// # Arguments
    /// * `batch_size` - The number of samples to acquire for each channel.
    /// * `stop_flag` - An atomic bool to signal the acquisition loop to stop.
    ///
    /// # Returns
    /// A vector containing the raw i32 samples, interleaved by channel.
    fn acquire_batched(
        &mut self,
        batch_size: usize,
        stop_flag: &AtomicBool,
    ) -> Result<(Vec<i32>, u64), EegSensorError>;

    /// Get current driver status
    fn get_status(&self) -> DriverStatus;

    /// Get current configuration
    fn get_config(&self) -> Result<AdcConfig, DriverError>;

    /// Shutdown the driver and clean up resources
    fn shutdown(&mut self) -> Result<(), DriverError>;
}
