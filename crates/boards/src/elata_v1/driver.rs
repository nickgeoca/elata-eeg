use eeg_types::{BridgeMsg, SensorError};
use log::info;
use sensors::{
    ads1299::{
        driver::Ads1299Driver,
        registers::{
            self, BIAS_SENSN_REG_MASK, CHN_OFF, CHN_REG, CONFIG1_REG, CONFIG2_REG, CONFIG3_REG,
            CONFIG4_REG, LOFF_SESP_REG, MISC1_REG, CH1SET_ADDR,
        },
    },
    AdcConfig, AdcDriver, DriverError, DriverStatus, DriverType,
};
use std::sync::atomic::AtomicBool;
use crossbeam_channel::Sender;

pub struct ElataV1Driver {
    inner: Ads1299Driver,
    config: AdcConfig,
}

impl ElataV1Driver {
    pub fn new(config: AdcConfig) -> Result<Self, DriverError> {
        let mut v1_config = config.clone();
        v1_config.board_driver = DriverType::Ads1299;
        let inner = Ads1299Driver::new(v1_config)?;
        Ok(Self { inner, config })
    }
}

impl AdcDriver for ElataV1Driver {
    fn initialize(&mut self) -> Result<(), DriverError> {
        info!("Initializing ElataV1 board...");

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
            active_ch_mask,
            BIAS_SENSN_REG_MASK,
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
        self.inner.acquire(tx, stop_flag)
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