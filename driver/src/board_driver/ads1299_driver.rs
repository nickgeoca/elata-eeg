use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use async_trait::async_trait;
use log::{info, warn, debug, trace, error};
use lazy_static::lazy_static;
use rppal::spi::{Spi, Bus, SlaveSelect, Mode};
use rppal::gpio::{Gpio, InputPin};
use super::types::{AdcConfig, AdcData, DriverStatus, DriverError, DriverEvent, DriverType};

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
    spi: Option<Spi>,
    drdy_pin: Option<InputPin>,
}

/// Internal state for the Ads1299Driver.
struct Ads1299Inner {
    config: AdcConfig,
    running: bool,
    status: DriverStatus,
    // Base timestamp for calculating sample timestamps (microseconds since epoch)
    base_timestamp: Option<u64>,
    // Total samples generated since acquisition started
    sample_count: u64,
    // Cache of register values
    registers: [u8; 24],
}

// ADS1299 Commands
const ADS1299_CMD_WAKEUP: u8 = 0x02;
const ADS1299_CMD_STANDBY: u8 = 0x04;
const ADS1299_CMD_RESET: u8 = 0x06;
const ADS1299_CMD_START: u8 = 0x08;
const ADS1299_CMD_STOP: u8 = 0x0A;
const ADS1299_CMD_RDATAC: u8 = 0x10;
const ADS1299_CMD_SDATAC: u8 = 0x11;
const ADS1299_CMD_RDATA: u8 = 0x12;

// ADS1299 Registers
const ADS1299_REG_ID: u8 = 0x00;
const ADS1299_REG_CONFIG1: u8 = 0x01;
const ADS1299_REG_CONFIG2: u8 = 0x02;
const ADS1299_REG_CONFIG3: u8 = 0x03;
const ADS1299_REG_LOFF: u8 = 0x04;
const ADS1299_REG_CH1SET: u8 = 0x05;
const ADS1299_REG_CH2SET: u8 = 0x06;
const ADS1299_REG_CH3SET: u8 = 0x07;
const ADS1299_REG_CH4SET: u8 = 0x08;
const ADS1299_REG_CH5SET: u8 = 0x09;
const ADS1299_REG_CH6SET: u8 = 0x0A;
const ADS1299_REG_CH7SET: u8 = 0x0B;
const ADS1299_REG_CH8SET: u8 = 0x0C;
const ADS1299_REG_BIAS_SENSP: u8 = 0x0D;
const ADS1299_REG_BIAS_SENSN: u8 = 0x0E;
const ADS1299_REG_LOFF_SENSP: u8 = 0x0F;
const ADS1299_REG_LOFF_SENSN: u8 = 0x10;
const ADS1299_REG_LOFF_FLIP: u8 = 0x11;
const ADS1299_REG_LOFF_STATP: u8 = 0x12;
const ADS1299_REG_LOFF_STATN: u8 = 0x13;
const ADS1299_REG_GPIO: u8 = 0x14;
const ADS1299_REG_MISC1: u8 = 0x15;
const ADS1299_REG_MISC2: u8 = 0x16;
const ADS1299_REG_CONFIG4: u8 = 0x17;

