//! Main driver implementation for the ADS1299 chip.

use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::thread;
use std::time::Duration;

use flume::{Receiver, Sender};
use log::{debug, info, warn};
use rppal::gpio::OutputPin;

use crate::spi_bus::SpiBus;
use crate::types::ChipConfig;
use eeg_types::SensorError;

use crate::types::{AdcConfig, AdcDriver, DriverError, DriverStatus};
use super::helpers::ch_sample_to_raw;
use super::registers::{
    BIAS_SENSN_ADDR, BIAS_SENSP_ADDR, CMD_RESET, CMD_SDATAC, CONFIG1_ADDR,
    CONFIG2_ADDR, CONFIG3_ADDR, CONFIG4_ADDR, LOFF_SENSP_ADDR, MISC1_ADDR, REG_ID_ADDR, CMD_STANDBY,
};

/// ADS1299 driver for interfacing with a single ADS1299 chip over a shared SPI bus.
#[derive(Clone)]
pub struct Ads1299Driver {
    inner: Arc<Mutex<Ads1299Inner>>,
    bus: Arc<SpiBus>,
}

/// Internal state for the Ads1299Driver.
pub struct Ads1299Inner {
    pub config: ChipConfig,
    pub cs_pin: OutputPin,
    pub running: bool,
    pub status: DriverStatus,
    pub registers: [u8; 24],
}

impl Ads1299Driver {
    /// Creates a new driver instance for one ADS1299 chip.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration specific to this chip.
    /// * `spi` - A shared SPI device handle.
    /// * `cs_pin` - The chip select pin for this chip.
    /// * `drdy_rx` - A channel receiver for DRDY signals.
    pub fn new(
        config: ChipConfig,
        bus: Arc<SpiBus>,
        cs_pin: OutputPin,
    ) -> Result<Self, DriverError> {
        let inner = Ads1299Inner {
            config,
            cs_pin,
            running: false,
            status: DriverStatus::NotInitialized,
            registers: [0u8; 24],
        };

        let driver = Ads1299Driver {
            inner: Arc::new(Mutex::new(inner)),
            bus,
        };

        let cs_pin_num = driver.inner.lock().unwrap().config.cs_pin;
        info!("Ads1299Driver created for CS pin {}", cs_pin_num);
        debug!("CS pin initialized to high");
        Ok(driver)
    }

    pub fn read_data_raw(&mut self) -> Result<Vec<i32>, SensorError> {
        let mut inner = self.inner.lock().unwrap();
        let num_channels = inner.config.channels.len();
        // The ADS1299 always sends a fixed-size frame for all 8 channels, regardless of configuration.
        // We must read all 27 bytes (3 status + 8 channels * 3 bytes) to avoid stalling the SPI bus.
        let mut frame_buffer = vec![0u8; 27];
        self.read_frame(&mut inner, &mut frame_buffer)?;

        let mut samples = Vec::with_capacity(num_channels);
        for i in 0..num_channels {
            let offset = 3 + i * 3;
            let sample = ch_sample_to_raw(
                frame_buffer[offset],
                frame_buffer[offset + 1],
                frame_buffer[offset + 2],
            );
            samples.push(sample);
        }
        Ok(samples)
    }

