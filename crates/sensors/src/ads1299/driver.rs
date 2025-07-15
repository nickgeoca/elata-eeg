//! Main driver implementation for the ADS1299 chip.

use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;
use log::{info, warn, debug, error};
use lazy_static::lazy_static;

use crate::types::{AdcConfig, AdcDriver, DriverStatus, DriverError, DriverType};
use eeg_types::{Packet, PacketHeader, SensorMeta, BridgeMsg, SensorError};
use super::error::HardwareLockGuard;
use super::registers::{
    CMD_RESET, CMD_SDATAC, REG_ID_ADDR,
    CONFIG1_ADDR, CONFIG2_ADDR, CONFIG3_ADDR, CONFIG4_ADDR,
    LOFF_SENSP_ADDR, MISC1_ADDR, CH1SET_ADDR, BIAS_SENSP_ADDR, BIAS_SENSN_ADDR,
    CONFIG1_REG, CONFIG2_REG, CONFIG3_REG, CONFIG4_REG, LOFF_SESP_REG, MISC1_REG,
    CHN_OFF, CHN_REG, BIAS_SENSN_REG_MASK, CMD_RDATAC
};
use super::spi::{SpiDevice, InputPinDevice, init_spi, init_drdy_pin, send_command_to_spi, wait_irq};
use super::helpers::{ch_sample_to_raw, current_timestamp_micros};


// Static hardware lock to simulate real hardware access constraints
lazy_static! {
    static ref HARDWARE_LOCK: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
}

/// ADS1299 driver for interfacing with the ADS1299EEG_FE board.
pub struct Ads1299Driver {
    inner: Arc<Mutex<Ads1299Inner>>,
    spi: Arc<Mutex<Option<Box<dyn SpiDevice>>>>,
    drdy_pin: Arc<Mutex<Option<Box<dyn InputPinDevice>>>>,
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
    // V2 metadata
    pub sensor_meta: Arc<SensorMeta>,
}

impl Ads1299Driver {
    pub fn new(
        config: AdcConfig,
    ) -> Result<Self, DriverError> {
        // Acquire the hardware lock using RAII guard
        let _hardware_lock_guard = HardwareLockGuard::new(&HARDWARE_LOCK)?;

        // Validate config
        if config.board_driver != DriverType::Ads1299 {
            return Err(DriverError::ConfigurationError(
                "Ads1299Driver requires config.board_driver=DriverType::Ads1299".to_string()
            ));
        }

        // Validate channels
        if config.channels.is_empty() {
            return Err(DriverError::ConfigurationError(
                "At least one channel must be configured".to_string()
            ));
        }

        // Check for duplicate channels
        let mut unique_channels = std::collections::HashSet::new();
        for &channel in &config.channels {
            if !unique_channels.insert(channel) {
                return Err(DriverError::ConfigurationError(
                    format!("Duplicate channel detected: {}", channel)
                ));
            }
        }

        // Validate channel indices for ADS1299 (0-7)
        for &channel in &config.channels {
            if channel > 7 {
                return Err(DriverError::ConfigurationError(
                    format!("Invalid channel index: {}. ADS1299 supports channels 0-7", channel)
                ));
            }
        }

        // Validate sample rate for ADS1299
        match config.sample_rate {
            250 | 500 | 1000 | 2000 | 4000 | 8000 | 16000 => {
                // Valid sample rates for ADS1299
            }
            _ => {
                return Err(DriverError::ConfigurationError(
                    format!("Invalid sample rate: {}. ADS1299 supports: 250, 500, 1000, 2000, 4000, 8000, 16000 Hz", config.sample_rate)
                ));
            }
        }

        // Validate batch size
        if config.batch_size == 0 {
            return Err(DriverError::ConfigurationError(
                "Batch size must be greater than 0".to_string()
            ));
        }

        // Initialize SPI
        let spi = init_spi()?;
        let drdy_pin = init_drdy_pin()?;
        
        // Initialize register cache
        let registers = [0u8; 24];
        
        let sensor_meta = Arc::new(SensorMeta {
            schema_ver: 2,
            source_type: "ADS1299".to_string(),
            v_ref: config.vref,
            adc_bits: 24,
            gain: config.gain,
            sample_rate: config.sample_rate,
            offset_code: 0, // Assuming no offset for now
            is_twos_complement: true,
        });

        let inner = Ads1299Inner {
            config: config.clone(),
            running: false,
            status: DriverStatus::NotInitialized,
            base_timestamp: None,
            sample_count: 0,
            registers,
            sensor_meta,
        };
        
        // Create the driver as mutable
        let mut driver = Ads1299Driver {
            inner: Arc::new(Mutex::new(inner)),
            spi: Arc::new(Mutex::new(Some(spi))),
            drdy_pin: Arc::new(Mutex::new(Some(drdy_pin))),
        };

        driver.initialize_chip()?;
        
        // Put chip in standby (low power) mode initially
        driver.send_command(super::registers::CMD_STANDBY)?;

        info!("Ads1299Driver created with config: {:?}", config);

        Ok(driver)
    }
    
