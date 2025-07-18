//! Main driver implementation for the ADS1299 chip.

use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use flume::Sender;
use log::{debug, info, warn};
use rppal::gpio::{Gpio, InputPin, OutputPin};
use rppal::spi::Spi;

use crate::types::ChipConfig;
use eeg_types::{BridgeMsg, SensorError};
use pipeline::data::{PacketData, PacketHeader, PacketOwned, SensorMeta};

use crate::types::{AdcConfig, AdcDriver, DriverError, DriverStatus};
use super::helpers::ch_sample_to_raw;
use super::registers::{
    BIAS_SENSN_ADDR, BIAS_SENSP_ADDR, CMD_RESET, CMD_SDATAC, CMD_STANDBY, CONFIG1_ADDR,
    CONFIG2_ADDR, CONFIG3_ADDR, CONFIG4_ADDR, LOFF_SENSP_ADDR, MISC1_ADDR, REG_ID_ADDR,
};
use super::spi::{wait_irq};

/// ADS1299 driver for interfacing with a single ADS1299 chip over a shared SPI bus.
#[derive(Clone)]
pub struct Ads1299Driver {
    inner: Arc<Mutex<Ads1299Inner>>,
    spi: Arc<Mutex<Spi>>,
}

/// Internal state for the Ads1299Driver.
pub struct Ads1299Inner {
    pub config: ChipConfig,
    pub cs_pin: OutputPin,
    pub drdy_pin: InputPin,
    pub running: bool,
    pub status: DriverStatus,
    pub registers: [u8; 24],
    pub sensor_meta: Arc<SensorMeta>,
}

impl Ads1299Driver {
    /// Creates a new driver instance for one ADS1299 chip.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration specific to this chip.
    /// * `spi` - A shared SPI device handle.
    pub fn new(
        config: ChipConfig,
        spi: Arc<Mutex<Spi>>,
        sensor_meta: Arc<SensorMeta>,
    ) -> Result<Self, DriverError> {
        let gpio = Gpio::new().map_err(|e| DriverError::GpioError(e.to_string()))?;
        let cs_pin = gpio
            .get(config.cs_pin)
            .map_err(|e| DriverError::GpioError(e.to_string()))?
            .into_output();
        let drdy_pin = gpio
            .get(config.drdy_pin)
            .map_err(|e| DriverError::GpioError(e.to_string()))?
            .into_input();

        let inner = Ads1299Inner {
            config,
            cs_pin,
            drdy_pin,
            running: false,
            status: DriverStatus::NotInitialized,
            registers: [0u8; 24],
            sensor_meta,
        };

        let driver = Ads1299Driver {
            inner: Arc::new(Mutex::new(inner)),
            spi,
        };
        
        let cs_pin = driver.inner.lock().unwrap().config.cs_pin;
        info!("Ads1299Driver created for CS pin {} (GPIO{})", cs_pin, cs_pin);
        debug!("CS pin initialized to {} (high)", driver.inner.lock().unwrap().cs_pin.is_set_high());
        Ok(driver)
    }

    /// Send a command to the ADS1299, handling CS and SPI bus locking.
    pub fn send_command(&self, command: u8) -> Result<(), DriverError> {
        let mut inner = self.inner.lock().unwrap();
        let mut spi = self.spi.lock().unwrap();

        inner.cs_pin.set_low();
        spi.write(&[command])
            .map_err(|e| DriverError::SpiError(e.to_string()))?;
        inner.cs_pin.set_high();
        Ok(())
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
        
        // Delay to allow reset to complete. The working python script sends 3 null bytes
        {
            let mut spi = self.spi.lock().unwrap();
            spi.write(&[0x00, 0x00, 0x00])
                .map_err(|e| DriverError::SpiError(e.to_string()))?;
        }

        self.send_command(CMD_SDATAC)?;
 
        debug!("Reading device ID...");
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

        self.inner.lock().unwrap().status = DriverStatus::Ok;
        Ok(())
    }

