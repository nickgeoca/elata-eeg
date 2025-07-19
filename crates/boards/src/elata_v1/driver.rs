use eeg_types::{BridgeMsg, SensorError};
use pipeline::data::SensorMeta;
use log::info;
use rppal::gpio::Gpio;
use rppal::spi::{Bus, Mode};
use sensors::{
    ads1299::{
        driver::Ads1299Driver,
        registers::{
            self, BIAS_SENSN_REG, CHN_OFF, CHN_REG, CONFIG1_REG, CONFIG2_REG, CONFIG3_REG,
            CONFIG4_REG, LOFF_SESP_REG, MISC1_REG, CH1SET_ADDR, CMD_RDATAC, CMD_START, CMD_WAKEUP,
BIASREF_INT , PD_BIAS , PD_REFBUF, DC_TEST, SRB1        },
    },
    AdcConfig, AdcDriver, DriverError, DriverStatus, spi_bus::SpiBus,
};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use flume::{Receiver, Sender};
use std::time::{SystemTime, UNIX_EPOCH};
use pipeline::data::PacketOwned;

pub struct ElataV1Driver {
    inner: Ads1299Driver,
    config: AdcConfig,
    drdy_tx: Sender<()>,
}

impl ElataV1Driver {
    pub fn new(config: AdcConfig) -> Result<Self, DriverError> {
        let chip_config = config.chips.get(0).ok_or_else(|| {
            DriverError::ConfigurationError("At least one chip must be configured for ElataV1".to_string())
        })?;

        let spi_bus = Arc::new(SpiBus::new(Bus::Spi0, 1_000_000, Mode::Mode1)?);
        let gpio = Arc::new(Gpio::new().map_err(|e| DriverError::GpioError(e.to_string()))?);

        let sensor_meta = Arc::new(SensorMeta {
            v_ref: config.vref,
            gain: config.gain,
            sample_rate: config.sample_rate,
            adc_bits: 24, // ADS1299 is a 24-bit ADC
            ..Default::default()
        });

        let cs_pin = gpio.get(chip_config.cs_pin)?.into_output();
        let (drdy_tx, drdy_rx): (Sender<()>, Receiver<()>) = flume::bounded(1);
        let inner = Ads1299Driver::new(chip_config.clone(), spi_bus, cs_pin, drdy_rx, sensor_meta)?;
        Ok(Self { inner, config, drdy_tx })
    }
}

impl AdcDriver for ElataV1Driver {
    fn initialize(&mut self) -> Result<(), DriverError> {
        info!("Initializing ElataV1 board...");

        // Reset the chip first to ensure it's in a known state
        self.inner.send_command(sensors::ads1299::registers::CMD_RESET)?;
        std::thread::sleep(std::time::Duration::from_millis(10)); // Wait for reset

        let gain_mask = registers::gain_to_reg_mask(self.config.gain)?;
        let sps_mask = registers::sps_to_reg_mask(self.config.sample_rate)?;
        let active_ch_mask = self.config.channels.iter().fold(0, |acc, &ch| acc | (1 << ch));
        let ch_settings: Vec<(u8, u8)> = (0..8)
            .map(|i| {
                (
                    CH1SET_ADDR + i,
                    if self.config.channels.contains(&i) {
                        CHN_REG | gain_mask
                    } else {
                        CHN_OFF
                    },
                )
            })
            .collect();
 
        self.inner.initialize_chip(
            CONFIG1_REG | sps_mask,
            CONFIG2_REG | DC_TEST,
            CONFIG3_REG | BIASREF_INT | PD_BIAS | PD_REFBUF,
            CONFIG4_REG,
            LOFF_SESP_REG,
            MISC1_REG | SRB1,
            &ch_settings,
            active_ch_mask,
            BIAS_SENSN_REG,
            active_ch_mask,
        )?;

        self.inner
            .send_command(sensors::ads1299::registers::CMD_STANDBY)?;

        info!("ElataV1 board initialized successfully.");
        Ok(())
    }

    fn acquire(
        &mut self,
        tx: Sender<BridgeMsg>,
        stop_flag: &AtomicBool,
    ) -> Result<(), SensorError> {
        // Wake up the chip and start data acquisition
        self.inner.send_command(CMD_WAKEUP)
            .map_err(|e| SensorError::HardwareFault(format!("Failed to send WAKEUP: {}", e)))?;
        self.inner.send_command(CMD_START)
            .map_err(|e| SensorError::HardwareFault(format!("Failed to send START: {}", e)))?;
        self.inner.send_command(CMD_RDATAC)
            .map_err(|e| SensorError::HardwareFault(format!("Failed to send RDATAC: {}", e)))?;
        info!("ElataV1 board is now acquiring data.");

        // Create a channel to bridge packets from the acquisition loop to the main pipeline
        let (packet_tx, packet_rx) = flume::unbounded::<(u8, pipeline::data::PacketOwned)>();
        let stop_flag_arc = Arc::new(AtomicBool::new(stop_flag.load(Ordering::Relaxed)));

        let initial_timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let increment_ns = (1_000_000_000.0 / self.config.sample_rate as f64) as u64;
        let mut packet_index: u64 = 0;

        // Spawn a dedicated thread to forward packets.
        let bridge_tx = tx.clone();
        let bridge_thread = std::thread::spawn(move || {
            while let Ok((_chip_id, packet)) = packet_rx.recv() {
                if let pipeline::data::PacketOwned::RawI32(mut data) = packet {
                    // Override timestamp with incremental one
                    data.header.ts_ns = initial_timestamp_ns + (packet_index * increment_ns);

                    if bridge_tx.send(BridgeMsg::Data(PacketOwned::RawI32(data))).is_err() {
                        break;
                    }
                    packet_index += 1;
                }
            }
        });

        // For ElataV1, we simulate the DRDY signal with a ticker thread.
        let sample_rate = self.config.sample_rate;
        let interval = std::time::Duration::from_secs_f64(1.0 / sample_rate as f64);
        let drdy_tx = self.drdy_tx.clone();
        let stop_flag_ticker_clone = stop_flag_arc.clone();
        let ticker_thread = std::thread::spawn(move || {
            while !stop_flag_ticker_clone.load(Ordering::Relaxed) {
                if drdy_tx.send(()).is_err() {
                    break;
                }
                std::thread::sleep(interval);
            }
        });

        // Run the acquisition loop directly in the current thread.
        let result = self.inner.acquire_raw(packet_tx, &stop_flag_arc, 0);

        // Wait for the threads to finish.
        let _ = bridge_thread.join();
        let _ = ticker_thread.join();

        result
    }

    fn get_status(&self) -> DriverStatus {
        self.inner.get_status()
    }

    fn get_config(&self) -> Result<AdcConfig, DriverError> {
        self.inner.get_config()
    }

    fn shutdown(&mut self) -> Result<(), DriverError> {
        self.inner.shutdown()
    }
}