    /// Send a command to the ADS1299.
    fn send_command(&self, command: u8) -> Result<(), DriverError> {
        let mut spi_opt = self.spi.lock().unwrap();
        let spi = spi_opt.as_mut().ok_or(DriverError::NotInitialized)?;
        send_command_to_spi(spi.as_mut(), command)
    }


    /// Initialize the ADS1299 chip with the current configuration.
    fn initialize_chip(&mut self) -> Result<(), DriverError> {
        let config = {
            let inner = self.inner.lock().unwrap();
            inner.config.clone()
        };
        
        // Power-up sequence following the working Python script pattern:
        
        // 1. Send RESET command (0x06)
        self.send_command(CMD_RESET)?;
        
        // 2. Send zeros
        if let Some(spi) = self.spi.lock().unwrap().as_mut() {
            spi.write(&[0x00, 0x00, 0x00]).map_err(|e| DriverError::SpiError(format!("SPI write error: {}", e)))?;
        }
        
        // 3. Send SDATAC command to stop continuous data acquisition mode
        self.send_command(CMD_SDATAC)?;
        
        // Check device ID to verify communication
        let id = self.read_register(REG_ID_ADDR)?;
        if id != 0x3E {
            return Err(DriverError::Other(format!("Invalid device ID: 0x{:02X}, expected 0x3E", id)));
        }
        
        // Setup registers for CH1 mode (working configuration)
        let _spi = self.spi.lock().unwrap().as_mut().ok_or(DriverError::NotInitialized)?;

        // Write registers in the specific order
        // Constants are imported from super::registers

        // Calculate masks based on config
        let active_ch_mask = config.channels.iter().fold(0, |mask, &ch| mask | (1 << ch));
        // Use the fully qualified path to call these functions
        let gain_mask = super::registers::gain_to_reg_mask(config.gain as f32)?;
        let sps_mask = super::registers::sps_to_reg_mask(config.sample_rate)?;

        // Write registers
        self.write_register(CONFIG1_ADDR, CONFIG1_REG | sps_mask)?;
        self.write_register(CONFIG2_ADDR, CONFIG2_REG)?;
        self.write_register(CONFIG3_ADDR, CONFIG3_REG)?;
        self.write_register(CONFIG4_ADDR, CONFIG4_REG)?;
        self.write_register(LOFF_SENSP_ADDR, LOFF_SESP_REG)?; // Assuming LOFF is off
        self.write_register(MISC1_ADDR, MISC1_REG)?;
        // Turn off all channels first
        for ch in 0..8 { // Assuming 8 channels max for ADS1299
            self.write_register(CH1SET_ADDR + ch, CHN_OFF)?;
        }
        // Turn on configured channels with correct gain
        for &ch in &config.channels {
            if ch < 8 { // Ensure channel index is valid
                 self.write_register(CH1SET_ADDR + ch as u8, CHN_REG | gain_mask)?;
            } else {
                 log::warn!("Channel index {} out of range (0-7), skipping configuration.", ch);
            }
        }
        // Configure bias based on active channels
        self.write_register(BIAS_SENSP_ADDR, active_ch_mask as u8)?;
        self.write_register(BIAS_SENSN_ADDR, BIAS_SENSN_REG_MASK)?;

        // Add register dump for verification (optional but helpful)
        log::info!("----Register Dump After Configuration----");
        let names = ["ID", "CONFIG1", "CONFIG2", "CONFIG3", "LOFF", "CH1SET", "CH2SET", "CH3SET", "CH4SET", "CH5SET", "CH6SET", "CH7SET", "CH8SET", "BIAS_SENSP", "BIAS_SENSN", "LOFF_SENSP", "LOFF_SENSN", "LOFF_FLIP", "LOFF_STATP", "LOFF_STATN", "GPIO", "MISC1", "MISC2", "CONFIG4"];
        for reg in 0..=0x17 {
            match self.read_register(reg as u8) {
                Ok(val) => log::info!("Reg 0x{:02X} ({:<12}): 0x{:02X}", reg, names.get(reg).unwrap_or(&"Unknown"), val),
                Err(e) => log::error!("Failed to read register 0x{:02X}: {}", reg, e),
            }
        }
        log::info!("----End Register Dump----");
        
        // Wait for configuration to settle
        thread::sleep(Duration::from_millis(10));
        
        // Update status
        {
            let mut inner = self.inner.lock().unwrap();
            inner.status = DriverStatus::Ok;
        }
        
        Ok(())
    }

    /// Read a register from the ADS1299.
    fn read_register(&self, register: u8) -> Result<u8, DriverError> {
        let mut spi_opt = self.spi.lock().unwrap();
        let spi = spi_opt.as_mut().ok_or(DriverError::NotInitialized)?;

        // Command: RREG (0x20) + register address
        let command = 0x20 | (register & 0x1F);

        // First transfer: command and count (number of registers to read minus 1)
        let write_buffer = [command, 0x00];
        spi.write(&write_buffer).map_err(|e| DriverError::SpiError(format!("SPI write command error: {}", e)))?;

        // Second transfer: read the data (send dummy byte to receive data)
        let mut read_buffer = [0u8];
        spi.transfer(&mut read_buffer, &[0u8]).map_err(|e| DriverError::SpiError(format!("SPI transfer error: {}", e)))?;

        Ok(read_buffer[0])
    }

