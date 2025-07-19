use std::collections::HashMap;
use std::error::Error;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use flume::{Selector, Sender};
use log::{debug, error, info, warn};
use rppal::gpio::{Gpio, InputPin};
use rppal::spi::{Bus, Mode};
use thread_priority::ThreadPriority;

use eeg_types::{BridgeMsg, SensorError};
use pipeline::data::{PacketOwned, PacketData, SensorMeta};
use sensors::{
    ads1299::registers::{
        self, BIAS_SENSN_REG, BIAS_SENSP_REG, CH1SET_ADDR, CHN_OFF, CHN_REG, CMD_RDATAC,
        CMD_RESET, CMD_SDATAC, CMD_STANDBY, CMD_WAKEUP, CONFIG1_REG,
        CONFIG2_REG, CONFIG3_REG, CONFIG4_REG, LOFF_SESP_REG, MISC1_REG, DAISY_DISABLE,
    BIASREF_INT , PD_BIAS, PD_REFBUF, BIAS_SENS_OFF_MASK, DC_TEST, SRB1},
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
    drdy_txs: Vec<Sender<()>>,
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
            1_240_000, // 2.048MHz/16 = 128kHz
            Mode::Mode1,
        )?);
        info!("SPI bus initialized.");

        let gpio = Arc::new(Gpio::new()?);
        info!("GPIO initialized.");

        let mut chip_drivers = Vec::with_capacity(NUM_CHIPS);
        let mut drdy_rxs = Vec::with_capacity(NUM_CHIPS);
        let mut drdy_txs = Vec::with_capacity(NUM_CHIPS);
        for _ in 0..NUM_CHIPS {
            let (tx, rx) = flume::bounded(1);
            drdy_txs.push(tx);
            drdy_rxs.push(rx);
        }

        let mut drdy_rxs_iter = drdy_rxs.into_iter();
        for chip_config in config.chips.iter() {
            let sensor_meta = Arc::new(SensorMeta {
                v_ref: config.vref,
                gain: config.gain,
                sample_rate: config.sample_rate,
                adc_bits: 24, // ADS1299 is a 24-bit ADC
                ..Default::default()
            });
            let cs_pin = gpio.get(chip_config.cs_pin)?.into_output();
            let drdy_rx = drdy_rxs_iter.next().unwrap();
            let driver =
                Ads1299Driver::new(chip_config.clone(), spi_bus.clone(), cs_pin, drdy_rx, sensor_meta)?;
            chip_drivers.push(driver);
        }

        Ok(Self {
            chip_drivers,
            gpio,
            status: Arc::new(Mutex::new(DriverStatus::Stopped)),
            config,
            drdy_txs,
        })
    }
}

impl AdcDriver for ElataV2Driver {
    fn initialize(&mut self) -> Result<(), DriverError> {
        info!("Initializing ElataV2 board with {} chips...", NUM_CHIPS);

        // Reset all chips first to ensure they are in a known state
        for chip in self.chip_drivers.iter_mut() {
            chip.send_command(CMD_RESET)?;
        thread::sleep(Duration::from_millis(140));

            // chip.send_command(CMD_WAKEUP)?;
        thread::sleep(Duration::from_millis(140));


        }
        // Wait for reset to complete
        thread::sleep(Duration::from_millis(140));

                
        for (i, chip) in self.chip_drivers.iter_mut().enumerate() {
                let val = chip.read_register(0x00)?;                info!("----------Reg 0x{:02X}: 0x{:02X}", 0x00, val);
            info!("Initializing Chip {}...", i);
            let chip_info = &self.config.chips[i];

            // --- Register Settings ---
            let gain_mask = registers::gain_to_reg_mask(self.config.gain)?;
            let sps_mask = registers::sps_to_reg_mask(self.config.sample_rate)?;
            let pd_bias = if i == 0 { PD_BIAS } else { 0x00 }; // Board 0 is the bias master
            let ch_settings: Vec<(u8, u8)> = (0..8)
                .map(|ch_idx| {
                    let setting = if chip_info.channels.contains(&ch_idx) {
                        CHN_REG | gain_mask
                    } else {
                        CHN_OFF
                    };
                    (CH1SET_ADDR + ch_idx, setting)
                })
                .collect();
            let active_ch_mask = chip_info.channels.iter().fold(0, |acc, &ch| acc | (1 << (ch % 8)));

            chip.initialize_chip(
                CONFIG1_REG | sps_mask | DAISY_DISABLE,
                CONFIG2_REG | DC_TEST,
                CONFIG3_REG | BIASREF_INT | PD_REFBUF | pd_bias,
                CONFIG4_REG,
                LOFF_SESP_REG,
                MISC1_REG | SRB1,
                &ch_settings,
                active_ch_mask,
                BIAS_SENSN_REG,
                active_ch_mask,
            )?;

            thread::sleep(Duration::from_millis(10));
            // Register dump for verification
            info!("---- Chip {} Register Dump ----", i);
            for reg in 0x00..=0x17 {
                let val = chip.read_register(reg)?;
                info!("Reg 0x{:02X}: 0x{:02X}", reg, val);
            }
            
            chip.send_command(CMD_STANDBY)?;
            info!("Chip {} initialized and ready.", i);

            // Add a small delay to allow the chip to stabilize
            thread::sleep(Duration::from_millis(10));
        }

        info!("ElataV2 board initialized successfully.");
        Ok(())
    }