    /// Read a register from the ADS1299.
    pub fn read_register(&self, register: u8) -> Result<u8, DriverError> {
        let mut inner = self.inner.lock().unwrap();
        let mut spi = self.spi.lock().unwrap();
        let mut read_buffer = [0u8; 1];

        let command = 0x20 | (register & 0x1F);
        let write_buf = [command, 0x00]; // RREG command, num registers-1

        debug!("SPI read_register 0x{:02X} - CS low", register);
        inner.cs_pin.set_low();
        // Per datasheet, wait 4 tCLK cycles after command before reading.
        // At 1MHz SPI, tCLK is 1us. 5us is a safe margin.
        thread::sleep(Duration::from_micros(5));

        // Send the command to read the register
        spi.write(&write_buf)
            .map_err(|e| DriverError::SpiError(e.to_string()))?;

        // Send a dummy byte to clock in the register value
        spi.read(&mut read_buffer)
            .map_err(|e| DriverError::SpiError(e.to_string()))?;

        inner.cs_pin.set_high();
        debug!("SPI read_register result: Ok(0x{:02X})", read_buffer[0]);

        Ok(read_buffer[0])
    }

    /// Write a value to a register in the ADS1299.
    fn write_register(&self, register: u8, value: u8) -> Result<(), DriverError> {
        let mut inner = self.inner.lock().unwrap();
        let mut spi = self.spi.lock().unwrap();

        let command = 0x40 | (register & 0x1F);
        let write_buf = [command, 0x00, value]; // WREG command, num registers-1, value

        inner.cs_pin.set_low();
        // Add a small delay for stability, similar to read_register
        thread::sleep(Duration::from_micros(5));

        let result = spi.write(&write_buf)
            .map_err(|e| DriverError::SpiError(e.to_string()));

        inner.cs_pin.set_high();

        if result.is_ok() {
            inner.registers[register as usize] = value;
        }

        result.map(|_| ())
    }

    /// Acquire data from this chip and send it down the channel.
    pub fn acquire_raw(
        &mut self,
        tx: Sender<PacketOwned>,
        stop_flag: &Arc<AtomicBool>,
        chip_id: u8,
    ) -> Result<(), SensorError> {
        let num_channels = self.inner.lock().unwrap().config.channels.len();
        let packet_size = 3 + (num_channels * 3); // status + 3 bytes/channel

        {
            let mut inner = self.inner.lock().unwrap();
            inner.running = true;
            inner.status = DriverStatus::Running;
        }

        while !stop_flag.load(Ordering::Relaxed) {
            let drdy_event = {
                let mut inner = self.inner.lock().unwrap();
                wait_irq(&mut inner.drdy_pin, Duration::from_millis(100))
            };

            match drdy_event {
                Ok(true) => {
                    let mut buffer = vec![0u8; packet_size];

                    {
                        // SPI access block
                        let mut inner = self.inner.lock().unwrap();
                        let mut spi = self.spi.lock().unwrap();
                        inner.cs_pin.set_low();
                        spi.read(&mut buffer)
                            .map_err(|e| SensorError::HardwareFault(e.to_string()))?;
                        inner.cs_pin.set_high();
                    }

                    let mut samples = Vec::with_capacity(num_channels);
                    for i in 0..num_channels {
                        let start = 3 + i * 3;
                        let sample = ch_sample_to_raw(buffer[start], buffer[start + 1], buffer[start + 2]);
                        samples.push(sample);
                    }

                    let packet = PacketOwned::RawI32(PacketData {
                        header: PacketHeader {
                            ts_ns: SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_nanos() as u64,
                            batch_size: samples.len() as u32,
                            meta: self.inner.lock().unwrap().sensor_meta.clone(),
                        },
                        samples,
                    });

                    if tx.send(packet).is_err() {
                        warn!("[Chip {}] Acquisition channel closed", chip_id);
                        break;
                    }
                }
                Ok(false) => continue, // Timeout
                Err(e) => {
                    let err = SensorError::HardwareFault(format!("[Chip {}] DRDY pin error: {}", chip_id, e));
                    return Err(err);
                }
            }
        }
        Ok(())
    }
}

// Implement the AdcDriver trait
impl crate::types::AdcDriver for Ads1299Driver {
    fn initialize(&mut self) -> Result<(), DriverError> {
        Err(DriverError::ConfigurationError(
            "Ads1299Driver cannot be initialized directly. Use a board driver.".to_string(),
        ))
    }

    fn acquire(&mut self, _tx: Sender<BridgeMsg>, _stop_flag: &AtomicBool) -> Result<(), SensorError> {
        Err(SensorError::HardwareFault(
            "Use acquire_raw for multi-chip setups".to_string(),
        ))
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
        self.send_command(CMD_STANDBY)?;
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