use std::error::Error;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::thread::{JoinHandle};
use std::time::Duration;
use crossbeam_channel::{select, bounded, Sender};
use log::{info, error, warn};
use thread_priority::{ThreadPriority};

use sensors::{AdcConfig, AdcDriver, DriverError, DriverStatus, raw::ads1299::Ads1299Driver};
use eeg_types::{BridgeMsg, SensorError};
use pipeline::data::Packet;
use sensors::ads1299::registers::{
    self, CHN_OFF, CHN_REG, CONFIG1_REG, CONFIG2_REG, CONFIG3_REG, CONFIG4_REG, LOFF_SESP_REG,
    MISC1_REG, BIAS_SENSN_REG_MASK, CH1SET_ADDR,
};

pub struct ElataV2Driver {
    chip1: Ads1299Driver,
    chip2: Ads1299Driver,
    status: Arc<Mutex<DriverStatus>>,
    config: AdcConfig,
}

impl ElataV2Driver {
    pub fn new(config: AdcConfig) -> Result<Self, Box<dyn Error>> {
        if config.chips.len() != 2 {
            return Err(Box::new(DriverError::ConfigurationError(
                "ElataV2Driver requires exactly two chip configurations".to_string(),
            )));
        }

        // Create a config for the first chip
        let mut chip1_config = config.clone();
        chip1_config.chips = vec![config.chips[0].clone()];
        chip1_config.channels = chip1_config.chips[0].channels.clone(); // Keep legacy field in sync
        let chip1 = Ads1299Driver::new(chip1_config)?;

        // Create a config for the second chip
        let mut chip2_config = config.clone();
        chip2_config.chips = vec![config.chips[1].clone()];
        chip2_config.channels = chip2_config.chips[0].channels.clone(); // Keep legacy field in sync
        let chip2 = Ads1299Driver::new(chip2_config)?;

        Ok(Self {
            chip1,
            chip2,
            status: Arc::new(Mutex::new(DriverStatus::Stopped)),
            config,
        })
    }
}

impl AdcDriver for ElataV2Driver {
    fn initialize(&mut self) -> Result<(), DriverError> {
        info!("Initializing ElataV2 board...");

        // --- Common Settings ---
        let gain_mask = registers::gain_to_reg_mask(self.config.gain)?;
        let sps_mask = registers::sps_to_reg_mask(self.config.sample_rate)?;
        let active_ch_mask = self.config.channels.iter().fold(0, |acc, &ch| acc | (1 << (ch % 8)));

        // --- Chip 1 Setup (Master, Daisy-Chain Enabled) ---
        let config1_chip1 = CONFIG1_REG | sps_mask | 0x40; // Set DAISY_EN bit
        let ch_settings_chip1: Vec<(u8, u8)> = (0..8)
            .map(|i| (CH1SET_ADDR + i, if self.config.channels.contains(&i) { CHN_REG | gain_mask } else { CHN_OFF }))
            .collect();

        self.chip1.initialize_chip(
            config1_chip1,
            CONFIG2_REG,
            CONFIG3_REG,
            CONFIG4_REG,
            LOFF_SESP_REG,
            MISC1_REG,
            &ch_settings_chip1,
            active_ch_mask,
            BIAS_SENSN_REG_MASK,
        )?;
        info!("Chip 1 initialized.");

        // --- Chip 2 Setup (Slave) ---
        let config1_chip2 = CONFIG1_REG | sps_mask; // DAISY_EN is 0
        let ch_settings_chip2: Vec<(u8, u8)> = (0..8)
            .map(|i| (CH1SET_ADDR + i, if self.config.channels.contains(&(i + 8)) { CHN_REG | gain_mask } else { CHN_OFF }))
            .collect();
        
        let active_ch_mask_chip2 = self.config.channels.iter()
            .filter(|&&ch| ch >= 8)
            .fold(0, |acc, &ch| acc | (1 << (ch % 8)));

        self.chip2.initialize_chip(
            config1_chip2,
            CONFIG2_REG,
            CONFIG3_REG,
            CONFIG4_REG,
            LOFF_SESP_REG,
            MISC1_REG,
            &ch_settings_chip2,
            active_ch_mask_chip2,
            BIAS_SENSN_REG_MASK,
        )?;
        info!("Chip 2 initialized.");

        // Put chips in standby after initialization
        self.chip1.send_command(sensors::ads1299::registers::CMD_STANDBY)?;
        self.chip2.send_command(sensors::ads1299::registers::CMD_STANDBY)?;

        info!("ElataV2 board initialized successfully.");
        Ok(())
    }