    // Sequence
    // 1) 
    fn acquire(
        &mut self,
        tx: Sender<BridgeMsg>,
        stop_flag: &AtomicBool,
    ) -> Result<(), SensorError> {
        *self.status.lock().unwrap() = DriverStatus::Running;
        let (data_tx, data_rx) = flume::bounded::<(u8, PacketOwned)>(NUM_CHIPS * 2);
        let mut start_pin = self.gpio.get(START_PIN).unwrap().into_output();
        start_pin.set_low();
        thread::sleep(Duration::from_millis(10));

        // --- Synchronize with START pulse ---
        info!("Synchronizing chips with START pulse...");
        for chip in self.chip_drivers.iter_mut() {
            chip.send_command(CMD_WAKEUP)?;
        }
        thread::sleep(Duration::from_millis(10));
        start_pin.set_high();
        thread::sleep(Duration::from_millis(10));
        // TODO wait for all data readies then send RDATAC
        for chip in self.chip_drivers.iter_mut() {
            chip.send_command(CMD_RDATAC)?;
        }
        // info!("Chips synchronized and in RDATAC mode.");

        // --- Spawn acquisition threads ---
        info!("Starting acquisition threads...");
        let mut handles: Vec<JoinHandle<Result<(), SensorError>>> = Vec::new();
        let thread_stop_flag = Arc::new(AtomicBool::new(false));

        for (i, chip) in self.chip_drivers.iter().enumerate() {
            let mut chip_clone = chip.clone();
            let thread_tx = data_tx.clone();
            let stop_clone = thread_stop_flag.clone();
            let handle = thread::Builder::new()
                .name(format!("chip_{}_acq", i))
                .spawn(move || {
                    if let Err(e) = thread_priority::set_current_thread_priority(ThreadPriority::Max) {
                        warn!("[Chip {}] Failed to set thread priority: {:?}", i, e);
                    }
                    chip_clone.acquire_raw(thread_tx, &stop_clone, i as u8)
                })
                .unwrap();
            handles.push(handle);
        }

        // --- Spawn DRDY interrupt dispatcher thread ---
        let dispatcher_stop_flag = thread_stop_flag.clone();
        let mut drdy_pins: Vec<InputPin> = self
            .config
            .chips
            .iter()
            .map(|c| self.gpio.get(c.drdy_pin).unwrap().into_input())
            .collect();

        for pin in &mut drdy_pins {
            pin.set_interrupt(rppal::gpio::Trigger::FallingEdge, None)
                .map_err(|e| SensorError::DriverError(e.to_string()))?;
        }

        let gpio_clone = self.gpio.clone();
        let drdy_txs_clone = self.drdy_txs.clone();
        let dispatcher_handle = thread::Builder::new()
            .name("drdy_dispatcher".to_string())
            .spawn(move || {
                let poll_timeout = Duration::from_millis(200);
                let drdy_pins_refs: Vec<&InputPin> = drdy_pins.iter().collect();
                while !dispatcher_stop_flag.load(Ordering::Relaxed) {
                    match gpio_clone.poll_interrupts(&drdy_pins_refs, true, Some(poll_timeout)) {
                        Ok(Some((pin, _level))) => {
                            if let Some(chip_index) =
                                drdy_pins.iter().position(|p| p.pin() == pin.pin())
                            {
                                if drdy_txs_clone[chip_index].send(()).is_err() {
                                    debug!("DRDY channel for chip {} closed.", chip_index);
                                    break;
                                }
                            }
                        }
                        Ok(None) => {
                            warn!("DRDY dispatcher timed out waiting for interrupt.");
                        }
                        Err(e) => {
                            error!("Error polling DRDY interrupts: {}", e);
                            break;
                        }
                    }
                }
                info!("DRDY dispatcher thread finished.");
            })
            .unwrap();
        handles.push(thread::spawn(move || {
            dispatcher_handle.join().unwrap();
            Ok(())
        }));

        // --- Data merging loop ---
        let merge_timeout = Duration::from_millis(
            (1.0 / self.config.sample_rate as f32 * 1000.0 * 2.0) as u64,
        );

        let initial_timestamp_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let increment_ns = (1_000_000_000.0 / self.config.sample_rate as f64) as u64;
        let mut packet_index: u64 = 0;

        while !stop_flag.load(Ordering::Relaxed) {
            let mut packet_buffer: HashMap<u8, PacketData<Vec<i32>>> = HashMap::with_capacity(NUM_CHIPS);
            let first_packet_deadline = Instant::now() + merge_timeout;

            while packet_buffer.len() < NUM_CHIPS {
                let timeout = first_packet_deadline.saturating_duration_since(Instant::now());
                if timeout.is_zero() {
                    warn!("Timed out waiting for a full frame of packets.");
                    break;
                }

                Selector::new()
                    .recv(&data_rx, |msg| {
                        if let Ok((chip_id, PacketOwned::RawI32(packet_data))) = msg {
                            packet_buffer.insert(chip_id, packet_data);
                        }
                    })
                    .wait_timeout(timeout)
                    .ok();
            }

            if packet_buffer.len() == NUM_CHIPS {
                let mut merged_samples = Vec::with_capacity(self.config.channels.len());
                let mut first_header = None;

                for i in 0..NUM_CHIPS {
                    if let Some(packet_data) = packet_buffer.remove(&(i as u8)) {
                        if first_header.is_none() {
                            first_header = Some(packet_data.header);
                        }
                        merged_samples.extend(packet_data.samples);
                    } else {
                        warn!("Missing packet from chip {} in frame", i);
                        break;
                    }
                }

                if let Some(mut header) = first_header {
                    header.ts_ns = initial_timestamp_ns + (packet_index * increment_ns);
                    header.batch_size = merged_samples.len() as u32;

                    let merged_packet = PacketOwned::RawI32(PacketData {
                        header,
                        samples: merged_samples,
                    });

                    if tx.send(BridgeMsg::Data(merged_packet)).is_err() {
                        break;
                    }
                    packet_index += 1;
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
