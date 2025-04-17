//! Main driver implementation for the ADS1299 chip.

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use log::{info, warn, debug, error};
use lazy_static::lazy_static;

use crate::board_drivers::types::{AdcConfig, AdcData, DriverStatus, DriverError, DriverEvent, DriverType};
use super::acquisition::{start_acquisition, stop_acquisition, InterruptData, SpiType, DrdyPinType};
use super::error::HardwareLockGuard;
use super::helpers::current_timestamp_micros;
use super::registers::{CMD_RESET, CMD_SDATAC, CMD_RDATAC};
use super::spi::{SpiDevice, InputPinDevice, init_spi, init_drdy_pin, send_command_to_spi, write_register};

// Static hardware lock to simulate real hardware access constraints
lazy_static! {
    static ref HARDWARE_LOCK: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
}

/// ADS1299 driver for interfacing with the ADS1299EEG_FE board.
pub struct Ads1299Driver {
    inner: Arc<Mutex<Ads1299Inner>>,
    task_handle: Option<JoinHandle<()>>,
    tx: mpsc::Sender<DriverEvent>,
    additional_channel_buffering: usize,
    spi: Option<Box<dyn SpiDevice>>,
    drdy_pin: Option<Box<dyn InputPinDevice>>,
    // New fields for interrupt-driven approach
    interrupt_task: Option<tokio::task::JoinHandle<()>>,
    interrupt_running: Arc<AtomicBool>,
}

/// Internal state for the Ads1299Driver.
pub struct Ads1299Inner {
    pub config: AdcConfig,
    pub running: bool,
    pub status: DriverStatus,
    // Base timestamp for calculating sample timestamps (microseconds since epoch)
    pub base_timestamp: Option<u64>,
    // Total samples generated since acquisition started
    pub sample_count: u64,
    // Cache of register values
    pub registers: [u8; 24],
}

impl Ads1299Driver {
    /// Creates a new Ads1299Driver with the given ADC config and optional extra channel buffering.
    /// `additional_channel_buffering` sets extra buffered batches (0 = lowest latency, but may cause backpressure).
    ///
    /// # Note
    /// Call `shutdown()` when done to ensure proper async cleanup; `Drop` only handles basic cleanup.
    /// # Returns
    /// A tuple containing the driver instance and a receiver for driver events.
    ///
    /// # Errors
    /// Returns an error under various conditions
    pub fn new(
        config: AdcConfig,
        additional_channel_buffering: usize
    ) -> Result<(Self, mpsc::Receiver<DriverEvent>), DriverError> {
        // Acquire the hardware lock using RAII guard
        let _hardware_lock_guard = HardwareLockGuard::new(&HARDWARE_LOCK)?;

        // Validate config
        if config.board_driver != DriverType::Ads1299 {
            return Err(DriverError::ConfigurationError(
                "Ads1299Driver requires config.board_driver=DriverType::Ads1299".to_string()
            ));
        }

        // Validate batch size
        if config.batch_size == 0 {
            return Err(DriverError::ConfigurationError(
                "Batch size must be greater than 0".to_string()
            ));
        }

        // Validate batch size relative to channel count
        if config.batch_size < config.channels.len() {
            return Err(DriverError::ConfigurationError(
                format!("Batch size ({}) must be at least equal to the number of channels ({})",
                        config.batch_size, config.channels.len())
            ));
        }

        // Validate total buffer size (prevent excessive memory usage)
        const MAX_BUFFER_SIZE: usize = 10000; // Arbitrary limit to prevent excessive memory usage
        let channel_buffer_size = config.batch_size + additional_channel_buffering;
        if channel_buffer_size > MAX_BUFFER_SIZE {
            return Err(DriverError::ConfigurationError(
                format!("Total buffer size ({}) exceeds maximum allowed ({})",
                        channel_buffer_size, MAX_BUFFER_SIZE)
            ));
        }

        // Initialize SPI
        let spi = match init_spi() {
            Ok(spi) => spi,
            Err(e) => {
                return Err(e);
            }
        };

        // Initialize DRDY pin
        let drdy_pin = match init_drdy_pin() {
            Ok(pin) => pin,
            Err(e) => {
                return Err(e);
            }
        };
        
        // Initialize register cache
        let registers = [0u8; 24];
        
        let inner = Ads1299Inner {
            config: config.clone(),
            running: false,
            status: DriverStatus::NotInitialized,
            base_timestamp: None,
            sample_count: 0,
            registers,
        };
        
        // Create channel with validated buffer size
        let (tx, rx) = mpsc::channel(channel_buffer_size);
        
        let driver = Ads1299Driver {
            inner: Arc::new(Mutex::new(inner)),
            task_handle: None,
            tx,
            additional_channel_buffering,
            spi: Some(spi),
            drdy_pin: Some(drdy_pin),
            interrupt_task: None,
            interrupt_running: Arc::new(AtomicBool::new(false)),
        };
        
        info!("Ads1299Driver created with config: {:?}", config);
        info!("Channel buffer size: {} (batch_size: {} + additional_buffering: {})",
              channel_buffer_size, config.batch_size, additional_channel_buffering);
        
        Ok((driver, rx))
    }
    