    /// Send a command to the ADS1299, handling CS and SPI bus locking.
    pub fn send_command(&self, command: u8) -> Result<(), DriverError> {
        let mut inner = self.inner.lock().unwrap();
        self.bus.write(&mut inner.cs_pin, &[command])
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
        ch_settings: &[(u8, u8)],
        _active_ch_mask: u8,
        bias_sensp: u8,
        bias_sensn: u8,
    ) -> Result<(), DriverError> {
        self.send_command(CMD_RESET)?;
        thread::sleep(Duration::from_millis(10));
        self.send_command(CMD_SDATAC)?;  // keep this as a safe guard against peramently damaging the chip (MISO line)
        thread::sleep(Duration::from_millis(10));

        let id = self.read_register(REG_ID_ADDR)?;
        debug!("Read device ID: 0x{:02X}", id);
        if id != 0x3E {
            warn!("Invalid device ID: 0x{:02X} (expected 0x3E)", id);
            return Err(DriverError::HardwareNotFound(format!(
                "Invalid device ID: 0x{:02X}", id
            )));
        }

        self.write_register(CONFIG1_ADDR, config1)?;
        self.write_register(CONFIG2_ADDR, config2)?;
        self.write_register(CONFIG3_ADDR, config3)?;
        self.write_register(CONFIG4_ADDR, config4)?;
        self.write_register(LOFF_SENSP_ADDR, loff)?;
        self.write_register(MISC1_ADDR, misc1)?;
        for &(addr, value) in ch_settings {
            self.write_register(addr, value)?;
        }
        self.write_register(BIAS_SENSP_ADDR, bias_sensp)?;
        self.write_register(BIAS_SENSN_ADDR, bias_sensn)?;

        thread::sleep(Duration::from_millis(10));
        for reg in 0x00..=0x17 {
            let val = self.read_register(reg)?;
            info!("Reg 0x{:02X}: 0x{:02X}", reg, val);
        }

        thread::sleep(Duration::from_micros(50));
        self.send_command(CMD_STANDBY)?;
        thread::sleep(Duration::from_micros(50));
        self.inner.lock().unwrap().status = DriverStatus::Ok;
        Ok(())
    }

    pub fn read_register(&self, register: u8) -> Result<u8, DriverError> {
        let _inner = self.inner.lock().unwrap();
        // Now we can access the public 'spi' field
        let spi = self.bus.spi.lock().unwrap();

        // Command (0x20 | reg), num registers-1 (0x00), dummy byte for clocking
        let write_buf = [0x20 | (register & 0x1F), 0x00, 0x00];
        let mut read_buf = [0u8; 3];

        spi.transfer(&mut read_buf, &write_buf)
            .map_err(|e| DriverError::SpiError(e.to_string()))?;
        Ok(read_buf[2])
    }


    /// Write a value to a register in the ADS1299.
    fn write_register(&self, register: u8, value: u8) -> Result<(), DriverError> {
        let mut inner = self.inner.lock().unwrap();
        let command = 0x40 | (register & 0x1F);
        let write_buf = [command, 0x00, value]; // WREG command, num registers-1, value

        let result = self.bus.write(&mut inner.cs_pin, &write_buf);

        if result.is_ok() {
            inner.registers[register as usize] = value;
        }
        result
    }

    /// Reads a single frame of data from the chip.
    /// Reads a single frame of data from the chip.
    pub fn read_frame(&self, inner: &mut Ads1299Inner, buffer: &mut [u8]) -> Result<(), SensorError> {
        self.bus
            .transfer(&mut inner.cs_pin, buffer)
            .map_err(|e| SensorError::HardwareFault(e.to_string()))
    }
}

// Implement the AdcDriver trait
impl crate::types::AdcDriver for Ads1299Driver {
    fn initialize(&mut self) -> Result<(), DriverError> {
        Err(DriverError::ConfigurationError(
            "Ads1299Driver cannot be initialized directly. Use a board driver.".to_string(),
        ))
    }

    fn acquire_batched(
        &mut self,
        _batch_size: usize,
        _stop_flag: &AtomicBool,
    ) -> Result<(Vec<i32>, u64), SensorError> {
        unimplemented!("This will be implemented by the board-specific drivers (ElataV1/V2)");
    }

    fn get_status(&self) -> DriverStatus {
        self.inner.lock().unwrap().status.clone()
    }

    fn get_config(&self) -> Result<AdcConfig, DriverError> {
        // This needs to be reconstructed from the inner ChipConfig
        // For now, return a default or error, as this driver is now chip-specific
        Err(DriverError::ConfigurationError("Cannot get global AdcConfig from a single chip driver".to_string()))
    }

    fn shutdown(&mut self) -> Result<(), DriverError> {
        debug!("Shutting down Ads1299Driver for CS pin {}", self.inner.lock().unwrap().config.cs_pin);
        let mut inner = self.inner.lock().unwrap();
        inner.running = false;
        inner.status = DriverStatus::NotInitialized;
        Ok(())
    }
}

impl Drop for Ads1299Driver {
    fn drop(&mut self) {
        if self.get_status() != DriverStatus::NotInitialized {
            warn!("Ads1299Driver dropped without calling shutdown() first.");
            let _ = self.shutdown();
        }
    }
}