    fn acquire(
        &mut self,
        tx: Sender<BridgeMsg>,
        stop_flag: &AtomicBool,
    ) -> Result<(), SensorError> {
        *self.status.lock().unwrap() = DriverStatus::Running;
        // Use bounded channels for backpressure
        let (tx1, rx1) = bounded::<BridgeMsg>(10);
        let (tx2, rx2) = bounded::<BridgeMsg>(10);

        let stop_flag1 = Arc::new(AtomicBool::new(false));
        let stop_flag2 = Arc::new(AtomicBool::new(false));

        let mut chip1 = self.chip1.clone();
        let thread_stop_flag1 = stop_flag1.clone();
        let thread_handle1: JoinHandle<Result<(), SensorError>> = std::thread::spawn(move || {
            if let Err(e) = thread_priority::set_current_thread_priority(ThreadPriority::Max) {
                warn!("Failed to set thread priority for chip 1: {:?}", e);
            }
            chip1.acquire(tx1, &thread_stop_flag1)
        });

        let mut chip2 = self.chip2.clone();
        let thread_stop_flag2 = stop_flag2.clone();
        let thread_handle2: JoinHandle<Result<(), SensorError>> = std::thread::spawn(move || {
            if let Err(e) = thread_priority::set_current_thread_priority(ThreadPriority::Max) {
                warn!("Failed to set thread priority for chip 2: {:?}", e);
            }
            chip2.acquire(tx2, &thread_stop_flag2)
        });

        // Time-gated data merging loop
        let merge_timeout = Duration::from_millis(5); // Max wait for the second packet

        while !stop_flag.load(Ordering::Relaxed) {
            select! {
                recv(rx1) -> msg1 => {
                    match msg1 {
                        Ok(BridgeMsg::Data(packet1)) => {
                            // Received from chip 1, now wait for chip 2
                            select! {
                                recv(rx2) -> msg2 => {
                                    match msg2 {
                                        Ok(BridgeMsg::Data(packet2)) => {
                                            // Both packets received, merge and send
                                            if let (Packet::RawI32(mut data1), Packet::RawI32(data2)) = (packet1, packet2) {
                                                data1.samples.extend(data2.samples);
                                                if tx.send(BridgeMsg::Data(Packet::RawI32(data1))).is_err() {
                                                    break;
                                                }
                                            } else {
                                                error!("Mismatched packet types from chips during merge.");
                                            }
                                        },
                                        Ok(BridgeMsg::Error(e)) => error!("Chip 2 error: {:?}", e),
                                        Err(_) => break, // Channel disconnected
                                    }
                                },
                                default(merge_timeout) => {
                                    warn!("Timed out waiting for chip 2 packet. Discarding chip 1 packet.");
                                }
                            }
                        },
                        Ok(BridgeMsg::Error(e)) => error!("Chip 1 error: {:?}", e),
                        Err(_) => break, // Channel disconnected
                    }
                },
                recv(rx2) -> msg2 => {
                     match msg2 {
                        Ok(BridgeMsg::Data(_)) => {
                            // This case should ideally not happen if chip 1 is master
                            warn!("Received packet from chip 2 before chip 1. Discarding.");
                        },
                        Ok(BridgeMsg::Error(e)) => error!("Chip 2 error: {:?}", e),
                        Err(_) => break, // Channel disconnected
                    }
                },
                default(Duration::from_secs(1)) => {
                    // Heartbeat to check stop_flag if no data is coming in
                    if stop_flag.load(Ordering::Relaxed) {
                        break;
                    }
                }
            }
        }

        stop_flag1.store(true, Ordering::Relaxed);
        stop_flag2.store(true, Ordering::Relaxed);

        if let Err(e) = thread_handle1.join().unwrap() {
             error!("Chip 1 acquisition thread failed: {:?}", e);
        }
        if let Err(e) = thread_handle2.join().unwrap() {
             error!("Chip 2 acquisition thread failed: {:?}", e);
        }

        *self.status.lock().unwrap() = DriverStatus::Stopped;
        Ok(())
    }

    fn get_status(&self) -> DriverStatus {
        self.status.lock().unwrap().clone()
    }

    fn get_config(&self) -> Result<AdcConfig, DriverError> {
        Ok(self.config.clone())
    }

    fn shutdown(&mut self) -> Result<(), DriverError> {
        self.chip1.shutdown()?;
        self.chip2.shutdown()?;
        *self.status.lock().unwrap() = DriverStatus::NotInitialized;
        Ok(())
    }
}