    /// Return the current configuration.
    pub(crate) async fn get_config(&self) -> Result<AdcConfig, DriverError> {
        let inner = self.inner.lock().await;
        Ok(inner.config.clone())
    }

    /// Start data acquisition from the ADS1299.
    ///
    /// This method validates the driver state, initializes the ADS1299 chip,
    /// and spawns a background task that reads data from the chip.
    pub(crate) async fn start_acquisition(&mut self) -> Result<(), DriverError> {
        let result = start_acquisition(
            self.inner.clone(),
            self.tx.clone(),
            self.interrupt_running.clone(),
            self.spi.take(),
            self.drdy_pin.take(),
        ).await?;
        
        self.interrupt_task = result.0;
        self.task_handle = result.1;
        self.spi = Some(result.2);
        self.drdy_pin = Some(result.3);
        
        Ok(())
    }

    /// Stop data acquisition from the ADS1299.
    ///
    /// This method signals the acquisition task to stop, waits for it to complete,
    /// and updates the driver status.
    pub(crate) async fn stop_acquisition(&mut self) -> Result<(), DriverError> {
        stop_acquisition(
            self.inner.clone(),
            &self.tx,
            &self.interrupt_running,
            &mut self.interrupt_task,
            &mut self.task_handle,
            &mut self.spi,
        ).await
    }

    /// Return the current driver status.
    ///
    /// This method returns the current status of the driver.
    pub(crate) async fn get_status(&self) -> DriverStatus {
        let inner = self.inner.lock().await;
        inner.status
    }

    /// Shut down the driver.
    ///
    /// This method stops any ongoing acquisition and resets the driver state.
    ///
    /// # Important
    /// This method should always be called before the driver is dropped to ensure
    /// proper cleanup of resources. The Drop implementation provides only basic cleanup
    /// and cannot perform the full async shutdown sequence.
    pub(crate) async fn shutdown(&mut self) -> Result<(), DriverError> {
        debug!("Shutting down Ads1299Driver");
        
        // First check if running, but don't hold the lock
        let should_stop = {
            let inner = self.inner.lock().await;
            inner.running
        };
        
        // Stop acquisition if needed
        if should_stop {
            debug!("Stopping acquisition as part of shutdown");
            self.stop_acquisition().await?;
        } else {
            // Even if acquisition is not running, make sure interrupt thread is stopped
            if self.interrupt_running.load(Ordering::SeqCst) {
                debug!("Stopping interrupt task as part of shutdown");
                self.interrupt_running.store(false, Ordering::SeqCst);

                // Wait for the interrupt task to complete
                if let Some(handle) = self.interrupt_task.take() {
                    match handle.await {
                        Ok(_) => debug!("Interrupt task completed successfully during shutdown"),
                        Err(e) => warn!("Interrupt task terminated with error during shutdown: {:?}", e),
                    }
                }
            }
        }

        // Update final state
        {
            let mut inner = self.inner.lock().await;
            inner.status = DriverStatus::NotInitialized;
            inner.base_timestamp = None;
            inner.sample_count = 0;
            // Config is now static, so we don't need to reset it
        }
        
        // Notify about the status change
        self.notify_status_change().await?;
        info!("Ads1299Driver shutdown complete");
        Ok(())
    }

    /// Send a command to the ADS1299.
    fn send_command(&mut self, command: u8) -> Result<(), DriverError> {
        let spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;
        send_command_to_spi(spi.as_mut(), command)
    }

    /// Reset the ADS1299 chip.
    fn reset_chip(&mut self) -> Result<(), DriverError> {
        // Send RESET command (0x06)
        self.send_command(CMD_RESET)?;
        
        // Wait for reset to complete (recommended 18 tCLK cycles, ~4.5µs at 4MHz)
        std::thread::sleep(std::time::Duration::from_micros(10));
        
        Ok(())
    }

