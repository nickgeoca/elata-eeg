//! Main driver implementation for the ADS1299 chip.

use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use crossbeam_channel::Sender;
use std::thread;
use std::time::Duration;
use log::{info, warn, debug};
use crate::types::{AdcConfig, AdcDriver, DriverStatus, DriverError, DriverType};
use eeg_types::{BridgeMsg, SensorError};
use pipeline::data::{Packet, PacketData, PacketHeader, SensorMeta};
use super::registers::{
    CMD_RESET, CMD_SDATAC, REG_ID_ADDR,
    CONFIG1_ADDR, CONFIG2_ADDR, CONFIG3_ADDR, CONFIG4_ADDR,
    LOFF_SENSP_ADDR, MISC1_ADDR, BIAS_SENSP_ADDR, BIAS_SENSN_ADDR,
    CMD_RDATAC
};
use super::spi::{SpiDevice, InputPinDevice, init_spi, init_drdy_pin, send_command_to_spi, wait_irq};
use super::helpers::{ch_sample_to_raw, current_timestamp_micros};


/// ADS1299 driver for interfacing with the ADS1299EEG_FE board.
#[derive(Clone)]
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

        use rppal::spi::{Bus, SlaveSelect};

        // Initialize SPI
        let chip_config = config.chips.get(0).ok_or_else(|| DriverError::ConfigurationError("Missing chip config".to_string()))?;
        let bus = match chip_config.spi_bus {
            0 => Bus::Spi0,
            1 => Bus::Spi1,
            _ => return Err(DriverError::ConfigurationError("Invalid SPI bus".to_string())),
        };
        let slave_select = match chip_config.cs_pin {
            0 => SlaveSelect::Ss0,
            1 => SlaveSelect::Ss1,
            2 => SlaveSelect::Ss2,
            _ => return Err(DriverError::ConfigurationError("Invalid CS pin".to_string())),
        };
        let spi = init_spi(bus, slave_select)?;
        let drdy_pin = init_drdy_pin(chip_config.drdy_pin)?;
        
        // Initialize register cache
        let registers = [0u8; 24];
        
        let channel_names = config
            .channels
            .iter()
            .map(|&ch| format!("ch{}", ch))
            .collect();

        let sensor_meta = Arc::new(SensorMeta {
            schema_ver: 2,
            source_type: "ADS1299".to_string(),
            v_ref: config.vref,
            adc_bits: 24,
            gain: config.gain,
            sample_rate: config.sample_rate,
            offset_code: 0, // Assuming no offset for now
            is_twos_complement: true,
            channel_names,
            #[cfg(feature = "meta-tags")]
            tags: Default::default(),
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
        let driver = Ads1299Driver {
            inner: Arc::new(Mutex::new(inner)),
            spi: Arc::new(Mutex::new(Some(spi))),
            drdy_pin: Arc::new(Mutex::new(Some(drdy_pin))),
        };

        info!("Ads1299Driver created with config: {:?}", config);

        Ok(driver)
    }
    
    /// Send a command to the ADS1299.
    pub fn send_command(&self, command: u8) -> Result<(), DriverError> {
        let mut spi_opt = self.spi.lock().unwrap();
        let spi = spi_opt.as_mut().ok_or(DriverError::NotInitialized)?;
        send_command_to_spi(spi.as_mut(), command)
    }


    /// Initialize the ADS1299 chip with raw register values.
    pub fn initialize_chip(
        &mut self,
        config1: u8,
        config2: u8,
        config3: u8,
        config4: u8,
        loff: u8,
        misc1: u8,
        ch_settings: &[(u8, u8)], // List of (channel_address, value)
        bias_sensp: u8,
        bias_sensn: u8,
    ) -> Result<(), DriverError> {
        // Power-up sequence
        self.send_command(CMD_RESET)?;
        thread::sleep(Duration::from_millis(10)); // Wait for reset
        self.send_command(CMD_SDATAC)?;
        thread::sleep(Duration::from_millis(10)); // Wait for SDATAC

        // Verify communication
        let id = self.read_register(REG_ID_ADDR)?;
        if id != 0x3E {
            return Err(DriverError::HardwareNotFound(format!(
                "Invalid device ID: 0x{:02X}, expected 0x3E",
                id
            )));
        }

        // Write the raw register values provided by the board driver
        self.write_register(CONFIG1_ADDR, config1)?;
        self.write_register(CONFIG2_ADDR, config2)?;
        self.write_register(CONFIG3_ADDR, config3)?;
        self.write_register(CONFIG4_ADDR, config4)?;
        self.write_register(LOFF_SENSP_ADDR, loff)?;
        self.write_register(MISC1_ADDR, misc1)?;

        // Configure each channel
        for &(addr, value) in ch_settings {
            self.write_register(addr, value)?;
        }

        // Configure bias sense registers
        self.write_register(BIAS_SENSP_ADDR, bias_sensp)?;
        self.write_register(BIAS_SENSN_ADDR, bias_sensn)?;

        // Optional: Dump registers for verification
        log::info!("----Register Dump After Configuration----");
        let names = ["ID", "CONFIG1", "CONFIG2", "CONFIG3", "LOFF", "CH1SET", "CH2SET", "CH3SET", "CH4SET", "CH5SET", "CH6SET", "CH7SET", "CH8SET", "BIAS_SENSP", "BIAS_SENSN", "LOFF_SENSP", "LOFF_SENSN", "LOFF_FLIP", "LOFF_STATP", "LOFF_STATN", "GPIO", "MISC1", "MISC2", "CONFIG4"];
        for reg in 0..=0x17 {
            if let Ok(val) = self.read_register(reg as u8) {
                log::info!("Reg 0x{:02X} ({:<12}): 0x{:02X}", reg, names.get(reg).unwrap_or(&"Unknown"), val);
            }
        }
        log::info!("----End Register Dump----");

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
    fn initialize(&mut self) -> Result<(), DriverError> {
        // This driver is meant to be initialized by a board-level driver
        // by calling `initialize_chip` with raw register values.
        // Direct initialization is not supported as it lacks board-specific context.
        Err(DriverError::ConfigurationError(
            "Ads1299Driver cannot be initialized directly. Use a board driver like ElataV1 or ElataV2.".to_string()
        ))
    }

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

                    let packet = Packet::RawI32(PacketData {
                        header: PacketHeader::default(), // TODO: Populate header properly
                        samples,
                    });

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