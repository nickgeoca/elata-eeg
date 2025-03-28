use std::time::SystemTime;
use std::error::Error;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::mpsc;
use std::future::Future;
use std::pin::Pin;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use log::{info, warn, debug, trace, error};
use super::mock_driver::MockDriver;

// Driver events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DriverEvent {
    Data(Vec<AdcData>),
    Error(String),
    StatusChange(DriverStatus),
}

// Driver status
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DriverStatus {
    NotInitialized,
    Ok,
    Error,
    Stopped,
    Running,
}

// Fix DriverType enum to match create_driver usage
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DriverType {
    Ads1299,
    Mock,
}

// ADC configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdcConfig {
    pub sample_rate: u32,
    pub gain: f32,
    pub channels: Vec<usize>,
    pub board_driver: DriverType,
    pub batch_size: usize,  // Number of samples to collect in a batch
    pub Vref: f32,  // Vref of adc
    /// High-pass filter cutoff frequency in Hz
    pub dsp_high_pass_cutoff_hz: f32,
    /// Low-pass filter cutoff frequency in Hz
    pub dsp_low_pass_cutoff_hz: f32,
}

impl Default for AdcConfig {
    fn default() -> Self {
        Self {
            sample_rate: 250,  // 250 Hz is a common EEG sampling rate
            gain: 1.0,
            channels: vec![0],
            board_driver: DriverType::Mock,
            batch_size: 32,    // Default batch size (typical SPI buffer size)
            Vref: 4.5,         // Vref for the ADC
            dsp_high_pass_cutoff_hz: 0.1,  // Default high-pass filter cutoff (Hz)
            dsp_low_pass_cutoff_hz: 100.0, // Default low-pass filter cutoff (Hz)
        }
    }
}

// ADC data point
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdcData {
    pub timestamp: u64,
    pub raw_samples: Vec<Vec<i32>>,
    pub voltage_samples: Vec<Vec<f32>>,
}

// Driver error
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("Hardware not found")]
    HardwareNotFound(String),
    
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    
    #[error("Acquisition error: {0}")]
    AcquisitionError(String),
    
    #[error("Driver not initialized")]
    NotInitialized,
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Other error: {0}")]
    Other(String),
    
    #[error("Driver not configured")]
    NotConfigured,
}

// Remove the problematic From implementations that violate orphan rules
// Instead, create wrapper types for external errors
#[derive(Debug)]
#[cfg(feature = "pi-hardware")]
pub struct SpiError(rppal::spi::Error);

#[cfg(not(feature = "pi-hardware"))]
#[derive(Debug)]
pub struct SpiError(String);

#[derive(Debug)]
pub struct TimeError(std::time::SystemTimeError);

#[cfg(feature = "pi-hardware")]
impl From<rppal::spi::Error> for SpiError {
    fn from(err: rppal::spi::Error) -> Self {
        SpiError(err)
    }
}

impl From<SpiError> for DriverError {
    fn from(err: SpiError) -> Self {
        #[cfg(feature = "pi-hardware")]
        {
            DriverError::Other(err.0.to_string())
        }
        #[cfg(not(feature = "pi-hardware"))]
        {
            DriverError::Other(err.0)
        }
    }
}

impl From<TimeError> for DriverError {
    fn from(err: TimeError) -> Self {
        DriverError::Other(err.0.to_string())
    }
}


#[async_trait]
pub trait AdcDriver: Send + Sync + 'static {
    async fn start_acquisition(&mut self) -> Result<(), DriverError>;
    async fn stop_acquisition(&mut self) -> Result<(), DriverError>;
    
    async fn shutdown(&mut self) -> Result<(), DriverError>;

    async fn get_config(&self) -> Result<AdcConfig, DriverError>;
    async fn get_status(&self) -> DriverStatus;
}

// Factory function to create the appropriate driver and return the event channel
pub async fn create_driver(config: AdcConfig)
    -> Result<(Box<dyn AdcDriver>, mpsc::Receiver<DriverEvent>), DriverError> {
    
    match config.board_driver {
        DriverType::Ads1299 => {
            #[cfg(feature = "pi-hardware")]
            {
                // Try to create real hardware HAL implementations
                info!("Attempting to create ADS1299 driver with hardware HAL");
                match (
                    super::rppal_hal::rppal_impl::create_spi(),
                    super::rppal_hal::rppal_impl::create_drdy()
                ) {
                    (Ok(spi_impl), Ok(drdy_impl)) => {
                        let spi = Box::new(spi_impl);
                        let drdy = Box::new(drdy_impl);
                        
                        match super::ads1299_driver::Ads1299Driver::new_with_hal(
                            spi, drdy, config.clone(), 0
                        ) {
                            Ok((driver, rx)) => {
                                // Check if the driver is in error state after creation
                                if driver.get_status().await == DriverStatus::Error {
                                    warn!("ADS1299 driver is in error state after creation, falling back to MockDriver");
                                } else {
                                    info!("ADS1299 driver created successfully with hardware HAL");
                                    return Ok((Box::new(driver), rx));
                                }
                            },
                            Err(e) => {
                                warn!("Ads1299Driver failed with hardware HAL: {}, falling back to MockDriver", e);
                            }
                        }
                    },
                    _ => {
                        warn!("Failed to initialize hardware HAL, falling back to MockDriver");
                    }
                }
            }

            // Fallback (either pi-hardware failed, or not enabled)
            #[cfg(not(feature = "pi-hardware"))]
            info!("pi-hardware feature not enabled, using MockDriver");

            // Create mock driver as fallback
            info!("Creating mock driver as fallback");
            let mut mock_config = config.clone();
            mock_config.board_driver = DriverType::Mock;
            let (mock_driver, mock_events) = super::mock_driver::MockDriver::new(mock_config, 0)?;
            Ok((Box::new(mock_driver), mock_events))
        },
        DriverType::Mock => {
            info!("Creating mock driver");
            let (driver, events) = super::mock_driver::MockDriver::new(config, 0)?;
            Ok((Box::new(driver), events))
        }
    }
}