    /// Write a value to a register in the ADS1299.
    fn write_register(&self, register: u8, value: u8) -> Result<(), DriverError> {
        let mut spi_opt = self.spi.lock().unwrap();
        let spi = spi_opt.as_mut().ok_or(DriverError::NotInitialized)?;

        // Command: WREG (0x40) + register address
        let command = 0x40 | (register & 0x1F);

        // First byte: command, second byte: number of registers to write minus 1 (0 for single register)
        // Third byte: value to write
        let write_buffer = [command, 0x00, value];

        spi.write(&write_buffer).map_err(|e| DriverError::SpiError(format!("SPI write error: {}", e)))?;

        // Update register cache
        let mut inner = self.inner.lock().unwrap();
        inner.registers[register as usize] = value;

        Ok(())
    }
}

// Implement the AdcDriver trait
impl crate::types::AdcDriver for Ads1299Driver {
    fn acquire(&mut self, tx: Sender<BridgeMsg>, stop_flag: &AtomicBool) -> Result<(), SensorError> {
        info!("ADS1299 synchronous acquisition started");

        let mut spi_opt = self.spi.lock().unwrap();
        let mut drdy_pin_opt = self.drdy_pin.lock().unwrap();

        let mut spi = spi_opt.take().ok_or(SensorError::HardwareFault("SPI not initialized".to_string()))?;
        let mut drdy_pin = drdy_pin_opt.take().ok_or(SensorError::HardwareFault("DRDY pin not initialized".to_string()))?;

        let (config, _meta) = {
            let mut inner = self.inner.lock().unwrap();
            inner.running = true;
            inner.status = DriverStatus::Running;
            inner.base_timestamp = Some(current_timestamp_micros().unwrap_or(0));
            inner.sample_count = 0;
            (inner.config.clone(), inner.sensor_meta.clone())
        };
        let num_channels = config.channels.len();
        let packet_size = 3 + (num_channels * 3); // status + 3 bytes/channel

        // Start data stream
        send_command_to_spi(spi.as_mut(), CMD_RDATAC)
            .map_err(|e| SensorError::HardwareFault(e.to_string()))?;

        while !stop_flag.load(Ordering::Relaxed) {
            match wait_irq(drdy_pin.as_mut(), Duration::from_millis(100)) {
                Ok(true) => {
                    let mut buffer = vec![0u8; packet_size];
                    spi.transfer(&mut buffer, &[]).map_err(|e| SensorError::HardwareFault(e.to_string()))?;

                    let mut samples = Vec::with_capacity(num_channels);
                    for i in 0..num_channels {
                        let start = 3 + i * 3;
                        let sample = ch_sample_to_raw(buffer[start], buffer[start + 1], buffer[start + 2]);
                        samples.push(sample);
                    }

                    let packet = Packet {
                        header: PacketHeader::default(), // TODO: Populate header properly
                        samples,
                    };

                    if tx.send(BridgeMsg::Data(packet)).is_err() {
                        warn!("ADS1299 bridge channel closed");
                        break;
                    }
                }
                Ok(false) => {
                    // Timeout, check stop flag and continue
                    continue;
                }
                Err(e) => {
                    let err = SensorError::HardwareFault(format!("DRDY pin error: {}", e));
                    tx.send(BridgeMsg::Error(err.clone())).ok();
                    return Err(err);
                }
            }
        }

        // Stop data stream
        send_command_to_spi(spi.as_mut(), CMD_SDATAC)
            .map_err(|e| SensorError::HardwareFault(e.to_string()))?;

        *spi_opt = Some(spi);
        *drdy_pin_opt = Some(drdy_pin);

        let mut inner = self.inner.lock().unwrap();
        inner.running = false;
        inner.status = DriverStatus::Stopped;

        info!("ADS1299 synchronous acquisition stopped");
        Ok(())
    }

    fn get_status(&self) -> DriverStatus {
        self.inner.lock().unwrap().status.clone()
    }

    fn get_config(&self) -> Result<AdcConfig, DriverError> {
        Ok(self.inner.lock().unwrap().config.clone())
    }

    fn shutdown(&mut self) -> Result<(), DriverError> {
        debug!("Shutting down Ads1299Driver");

        // Put chip in standby (low power) mode on shutdown
        if let Some(driver_spi) = self.spi.lock().unwrap().as_mut() {
            let _ = driver_spi.write(&[super::registers::CMD_STANDBY]);
        }

        let mut inner = self.inner.lock().unwrap();
        inner.running = false;
        inner.status = DriverStatus::NotInitialized;
        inner.base_timestamp = None;
        inner.sample_count = 0;

        info!("Ads1299Driver shutdown complete");
        Ok(())
    }
}

impl Drop for Ads1299Driver {
    fn drop(&mut self) {
        // Ensure shutdown is called.
        if self.get_status() != DriverStatus::NotInitialized {
             warn!("Ads1299Driver dropped without calling shutdown() first. This may lead to resource leaks.");
             let _ = self.shutdown();
        }
    }
}