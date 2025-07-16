use std::collections::HashMap;
use std::error::Error;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, select, Sender};
use log::{error, info, warn};
use rppal::gpio::{Gpio, OutputPin};
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use thread_priority::ThreadPriority;
use sensors::ads1299::registers::CMD_RESET;

use eeg_types::{BridgeMsg, SensorError};
use pipeline::data::{Packet, PacketData};
use sensors::{
    ads1299::registers::{
        self, BIAS_SENSN_REG_MASK, BIAS_SENSP_REG_MASK, CH1SET_ADDR, CHN_OFF, CHN_REG, CMD_RDATAC, PD_BIAS,
        CMD_SDATAC, CMD_STANDBY, CMD_WAKEUP, CONFIG1_REG, CONFIG2_REG, CONFIG3_REG, CONFIG4_REG,
        LOFF_SESP_REG, MISC1_REG,
    },
    AdcConfig, AdcDriver, DriverError, DriverStatus,
    raw::ads1299::Ads1299Driver,
};

const NUM_CHIPS: usize = 2;
const START_PIN: u8 = 22; // GPIO for START pulse

pub struct ElataV2Driver {
    chip_drivers: Vec<Ads1299Driver>,
    start_pin: OutputPin,
    status: Arc<Mutex<DriverStatus>>,
    config: AdcConfig,
}

impl ElataV2Driver {
    pub fn new(config: AdcConfig) -> Result<Self, Box<dyn Error>> {
        if config.chips.len() != NUM_CHIPS {
            return Err(Box::new(DriverError::ConfigurationError(format!(
                "ElataV2Driver requires exactly {} chip configurations",
                NUM_CHIPS
            ))));
        }

        let spi_config = Spi::new(
            Bus::Spi0,
            SlaveSelect::Ss1,
            128_000, // 2.048MHz/16 = 128kHz
            Mode::Mode1,
        )?;
        info!("SPI configured - Mode: {:?}, Speed: {:?} Hz, CS: {:?}",
            spi_config.mode(), spi_config.clock_speed(), SlaveSelect::Ss1);
        let spi = Arc::new(Mutex::new(spi_config));

        let mut chip_drivers = Vec::with_capacity(NUM_CHIPS);
        for chip_config in config.chips.iter() {
            let driver = Ads1299Driver::new(chip_config.clone(), spi.clone())?;
            chip_drivers.push(driver);
        }

        let mut start_pin = Gpio::new()?.get(START_PIN)?.into_output();
        start_pin.set_low(); // Prepare for START pulse

        Ok(Self {
            chip_drivers,
            start_pin,
            status: Arc::new(Mutex::new(DriverStatus::Stopped)),
            config,
        })
    }
}

impl AdcDriver for ElataV2Driver {
    fn initialize(&mut self) -> Result<(), DriverError> {
        info!("Initializing ElataV2 board with {} chips...", NUM_CHIPS);

        let gain_mask = registers::gain_to_reg_mask(self.config.gain)?;
        let sps_mask = registers::sps_to_reg_mask(self.config.sample_rate)?;

        for (i, chip) in self.chip_drivers.iter_mut().enumerate() {
            info!("Initializing Chip {}...", i);
            let chip_info = &self.config.chips[i];

            // --- Register Settings ---
            let config1 = CONFIG1_REG | sps_mask; // Use external clock
            let mut config3 = CONFIG3_REG;
            let mut bias_sensp = 0x00;
            let mut bias_sensn = 0x00;

            if i == 0 { // Board 0 is the bias master
                config3 |= PD_BIAS; // Enable bias buffer
                bias_sensp = BIAS_SENSP_REG_MASK; // Use all channels for bias
                bias_sensn = BIAS_SENSN_REG_MASK;
            } else { // Other boards are slaves
                config3 &= !PD_BIAS; // Disable bias buffer
            }

            let ch_settings: Vec<(u8, u8)> = (0..8)
                .map(|ch_idx| {
                    let channel_id = ch_idx + (i as u8 * 8);
                    let setting = if chip_info.channels.contains(&channel_id) {
                        CHN_REG | gain_mask
                    } else {
                        CHN_OFF
                    };
                    (CH1SET_ADDR + ch_idx, setting)
                })
                .collect();

            let active_ch_mask = chip_info.channels.iter().fold(0, |acc, &ch| acc | (1 << (ch % 8)));

            chip.initialize_chip(
                config1,
                CONFIG2_REG,
                config3,
                CONFIG4_REG,
                LOFF_SESP_REG,
                MISC1_REG,
                &ch_settings,
                active_ch_mask,
                bias_sensp,
                bias_sensn,
            )?;
            
            // Register dump for verification
            info!("---- Chip {} Register Dump ----", i);
            for reg in 0x00..=0x17 {
                let val = chip.read_register(reg)?;
                info!("Reg 0x{:02X}: 0x{:02X}", reg, val);
            }
            
            chip.send_command(CMD_STANDBY)?;
            info!("Chip {} initialized and in standby.", i);

            if i == 0 {
                // Add a small delay to allow the second chip to stabilize
                thread::sleep(Duration::from_millis(10));
            }
        }

        info!("ElataV2 board initialized successfully.");
        Ok(())
    }