    /// Initialize the ADS1299 chip with the current configuration.
    async fn initialize_chip(&mut self) -> Result<(), DriverError> {
        let config = {
            let inner = self.inner.lock().await;
            inner.config.clone()
        };
        
        // Power-up sequence following the working Python script pattern:
        
        // 1. Send RESET command (0x06)
        self.send_command(CMD_RESET)?;
        
        // 2. Send zeros
        if let Some(spi) = self.spi.as_mut() {
            spi.write(&[0x00, 0x00, 0x00]).map_err(|e| DriverError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("SPI write error: {}", e)
            )))?;
        }
        
        // 3. Send SDATAC command to stop continuous data acquisition mode
        self.send_command(CMD_SDATAC)?;
        
        // Check device ID to verify communication
        let id = self.read_register(REG_ID_ADDR)?;
        if id != 0x3E {
            return Err(DriverError::Other(format!("Invalid device ID: 0x{:02X}, expected 0x3E", id)));
        }
        
        // Setup registers for CH1 mode (working configuration)
        let mut spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;

        // Write registers in the specific order
        // Implementation details moved to acquisition.rs
        
        // Wait for configuration to settle
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        // Update status
        {
            let mut inner = self.inner.lock().await;
            inner.status = DriverStatus::Ok;
        }
        
        Ok(())
    }

    /// Read a register from the ADS1299.
    fn read_register(&mut self, register: u8) -> Result<u8, DriverError> {
        let spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;
        
        // Command: RREG (0x20) + register address
        let command = 0x20 | (register & 0x1F);
        
        // First transfer: command and count (number of registers to read minus 1)
        let write_buffer = [command, 0x00];
        spi.write(&write_buffer).map_err(|e| DriverError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("SPI write command error: {}", e)
        )))?;
        
        // Second transfer: read the data (send dummy byte to receive data)
        let mut read_buffer = [0u8];
        spi.transfer(&mut read_buffer, &[0u8]).map_err(|e| DriverError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("SPI transfer error: {}", e)
        )))?;
        
        Ok(read_buffer[0])
    }

    /// Write a value to a register in the ADS1299.
    fn write_register(&mut self, register: u8, value: u8) -> Result<(), DriverError> {
        let spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;
        
        // Command: WREG (0x40) + register address
        let command = 0x40 | (register & 0x1F);
        
        // First byte: command, second byte: number of registers to write minus 1 (0 for single register)
        // Third byte: value to write
        let write_buffer = [command, 0x00, value];
        
        spi.write(&write_buffer).map_err(|e| DriverError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("SPI write error: {}", e)
        )))?;
        
        // Update register cache
        let mut inner = self.inner.try_lock().map_err(|_| DriverError::Other("Failed to lock inner state".to_string()))?;
        inner.registers[register as usize] = value;
        
        Ok(())
    }

    /// Internal helper to notify status changes over the event channel.
    ///
    /// This method sends a status change event to any listeners.
    async fn notify_status_change(&self) -> Result<(), DriverError> {
        // Get current status
        let status = {
            let inner = self.inner.lock().await;
            inner.status
        };
        
        debug!("Sending status change notification: {:?}", status);
        
        // Send the status change event
        self.tx
            .send(DriverEvent::StatusChange(status))
            .await
            .map_err(|e| DriverError::Other(format!("Failed to send status change: {}", e)))
    }
}

// Implement the AdcDriver trait
#[async_trait::async_trait]
impl super::super::types::AdcDriver for Ads1299Driver {
    async fn shutdown(&mut self) -> Result<(), DriverError> {
        self.shutdown().await
    }

    async fn start_acquisition(&mut self) -> Result<(), DriverError> {
        self.start_acquisition().await
    }

    async fn stop_acquisition(&mut self) -> Result<(), DriverError> {
        self.stop_acquisition().await
    }

    async fn get_status(&self) -> DriverStatus {
        self.get_status().await
    }

    async fn get_config(&self) -> Result<AdcConfig, DriverError> {
        self.get_config().await
    }
}

// Implement Send and Sync for Ads1299Driver
// SAFETY: This unsafe impl of Send/Sync is justified due to the internal Arc<Mutex> and careful resource handling.
// If the driver's internal structure changes, this safety guarantee must be re-evaluated.
unsafe impl Send for Ads1299Driver {}
unsafe impl Sync for Ads1299Driver {}

/// Implementation of Drop for Ads1299Driver to handle cleanup when the driver is dropped.
///
/// Note: This provides only basic cleanup. For proper cleanup, users should explicitly
/// call `shutdown()` before letting the driver go out of scope. The Drop implementation
/// cannot perform the full async shutdown sequence because Drop is not async.
impl Drop for Ads1299Driver {
    fn drop(&mut self) {
        // Since we can't use .await in Drop, we'll just log a warning
        error!("Ads1299Driver dropped without calling shutdown() first. This may lead to resource leaks.");
        error!("Always call driver.shutdown().await before dropping the driver.");
        
        // Note: We can't properly clean up in Drop because we can't use .await
        // This is why users should call shutdown() explicitly.
        
        // Note: We cannot await the task_handle here because Drop is not async.
        // This is why users should call shutdown() explicitly.
        if self.task_handle.is_some() {
            error!("Background task may still be running. Call shutdown() to properly terminate it.");
        }
        
        // Check if interrupt task is still running
        if self.interrupt_running.load(Ordering::SeqCst) {
            error!("Interrupt task may still be running. Call shutdown() to properly terminate it.");
            // Try to stop the interrupt task
            self.interrupt_running.store(false, Ordering::SeqCst);
        }

        // Try to drop the interrupt task if it exists
        if let Some(_handle) = self.interrupt_task.take() {
            // Cannot await in Drop, so just detach
        }
        
        // Hardware lock is released automatically by HardwareLockGuard's Drop.
    }
}
