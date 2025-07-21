use std::collections::HashMap;
use std::error::Error;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flume::{Receiver, Sender, Selector};
use log::{debug, error, info, warn};
use rppal::gpio::{Gpio, InputPin, OutputPin};
use rppal::spi::{Bus, Mode};
use thread_priority::ThreadPriority;

use eeg_types::data::{PacketData, PacketOwned, SensorMeta};
use eeg_types::SensorError;
use sensors::{
    ads1299::registers::{
        self, BIAS_SENSN_REG, BIAS_SENSP_REG, CH1SET_ADDR, CHN_OFF, CHN_REG, CMD_RDATAC,
        CMD_RESET, CMD_SDATAC, CMD_STANDBY, CMD_WAKEUP, CONFIG1_REG, CMD_START,
        CONFIG2_REG, CONFIG3_REG, CONFIG4_REG, LOFF_SESP_REG, MISC1_REG, DAISY_DISABLE,
    BIASREF_INT , PD_BIAS, PD_REFBUF, BIAS_SENS_OFF_MASK, DC_TEST, SRB1, MUX_NORMAL, POWER_OFF_CH},
    AdcConfig, AdcDriver, DriverError, DriverStatus,
    ads1299::driver::Ads1299Driver, spi_bus::SpiBus,
};

const NUM_CHIPS: usize = 1;
const START_PIN: u8 = 22; // GPIO for START pulse

pub struct ElataV2Driver {
    chip_drivers: Vec<Ads1299Driver>,
    gpio: Arc<Gpio>,
    status: Arc<Mutex<DriverStatus>>,
    config: AdcConfig,
    drdy_tx: Sender<usize>,
    drdy_rx: Receiver<usize>,
    start_pin: Arc<Mutex<Option<OutputPin>>>,
}

impl ElataV2Driver {
    pub fn new(config: AdcConfig) -> Result<Self, Box<dyn Error>> {
        if config.chips.len() != NUM_CHIPS {
            return Err(Box::new(DriverError::ConfigurationError(format!(
                "ElataV2Driver requires exactly {} chip configurations",
                NUM_CHIPS
            ))));
        }

        let spi_bus = Arc::new(SpiBus::new(
            Bus::Spi0,
            1_240_000,
            Mode::Mode1,
        )?);
        info!("SPI bus initialized.");

        let gpio = Arc::new(Gpio::new()?);
        info!("GPIO initialized.");

        let mut chip_drivers = Vec::with_capacity(NUM_CHIPS);
        let (drdy_tx, drdy_rx) = flume::bounded(128 * NUM_CHIPS);

        for (i, chip_config) in config.chips.iter().enumerate() {
            let cs_pin = gpio.get(chip_config.cs_pin)?.into_output();
            let driver = Ads1299Driver::new(
                chip_config.clone(),
                spi_bus.clone(),
                cs_pin,
            )?;
            chip_drivers.push(driver);
        }

        Ok(Self {
            chip_drivers,
            gpio,
            status: Arc::new(Mutex::new(DriverStatus::Stopped)),
            config,
            drdy_tx,
            drdy_rx,
            start_pin: Arc::new(Mutex::new(None)),
        })
    }
}