    fn acquire(
        &mut self,
        tx: Sender<BridgeMsg>,
        stop_flag: &AtomicBool,
    ) -> Result<(), SensorError> {
        *self.status.lock().unwrap() = DriverStatus::Running;
        let (data_tx, data_rx) = bounded::<Packet>(NUM_CHIPS * 2); // Bounded channel for backpressure

        // --- Wake up and start all chips ---
        for chip in self.chip_drivers.iter_mut() {
            chip.send_command(CMD_WAKEUP)?;
            chip.send_command(CMD_RDATAC)?;
        }

        // --- Synchronize with START pulse ---
        thread::sleep(Duration::from_micros(4)); // Wait for t_CLK * 2
        self.start_pin.set_high();
        info!("START pulse sent. Acquisition running.");

        // --- Spawn acquisition threads ---
        let mut handles: Vec<JoinHandle<Result<(), SensorError>>> = Vec::new();
        let thread_stop_flag = Arc::new(AtomicBool::new(false));

        for (i, chip) in self.chip_drivers.iter_mut().enumerate() {
            let mut chip_clone = chip.clone();
            let thread_tx = data_tx.clone();
            let stop = thread_stop_flag.clone();
            let handle = thread::Builder::new()
                .name(format!("chip_{}_acq", i))
                .spawn(move || {
                    if let Err(e) = thread_priority::set_current_thread_priority(ThreadPriority::Max) {
                        warn!("[Chip {}] Failed to set thread priority: {:?}", i, e);
                    }
                    chip_clone.acquire_raw(thread_tx, &stop, i as u8)
                })
                .unwrap();
            handles.push(handle);
        }

        // --- Data merging loop ---
        let merge_timeout = Duration::from_millis(
            (1.0 / self.config.sample_rate as f32 * 1000.0 * 2.0) as u64,
        );

        while !stop_flag.load(Ordering::Relaxed) {
            let mut packet_buffer: HashMap<u8, Vec<i32>> = HashMap::with_capacity(NUM_CHIPS);
            let first_packet_deadline = Instant::now() + merge_timeout;

            // Collect packets from all chips
            while packet_buffer.len() < NUM_CHIPS {
                let timeout = first_packet_deadline.saturating_duration_since(Instant::now());
                if timeout.is_zero() {
                    warn!("Timed out waiting for a full frame of packets.");
                    break;
                }

                select! {
                    recv(data_rx) -> msg => {
                        if let Ok(Packet::RawI32(packet_data)) = msg {
                            // The chip_id is passed in the timestamp field of the header
                            let chip_id = packet_data.header.ts_ns as u8;
                            packet_buffer.insert(chip_id, packet_data.samples);
                        } else {
                            break; // Channel disconnected
                        }
                    },
                    default(timeout) => {
                        warn!("Timed out waiting for packet. Buffer has {}/{} packets.", packet_buffer.len(), NUM_CHIPS);
                        break;
                    }
                }
            }

            if packet_buffer.len() == NUM_CHIPS {
                // Packets are collected, merge them in order
                let mut merged_samples = Vec::with_capacity(8 * NUM_CHIPS);
                let final_header = Default::default();

                for i in 0..NUM_CHIPS {
                    if let Some(samples) = packet_buffer.get(&(i as u8)) {
                        if i == 0 {
                            // This is a hack until the packet format is updated
                            // final_header = packet_buffer.get(&0).unwrap().header.clone();
                        }
                        merged_samples.extend_from_slice(samples);
                    } else {
                        error!("Logic error: Missing packet for chip {} in buffer.", i);
                        continue; // Skip this frame
                    }
                }

                let merged_packet = Packet::RawI32(PacketData {
                    header: final_header,
                    samples: merged_samples,
                });

                if tx.send(BridgeMsg::Data(merged_packet)).is_err() {
                    break; // Upstream channel closed
                }
            }
        }

        // --- Shutdown threads ---
        thread_stop_flag.store(true, Ordering::Relaxed);
        for handle in handles {
            if let Err(e) = handle.join().unwrap() {
                error!("Acquisition thread failed: {:?}", e);
            }
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
        info!("Shutting down ElataV2 board...");
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