impl Ads1299Driver {
    /// Create a new instance of the Ads1299Driver.
    ///
    /// This constructor takes an ADC configuration and an optional additional channel buffering parameter.
    /// The additional_channel_buffering parameter determines how many extra batches can be buffered in the channel
    /// beyond the minimum required (which is the batch_size from the config). Setting this to 0 minimizes
    /// latency but may cause backpressure if the consumer can't keep up.
    ///
    /// # Important
    /// Users should explicitly call `shutdown()` when done with the driver to ensure proper cleanup.
    /// While the Drop implementation provides some basic cleanup, it cannot perform the full async shutdown sequence.
    ///
    /// # Returns
    /// A tuple containing the driver instance and a receiver for driver events.
    ///
    /// # Errors
    /// Returns an error if:
    /// - config.board_driver is not DriverType::Ads1299
    /// - config.batch_size is 0 (batch size must be positive)
    /// - config.batch_size is less than the number of channels (need at least one sample per channel)
    /// - SPI or GPIO initialization fails
    pub fn new(
        config: AdcConfig,
        additional_channel_buffering: usize
    ) -> Result<(Self, mpsc::Receiver<DriverEvent>), DriverError> {
        // Try to acquire the hardware lock to simulate real hardware access constraints
        let mut hardware_in_use = HARDWARE_LOCK.lock()
            .map_err(|_| DriverError::Other("Failed to acquire hardware lock".to_string()))?;
            
        if *hardware_in_use {
            return Err(DriverError::HardwareNotFound(
                "Hardware already in use by another driver instance".to_string()
            ));
        }
        
        // Mark hardware as in use
        *hardware_in_use = true;
        
        // Validate config
        if config.board_driver != DriverType::Ads1299 {
            // Release the lock if we're returning an error
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                "Ads1299Driver requires config.board_driver=DriverType::Ads1299".to_string()
            ));
        }
        
        // Validate batch size
        if config.batch_size == 0 {
            // Release the lock if we're returning an error
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                "Batch size must be greater than 0".to_string()
            ));
        }
        
        // Validate batch size relative to channel count
        if config.batch_size < config.channels.len() {
            // Release the lock if we're returning an error
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                format!("Batch size ({}) must be at least equal to the number of channels ({})",
                        config.batch_size, config.channels.len())
            ));
        }
        
        // Validate total buffer size (prevent excessive memory usage)
        const MAX_BUFFER_SIZE: usize = 10000; // Arbitrary limit to prevent excessive memory usage
        let channel_buffer_size = config.batch_size + additional_channel_buffering;
        if channel_buffer_size > MAX_BUFFER_SIZE {
            // Release the lock if we're returning an error
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                format!("Total buffer size ({}) exceeds maximum allowed ({})",
                        channel_buffer_size, MAX_BUFFER_SIZE)
            ));
        }
        
        // Initialize SPI
        let spi = match Self::init_spi() {
            Ok(spi) => spi,
            Err(e) => {
                // Release the lock if we're returning an error
                *hardware_in_use = false;
                return Err(e);
            }
        };
        
        // Initialize DRDY pin
        let drdy_pin = match Self::init_drdy_pin() {
            Ok(pin) => pin,
            Err(e) => {
                // Release the lock if we're returning an error
                *hardware_in_use = false;
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
        // Check preconditions without holding the lock for too long
        {
            let inner = self.inner.lock().await;
                
            if inner.running {
                return Err(DriverError::ConfigurationError("Acquisition already running".to_string()));
            }
        }
        
        // Initialize the chip
        self.initialize_chip().await?;
        
        // Get the initial timestamp
        let start_time = current_timestamp_micros()?;
        
        // Update state to running
        {
            let mut inner = self.inner.lock().await;
            inner.running = true;
            inner.status = DriverStatus::Running;
            inner.base_timestamp = Some(start_time);
            inner.sample_count = 0;
        }
        
        // Notify about the status change
        self.notify_status_change().await?;
        
        // Check DRDY pin state before starting
        if let Some(drdy_pin) = &self.drdy_pin {
            let drdy_state = if drdy_pin.is_high() { "HIGH" } else { "LOW" };
            debug!("DRDY pin state before starting conversion: {}", drdy_state);
            
            // DRDY should be high when idle
            if !drdy_pin.is_high() {
                warn!("DRDY pin is LOW before starting conversion, which is unexpected. This may indicate a hardware issue.");
            }
        }
        
        // Start conversion
        self.start_conversion()?;
        debug!("Conversion started");
        
        // Prepare for background task
        let inner_arc = self.inner.clone();
        let tx = self.tx.clone();
        let drdy_pin = self.drdy_pin.take().ok_or(DriverError::NotInitialized)?;
        let mut spi = self.spi.take().ok_or(DriverError::NotInitialized)?;
        
        // Spawn a task that monitors DRDY pin and reads data
        let handle = tokio::spawn(async move {
            // Get configuration and base timestamp
            let (config, base_timestamp) = {
                let inner = inner_arc.lock().await;
                (inner.config.clone(), inner.base_timestamp.expect("Base timestamp should be set"))
            };
            
            // Get batch size from config
            let batch_size = config.batch_size;
            let num_channels = config.channels.len();
            
            info!("Starting acquisition with batch size: {}, sample rate: {} Hz",
                   batch_size, config.sample_rate);
            info!("Buffering {} samples before sending batches", batch_size);
            
            // Make sure we're in SDATAC mode before starting
            if let Err(e) = send_command_to_spi(&mut spi, ADS1299_CMD_SDATAC) {
                error!("Failed to send SDATAC command: {:?}", e);
                return;
            }
            
            // Small delay after sending command
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            
            // Enter continuous data mode (RDATAC)
            if let Err(e) = send_command_to_spi(&mut spi, ADS1299_CMD_RDATAC) {
                error!("Failed to send RDATAC command: {:?}", e);
                return;
            }
            
            // Small delay after sending command
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
            
            debug!("Starting acquisition loop using RDATAC (continuous data mode)");
            
            // Main acquisition loop
            let mut sample_buffer: Vec<AdcData> = Vec::with_capacity(batch_size);
            
            loop {
                // Check if we should continue running
                let (should_continue, current_sample_count) = {
                    let mut inner = inner_arc.lock().await;
                    if !inner.running {
                        (false, 0)
                    } else {
                        let count = inner.sample_count;
                        // Update the sample count for the next sample
                        inner.sample_count += 1;
                        (true, count)
                    }
                };
                
                if !should_continue {
                    // Send any remaining samples in the buffer before breaking
                    if !sample_buffer.is_empty() {
                        debug!("Sending final batch of {} samples before stopping", sample_buffer.len());
                        if let Err(e) = tx.send(DriverEvent::Data(sample_buffer)).await {
                            error!("Ads1299Driver event channel closed: {}", e);
                        }
                    }
                    break;
                }
                
                // Measure DRDY timing for debugging
                let drdy_start = std::time::Instant::now();
                
                // Wait for DRDY to go low (active low) with timeout
                let mut timeout = 1000; // Adjust based on sample rate
                while drdy_pin.is_high() && timeout > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_micros(10)).await;
                    timeout -= 1;
                }
                
                let drdy_duration = drdy_start.elapsed();
                
                if timeout == 0 {
                    error!("DRDY timeout - pin never went low");
                    continue; // Skip this sample and try again
                } else {
                    // Only log timing occasionally to avoid flooding logs
                    if current_sample_count % 100 == 0 {
                        debug!("DRDY timing: {:?} (timeout count: {})", drdy_duration, 1000 - timeout);
                    }
                }
                
                // Read data from ADS1299
                let raw_samples = match read_data_from_spi(&mut spi, num_channels) {
                    Ok(samples) => samples,
                    Err(e) => {
                        error!("Error reading data: {:?}", e);
                        continue;
                    }
                };
                
                // Calculate timestamp
                let sample_interval = (1_000_000 / config.sample_rate) as u64;
                let timestamp = base_timestamp + current_sample_count * sample_interval;
                
                // Convert raw samples to voltage
                let mut voltage_samples = Vec::with_capacity(num_channels);
                for (i, &raw) in raw_samples.iter().enumerate() {
                    let channel_idx = config.channels[i];
                    let voltage = convert_to_voltage(raw, config.gain, config.Vref);
                    voltage_samples.push(vec![voltage]);
                }
                
                // Create AdcData
                let data = AdcData {
                    timestamp,
                    raw_samples: raw_samples.iter().map(|&s| vec![s]).collect(),
                    voltage_samples,
                };
                
                // Add to buffer
                sample_buffer.push(data);
                
                // Send the batch when we've collected batch_size samples
                if sample_buffer.len() >= batch_size {
                    info!("Sending batch of {} samples (current_sample_count: {})",
                          sample_buffer.len(), current_sample_count);
                    if let Err(e) = tx.send(DriverEvent::Data(sample_buffer)).await {
                        error!("Ads1299Driver event channel closed: {}", e);
                        break;
                    }
                    // Create a new buffer for the next batch
                    sample_buffer = Vec::with_capacity(batch_size);
                }
            }
            
            // We're already in SDATAC mode, so no need to send it again
            debug!("Acquisition loop terminated");
            
            debug!("Acquisition task terminated");
        });
        
        self.task_handle = Some(handle);
        info!("Ads1299Driver acquisition started");
        Ok(())
    }

    /// Stop data acquisition from the ADS1299.
    ///
    /// This method signals the acquisition task to stop, waits for it to complete,
    /// and updates the driver status.
    pub(crate) async fn stop_acquisition(&mut self) -> Result<(), DriverError> {
        // Signal the acquisition task to stop
        {
            let mut inner = self.inner.lock().await;
            
            if !inner.running {
                debug!("Stop acquisition called, but acquisition was not running");
                return Ok(());
            }
            
            inner.running = false;
            debug!("Signaled acquisition task to stop");
        }
        
        // Exit continuous data mode and stop conversion
        if let Some(spi) = self.spi.as_mut() {
            // First exit RDATAC mode
            if let Err(e) = send_command_to_spi(spi, ADS1299_CMD_SDATAC) {
                warn!("Failed to send SDATAC command during stop_acquisition: {:?}", e);
                // Continue anyway to try to stop conversion
            }
            
            // Small delay after sending command
            std::thread::sleep(std::time::Duration::from_millis(5));
            
            // Then stop conversion
            self.stop_conversion()?;
        }
        
        // Wait for the task to complete
        if let Some(handle) = self.task_handle.take() {
            match handle.await {
                Ok(_) => debug!("Acquisition task completed successfully"),
                Err(e) => warn!("Acquisition task terminated with error: {}", e),
            }
        }
        
        // Update driver status
        {
            let mut inner = self.inner.lock().await;
            inner.status = DriverStatus::Stopped;
            inner.sample_count = 0;
            // Keep the base_timestamp as it is - we'll set a new one when acquisition starts again
        }
        
        // Notify about the status change
        self.notify_status_change().await?;
        info!("Ads1299Driver acquisition stopped");
        Ok(())
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

    /// Initialize SPI communication with the ADS1299.
    fn init_spi() -> Result<Spi, DriverError> {
        // Use SPI0 with CS0 at 500kHz (matching the working Python script)
        // ADS1299 datasheet specifies CPOL=0, CPHA=1 (Mode 1)
        let spi_speed = 500_000; // 500kHz - confirmed working with Python script
        debug!("Initializing SPI with speed: {} Hz, Mode: Mode1 (CPOL=0, CPHA=1)", spi_speed);
        
        Spi::new(
            Bus::Spi0,
            SlaveSelect::Ss0,
            spi_speed,
            Mode::Mode1,  // CPOL=0, CPHA=1 for ADS1299
        ).map_err(|e| DriverError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("SPI initialization error: {}", e)
        )))
    }

    /// Initialize the DRDY pin for detecting when new data is available.
    fn init_drdy_pin() -> Result<InputPin, DriverError> {
        let gpio = Gpio::new().map_err(|e| DriverError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("GPIO initialization error: {}", e)
        )))?;
        
        // GPIO25 (Pin 22) is used for DRDY
        Ok(gpio.get(25).map_err(|e| DriverError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("GPIO pin error: {}", e)
        )))?.into_input_pullup())
    }

    /// Send a command to the ADS1299.
    fn send_command(&mut self, command: u8) -> Result<(), DriverError> {
        let spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;
        send_command_to_spi(spi, command)
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

    /// Read data from the ADS1299.
    fn read_data(&mut self) -> Result<Vec<i32>, DriverError> {
        // Get the number of channels
        let num_channels = {
            let inner = self.inner.try_lock().map_err(|_| DriverError::Other("Failed to lock inner state".to_string()))?;
            inner.config.channels.len()
        };
        
        // Get SPI and read data
        let spi = self.spi.as_mut().ok_or(DriverError::NotInitialized)?;
        read_data_from_spi(spi, num_channels)
    }

    /// Reset the ADS1299 chip.
    fn reset_chip(&mut self) -> Result<(), DriverError> {
        // Send RESET command (0x06)
        self.send_command(ADS1299_CMD_RESET)?;
        
        // Wait for reset to complete (recommended 18 tCLK cycles, ~4.5µs at 4MHz)
        std::thread::sleep(std::time::Duration::from_micros(10));
        
        Ok(())
    }

    /// Start conversion on the ADS1299.
    fn start_conversion(&mut self) -> Result<(), DriverError> {
        // Send START command (0x08)
        self.send_command(ADS1299_CMD_START)?;
        Ok(())
    }

    /// Stop conversion on the ADS1299.
    fn stop_conversion(&mut self) -> Result<(), DriverError> {
        // Send STOP command (0x0A)
        self.send_command(ADS1299_CMD_STOP)?;
        Ok(())
    }

    /// Configure the ADS1299 for single-ended operation.
    fn configure_single_ended(&mut self) -> Result<(), DriverError> {
        // Set MISC1 register (0x15)
        // Bit 5 (SRB1) = 0: SRB1 is disconnected from all negative inputs
        self.write_register(ADS1299_REG_MISC1, 0x00)?;
        
        // Get configuration data we need
        let (gain, channels) = {
            let inner = self.inner.try_lock().map_err(|_| DriverError::Other("Failed to lock inner state".to_string()))?;
            (inner.config.gain, inner.config.channels.clone())
        };
        
        // Get gain code
        let gain_code = self.gain_to_register_value(gain)?;
        
        // Configure channels
        for &channel in &channels {
            if channel < 8 {
                // Set CHnSET register (0x05 + channel)
                // Use 0x15 (gain=1, SRB2 enabled, test signal)
                // The value 0x15 means:
                // - Bit 7 (PD) = 0: Channel powered up
                // - Bits 6-4 (GAIN) = 001: Gain = 1
                // - Bit 3 (SRB2) = 1: SRB2 connected to negative input
                // - Bits 2-0 (MUX) = 101: Test signal
                self.write_register(0x05 + channel as u8, 0x15)?;
                debug!("Configured channel {} with CHnSET=0x15 (gain=1, SRB2 enabled, test signal)", channel);
            }
        }
        
        // Set CONFIG3 register (0x03)
        // 0x66 = 0110 0110 (PD_REFBUF=0, Bit6=1, Bit5=1, BIAS_MEAS=0, BIASREF_INT=0, PD_BIAS=1, BIAS_LOFF_SENS=1, BIAS_STAT=0)
        // Bit 7 (PD_REFBUF) = 0: Reference buffer powered up
        // Bit 6-5 = 11: Not specified in datasheet
        // Bit 2 (PD_BIAS) = 1: Bias buffer powered up
        // Bit 1 (BIAS_LOFF_SENS) = 1: Bias drive connected to LOFF sense
        self.write_register(ADS1299_REG_CONFIG3, 0x66)?;
        debug!("Configured CONFIG3=0x66 (bias buffer enabled, LOFF sense enabled)");
        
        Ok(())
    }

    /// Configure the sample rate on the ADS1299.
    fn configure_sample_rate(&mut self, sample_rate: u32) -> Result<(), DriverError> {
        // Calculate CONFIG1 register value based on sample rate
        let config1_value = match sample_rate {
            250 => 0x96,  // 250 SPS (default)
            500 => 0x95,  // 500 SPS
            1000 => 0x94, // 1000 SPS
            2000 => 0x93, // 2000 SPS
            _ => return Err(DriverError::ConfigurationError(
                format!("Unsupported sample rate: {}. Supported rates: 250, 500, 1000, 2000", sample_rate)
            )),
        };
        
        // Set CONFIG1 register (0x01)
        self.write_register(ADS1299_REG_CONFIG1, config1_value)?;
        
        Ok(())
    }

    /// Convert gain value to register value.
    fn gain_to_register_value(&self, gain: f32) -> Result<u8, DriverError> {
        match gain as u8 {
            1 => Ok(0x01),  // Gain = 1 (0b001)
            2 => Ok(0x02),  // Gain = 2 (0b010)
            4 => Ok(0x03),  // Gain = 4 (0b011)
            6 => Ok(0x00),  // Gain = 6 (0b000)
            8 => Ok(0x05),  // Gain = 8 (0b101)
            12 => Ok(0x06), // Gain = 12 (0b110)
            24 => Ok(0x07), // Gain = 24 (0b111) - was incorrectly mapped to 0x00
            _ => Err(DriverError::ConfigurationError(
                format!("Unsupported gain: {}. Supported gains: 1, 2, 4, 6, 8, 12, 24", gain)
            )),
        }
    }

    /// Initialize the ADS1299 chip with the current configuration.
    async fn initialize_chip(&mut self) -> Result<(), DriverError> {
        let config = {
            let inner = self.inner.lock().await;
            inner.config.clone()
        };
        
        // Power-up sequence following the working Python script pattern:
        
        // 1. Send RESET command (0x06)
        self.send_command(ADS1299_CMD_RESET)?;
        
        // 2. Send zeros (as in the Python script)
        if let Some(spi) = self.spi.as_mut() {
            spi.write(&[0x00, 0x00, 0x00]).map_err(|e| DriverError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("SPI write error: {}", e)
            )))?;
        }
        
        // 3. Send SDATAC command to stop continuous data acquisition mode
        self.send_command(ADS1299_CMD_SDATAC)?;
        
        // Check device ID to verify communication
        let id = self.read_register(ADS1299_REG_ID)?;
        if id != 0x3E {
            return Err(DriverError::Other(format!("Invalid device ID: 0x{:02X}, expected 0x3E", id)));
        }
        
        // Configure sample rate
        self.configure_sample_rate(config.sample_rate)?;
        
        // Verify CONFIG1 was set correctly
        let config1 = self.read_register(ADS1299_REG_CONFIG1)?;
        let expected_config1 = match config.sample_rate {
            250 => 0x96,  // 250 SPS (default)
            500 => 0x95,  // 500 SPS
            1000 => 0x94, // 1000 SPS
            2000 => 0x93, // 2000 SPS
            _ => 0x96,    // Default to 250 SPS
        };
        if config1 != expected_config1 {
            warn!("CONFIG1 register verification failed: expected 0x{:02X}, got 0x{:02X}",
                  expected_config1, config1);
        } else {
            debug!("CONFIG1 register verified: 0x{:02X} (sample rate: {} SPS)",
                   config1, config.sample_rate);
        }
        
        // Set CONFIG2 register (0x02)
        // Bits 7-6 = 11: Internal reference enabled
        // Bit 5 = 1: Test signal amplitude = 1 × –(VREFP – VREFN) / 2400
        // Bits 4-3 = 00: Not used
        // Bit 2-0 = 001: Test signal frequency = fCLK / 2^21
        self.write_register(ADS1299_REG_CONFIG2, 0xD1)?;
        
        // Verify CONFIG2 was set correctly
        let config2 = self.read_register(ADS1299_REG_CONFIG2)?;
        if config2 != 0xD1 {
            warn!("CONFIG2 register verification failed: expected 0xD1, got 0x{:02X}", config2);
        }
        
        // Set CONFIG3 register (0x03)
        // 0x66 = 0110 0110 (PD_REFBUF=0, Bit6=1, Bit5=1, BIAS_MEAS=0, BIASREF_INT=0, PD_BIAS=1, BIAS_LOFF_SENS=1, BIAS_STAT=0)
        // Bit 7 (PD_REFBUF) = 0: Reference buffer powered up
        // Bit 6-5 = 11: Not specified in datasheet
        // Bit 2 (PD_BIAS) = 1: Bias buffer powered up
        // Bit 1 (BIAS_LOFF_SENS) = 1: Bias drive connected to LOFF sense
        self.write_register(ADS1299_REG_CONFIG3, 0x66)?;
        
        // Verify CONFIG3 was set correctly
        let config3 = self.read_register(ADS1299_REG_CONFIG3)?;
        if config3 != 0x66 {
            warn!("CONFIG3 register verification failed: expected 0x66, got 0x{:02X}", config3);
        } else {
            debug!("CONFIG3 register verified: 0x66 (bias buffer enabled, LOFF sense enabled)");
        }
        
        // Set MISC1 register (0x15)
        // Bit 5 (SRB1) = 0: SRB1 disconnected from all negative inputs
        self.write_register(ADS1299_REG_MISC1, 0x00)?;
        
        // Verify MISC1 was set correctly
        let misc1 = self.read_register(ADS1299_REG_MISC1)?;
        if misc1 != 0x00 {
            warn!("MISC1 register verification failed: expected 0x00, got 0x{:02X}", misc1);
        }
        
        // Configure channels
        for &channel in &config.channels {
            if channel < 8 {
                // Set CHnSET to 0x15 (gain=1, SRB2 enabled, test signal)
                // 0x15 = 0001 0101 (PD=0, GAIN=001 (gain=1), SRB2=1, MUX=101 (test signal))
                let reg_addr = 0x05 + channel as u8;
                self.write_register(reg_addr, 0x15)?;
                
                // Verify channel setting was set correctly
                let ch_value = self.read_register(reg_addr)?;
                if ch_value != 0x15 {
                    warn!("Channel {} register verification failed: expected 0x15, got 0x{:02X}",
                          channel, ch_value);
                } else {
                    debug!("Channel {} configured successfully with value 0x15 (gain=1, SRB2 enabled, test signal)",
                           channel);
                }
            }
        }
        
        // Set CONFIG4 register (0x17)
        // Bit 0 (PD_LOFF_COMP) = 0: Lead-off comparators disabled
        self.write_register(ADS1299_REG_CONFIG4, 0x00)?;
        
        // Wait for configuration to settle
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        // Print all register values for debugging
        debug!("ADS1299 Register Values after initialization:");
        for reg in 0..=0x17 {
            if reg <= 0x17 {
                let value = self.read_register(reg)?;
                debug!("Register 0x{:02X} = 0x{:02X}", reg, value);
            }
        }
        
        // Update status
        {
            let mut inner = self.inner.lock().await;
            inner.status = DriverStatus::Ok;
        }
        
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

