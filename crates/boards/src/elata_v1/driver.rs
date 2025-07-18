use eeg_types::{BridgeMsg, SensorError};
use pipeline::data::SensorMeta;
use log::info;
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use sensors::{
    ads1299::{
        driver::Ads1299Driver,
        registers::{
            self, BIAS_SENSN_REG_MASK, CHN_OFF, CHN_REG, CONFIG1_REG, CONFIG2_REG, CONFIG3_REG,
            CONFIG4_REG, LOFF_SESP_REG, MISC1_REG, CH1SET_ADDR, CMD_RDATAC, CMD_START, CMD_WAKEUP,
        },
    },
    AdcConfig, AdcDriver, DriverError, DriverStatus,
};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use flume::Sender;

pub struct ElataV1Driver {
    inner: Ads1299Driver,
    config: AdcConfig,
}

impl ElataV1Driver {
    pub fn new(config: AdcConfig) -> Result<Self, DriverError> {
        let chip_config = config.chips.get(0).ok_or_else(|| {
            DriverError::ConfigurationError("At least one chip must be configured for ElataV1".to_string())
        })?;

        let spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, 1_000_000, Mode::Mode1)
            .map_err(|e| DriverError::SpiError(e.to_string()))?;
        let spi = Arc::new(Mutex::new(spi));

        let sensor_meta = Arc::new(SensorMeta {
            v_ref: config.vref,
            gain: config.gain,
            sample_rate: config.sample_rate,
            adc_bits: 24, // ADS1299 is a 24-bit ADC
            ..Default::default()
        });

        let inner = Ads1299Driver::new(chip_config.clone(), spi, sensor_meta)?;
        Ok(Self { inner, config })
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

        let config1 = CONFIG1_REG | sps_mask; // No daisy chain

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
            config1,
            CONFIG2_REG,
            CONFIG3_REG,
            CONFIG4_REG,
            LOFF_SESP_REG,
            MISC1_REG,
            &ch_settings,
            active_ch_mask, // bias_sensp
            BIAS_SENSN_REG_MASK,
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
        let (packet_tx, packet_rx) = flume::unbounded::<pipeline::data::PacketOwned>();
        let stop_flag_arc = Arc::new(AtomicBool::new(stop_flag.load(Ordering::Relaxed)));

        // Spawn a dedicated thread to forward packets.
        let bridge_tx = tx.clone();
        let bridge_thread = std::thread::spawn(move || {
            while let Ok(packet) = packet_rx.recv() {
                if bridge_tx.send(BridgeMsg::Data(packet.into())).is_err() {
                    break;
                }
            }
        });

        // Run the acquisition loop directly in the current thread.
        let result = self.inner.acquire_raw(packet_tx, &stop_flag_arc, 0);

        // Wait for the bridge thread to finish.
        let _ = bridge_thread.join();

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