impl AdcDriver for ElataV2Driver {
    fn initialize(&mut self) -> Result<(), DriverError> {
        info!("Initializing ElataV2 board with {} chips...", NUM_CHIPS);

        // 1. Initialize chip registers first
        for (i, chip) in self.chip_drivers.iter_mut().enumerate() {
            info!("Initializing Chip {}...", i);
            let chip_info = &self.config.chips[i];

            let gain_mask = registers::gain_to_reg_mask(chip_info.gain)?;
            let sps_mask = registers::sps_to_reg_mask(self.config.sample_rate)?;
            let pd_bias = if i == 0 { PD_BIAS } else { 0x00 };

            let ch_settings: Vec<(u8, u8)> = (0..8)
                .map(|ch_idx| {
                    let setting = if chip_info.channels.contains(&ch_idx) {
                        CHN_REG | MUX_NORMAL | gain_mask
                    } else {
                        POWER_OFF_CH
                    };
                    (CH1SET_ADDR + ch_idx, setting)
                })
                .collect();
            let active_ch_mask = chip_info.channels.iter().fold(0, |acc, &ch| acc | (1 << (ch % 8)));
            chip.initialize_chip(
                CONFIG1_REG | sps_mask | DAISY_DISABLE,
                CONFIG2_REG,
                CONFIG3_REG | BIASREF_INT | PD_REFBUF | pd_bias,
                CONFIG4_REG,
                LOFF_SESP_REG,
                MISC1_REG | SRB1,
                &ch_settings,
                active_ch_mask,
                BIAS_SENSN_REG,
                active_ch_mask,
            )?;
            info!("Chip {} initialized and ready.", i);
        }

        thread::sleep(Duration::from_millis(10));

        // 2. Spawn DRDY handler threads *before* starting data acquisition
        for i in 0..NUM_CHIPS {
            let chip_config = &self.config.chips[i];
            let drdy_pin = self.gpio.get(chip_config.drdy_pin)?.into_input_pullup();
            let drdy_tx = self.drdy_tx.clone();
            thread::Builder::new()
                .name(format!("drdy_handler_{}", i))
                .spawn(move || {
                    let mut last_drdy = Instant::now();
                    loop {
                        // Basic edge detection: wait for pin to go low
                        if drdy_pin.is_low() {
                            // Basic debouncing
                            if last_drdy.elapsed() > Duration::from_micros(200) {
                                if drdy_tx.send(i).is_err() {
                                    // Main receiver has dropped, exit thread
                                    break;
                                }
                                last_drdy = Instant::now();
                            }
                        }
                        // A small sleep to prevent pegging the CPU
                        thread::sleep(Duration::from_micros(50));
                    }
                })
                .map_err(|e| DriverError::Other(format!("Failed to spawn DRDY handler thread: {}", e)))?;
            info!("DRDY polling handler started for chip {}", i);
        }

        // 3. Now, start data acquisition on all chips
        let mut start_pin = self.gpio.get(START_PIN)?.into_output();
        start_pin.set_low();

        for chip in self.chip_drivers.iter_mut() {
            chip.send_command(CMD_WAKEUP)?;
            thread::sleep(Duration::from_micros(50));
        }

        // Pulse START pin to begin conversions
        thread::sleep(Duration::from_millis(1));
        start_pin.set_high();
        thread::sleep(Duration::from_millis(1));
        for chip in self.chip_drivers.iter_mut() {
            chip.send_command(CMD_RDATAC)?;
            thread::sleep(Duration::from_micros(50));
        }

        // Store the pin so it's not dropped and its state is maintained
        *self.start_pin.lock().unwrap() = Some(start_pin);

        info!("ElataV2 board initialized successfully and is acquiring data.");
        Ok(())
    }

    fn acquire_batched(
        &mut self,
        batch_size: usize,
        stop_flag: &AtomicBool,
    ) -> Result<(Vec<i32>, u64), SensorError> {
        *self.status.lock().unwrap() = DriverStatus::Running;

        let total_channels: usize = self.config.chips.iter().map(|c| c.channels.len()).sum();
        if total_channels == 0 {
            return Ok((Vec::new(), 0));
        }
        let mut batch_buffer: Vec<i32> = Vec::with_capacity(batch_size * total_channels);
        let mut first_drdy_timestamp = 0;

        for i in 0..batch_size {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            // Wait for a DRDY signal from any chip
            match self.drdy_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(chip_index) => {
                    if i == 0 {
                        first_drdy_timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
                    }
                    // A chip is ready, read its data
                    let driver = &mut self.chip_drivers[chip_index];
                    match driver.read_data_raw() {
                        Ok(data) => {
                            debug!(
                                "[Chip {}] Acquired {} samples.",
                                chip_index,
                                data.len() / self.config.chips[chip_index].channels.len()
                            );
                            batch_buffer.extend(data);
                        }
                        Err(e) => {
                            error!("[Chip {}] Failed to read data: {}", chip_index, e);
                            // Optional: decide if we should break or continue
                        }
                    }
                }
                Err(flume::RecvTimeoutError::Timeout) => {
                    warn!("DRDY timeout. No data received in 500ms.");
                    continue; // Or break, depending on desired behavior
                }
                Err(flume::RecvTimeoutError::Disconnected) => {
                    error!("DRDY channel disconnected. Stopping acquisition.");
                    break;
                }
            }
        }

        *self.status.lock().unwrap() = DriverStatus::Stopped;
        Ok((batch_buffer, first_drdy_timestamp))
    }

    fn get_status(&self) -> DriverStatus {
        self.status.lock().unwrap().clone()
    }

    fn get_config(&self) -> Result<AdcConfig, DriverError> {
        Ok(self.config.clone())
    }

    fn shutdown(&mut self) -> Result<(), DriverError> {
        info!("Shutting down ElataV2 board...");

        // Take START pin low to stop conversions
        if let Some(mut start_pin) = self.start_pin.lock().unwrap().take() {
            start_pin.set_low();
            info!("START pin set to low.");
        }

        for (i, chip) in self.chip_drivers.iter_mut().enumerate() {
            info!("Sending SDATAC and STANDBY to chip {}", i);
            chip.send_command(CMD_SDATAC)?;
            chip.send_command(CMD_STANDBY)?;
            chip.shutdown()?;
        }
        *self.status.lock().unwrap() = DriverStatus::NotInitialized;
        info!("ElataV2 board shut down.");
        Ok(())
    }
}