// Helper function to send a command to SPI
fn send_command_to_spi(spi: &mut Spi, command: u8) -> Result<(), DriverError> {
    let buffer = [command];
    spi.write(&buffer).map_err(|e| DriverError::IoError(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("SPI write error: {}", e)
    )))?;
    Ok(())
}

// Helper function to read data from SPI in continuous mode (RDATAC)
fn read_data_from_spi(spi: &mut Spi, num_channels: usize) -> Result<Vec<i32>, DriverError> {
    debug!("Reading data from ADS1299 via SPI for {} channels in continuous mode", num_channels);
    
    // In continuous mode (RDATAC), we don't need to send RDATA command before each read
    // We just read the data directly when DRDY goes low
    
    // Calculate total bytes to read: 1 status byte + (3 bytes per channel * num_channels)
    let total_bytes = 1 + (3 * num_channels);
    debug!("Reading {} total bytes (1 status + {} data bytes)", total_bytes, 3 * num_channels);
    
    // Prepare buffers for SPI transfer
    let mut read_buffer = vec![0u8; total_bytes];
    let write_buffer = vec![0u8; total_bytes];
    
    // Perform SPI transfer
    match spi.transfer(&mut read_buffer, &write_buffer) {
        Ok(_) => debug!("SPI transfer successful, read {} bytes", read_buffer.len()),
        Err(e) => {
            error!("SPI transfer error: {}", e);
            return Err(DriverError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("SPI transfer error: {}", e)
            )));
        }
    }
    
    // Log raw data for debugging
    debug!("Raw SPI data: {:02X?}", read_buffer);
    
    // Parse the data (skip the first status byte)
    let mut samples = Vec::with_capacity(num_channels);
    
    for ch in 0..num_channels {
        let start_idx = 1 + (ch * 3); // Skip 1 status byte, then 3 bytes per channel
        
        // Extract the 3 bytes for this channel
        let msb = read_buffer[start_idx] as i32;
        let mid = read_buffer[start_idx + 1] as i32;
        let lsb = read_buffer[start_idx + 2] as i32;
        
        // Combine bytes into a 24-bit signed integer
        let mut value = (msb << 16) | (mid << 8) | lsb;
        
        // Sign extension for negative values
        if (value & 0x800000) != 0 {
            value |= -16777216; // 0xFF000000 as signed
        }
        
        debug!("Channel {}: raw bytes [{:02X} {:02X} {:02X}] = {}",
               ch, read_buffer[start_idx], read_buffer[start_idx + 1], read_buffer[start_idx + 2], value);
        
        samples.push(value);
    }
    
    Ok(samples)
}

// Helper function to get current timestamp in microseconds
fn current_timestamp_micros() -> Result<u64, DriverError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .map_err(|e| DriverError::Other(format!("Failed to get timestamp: {}", e)))
}

// Helper function to convert raw sample to voltage
fn convert_to_voltage(sample: i32, gain: f32, vref: f32) -> f32 {
    // Formula: voltage = (sample * vref) / (gain * 2^23)
    let result = (sample as f64 * vref as f64) / (gain as f64 * 8388608.0);
    info!("Converting raw sample {} to voltage: {} (gain={}, vref={})",
          sample, result, gain, vref);
    result as f32
}

// Implement the AdcDriver trait
#[async_trait]
impl super::types::AdcDriver for Ads1299Driver {
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
// This is safe because we're using Arc<Mutex<>> for shared state
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
        
        // Release the hardware lock
        if let Ok(mut lock) = HARDWARE_LOCK.lock() {
            *lock = false;
            debug!("Hardware lock released in Drop implementation");
        } else {
            error!("Failed to release hardware lock in Drop implementation");
        }
    }
}
