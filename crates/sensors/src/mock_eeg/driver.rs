use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use crossbeam_channel::Sender;
use std::thread;
use std::time::Duration;
use log::{info, warn, debug};
use crate::types::AdcDriver;
use lazy_static::lazy_static;

use crate::types::{AdcConfig, DriverStatus, DriverError, DriverType};
use super::mock_data_generator::{gen_realistic_eeg_data, current_timestamp_micros};
use eeg_types::{BridgeMsg, SensorError};
use pipeline::data::{Packet, PacketData, PacketHeader, SensorMeta};

// Static hardware lock to simulate real hardware access constraints
lazy_static! {
    static ref HARDWARE_LOCK: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
}

/// A stubbed-out driver that does not access any hardware.
pub struct MockDriver {
    inner: Arc<Mutex<MockInner>>,
}

/// Internal state for the MockDriver.
struct MockInner {
    config: AdcConfig,
    running: bool,
    status: DriverStatus,
    // Base timestamp for calculating sample timestamps (microseconds since epoch)
    base_timestamp: Option<u64>,
    // Total samples generated since acquisition started
    sample_count: u64,
    meta: Arc<SensorMeta>,
}

impl MockDriver {
    pub fn new(config: AdcConfig) -> Result<Self, DriverError> {
        // Try to acquire the hardware lock to simulate real hardware access constraints
        let mut hardware_in_use = HARDWARE_LOCK.lock()
            .map_err(|_| DriverError::Other("Failed to acquire hardware lock".to_string()))?;

        if *hardware_in_use {
            return Err(DriverError::HardwareNotFound(
                "Hardware already in use by another driver instance".to_string()
            ));
        }

        // Mark hardware as in use
        *hardware_in_use = true;

        // Validate config
        if config.board_driver != DriverType::MockEeg {
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                "MockDriver requires config.board_driver=DriverType::MockEeg".to_string()
            ));
        }

        if config.channels.is_empty() {
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                "At least one channel must be configured".to_string()
            ));
        }

        let mut unique_channels = std::collections::HashSet::new();
        for &channel in &config.channels {
            if !unique_channels.insert(channel) {
                *hardware_in_use = false;
                return Err(DriverError::ConfigurationError(
                    format!("Duplicate channel detected: {}", channel)
                ));
            }
        }

        for &channel in &config.channels {
            if channel > 31 {
                *hardware_in_use = false;
                return Err(DriverError::ConfigurationError(
                    format!("Invalid channel index: {}. MockDriver supports channels 0-31", channel)
                ));
            }
        }

        if config.batch_size == 0 {
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                "Batch size must be greater than 0".to_string()
            ));
        }

        if config.batch_size < config.channels.len() {
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                format!("Batch size ({}) must be at least equal to the number of channels ({})",
                        config.batch_size, config.channels.len())
            ));
        }

        let channel_names = config
            .channels
            .iter()
            .map(|&ch| format!("ch{}", ch))
            .collect();

        let meta = Arc::new(SensorMeta {
            schema_ver: 2,
            source_type: "MockEeg".to_string(),
            v_ref: config.vref,
            adc_bits: 24,
            gain: config.gain,
            sample_rate: config.sample_rate,
            offset_code: 0,
            is_twos_complement: true,
            channel_names,
            #[cfg(feature = "meta-tags")]
            tags: Default::default(),
        });

        let inner = MockInner {
            config: config.clone(),
            running: false,
            status: DriverStatus::Ok,
            base_timestamp: None,
            sample_count: 0,
            meta,
        };

        let driver = MockDriver {
            inner: Arc::new(Mutex::new(inner)),
        };

        info!("MockDriver created with config: {:?}", config);

        Ok(driver)
    }
}

// Implement the AdcDriver trait
impl crate::types::AdcDriver for MockDriver {
    fn initialize(&mut self) -> Result<(), DriverError> {
        // No hardware to initialize for the mock driver
        Ok(())
    }

    fn acquire(&mut self, tx: crossbeam_channel::Sender<BridgeMsg>, stop_flag: &AtomicBool) -> Result<(), SensorError> {
        info!("MockDriver synchronous acquisition started");

        let inner_arc = self.inner.clone();

        // Lock the inner state once to get the necessary info
        let (config, meta) = {
            let mut inner_guard = inner_arc.lock().unwrap();
            inner_guard.running = true;
            inner_guard.status = DriverStatus::Running;
            inner_guard.base_timestamp = Some(current_timestamp_micros().unwrap_or(0));
            inner_guard.sample_count = 0;
            (inner_guard.config.clone(), inner_guard.meta.clone())
        };

        let batch_size = config.batch_size;
        let sample_interval_us = (1_000_000 / config.sample_rate) as u64;

        while !stop_flag.load(Ordering::Relaxed) {
            let (current_sample_count, base_timestamp) = {
                let mut inner_guard = inner_arc.lock().unwrap();
                let count = inner_guard.sample_count;
                inner_guard.sample_count += batch_size as u64;
                (count, inner_guard.base_timestamp.unwrap())
            };

            let relative_timestamp = current_sample_count * sample_interval_us;
            let samples = gen_realistic_eeg_data(&config, relative_timestamp);

            let packet = Packet::RawI32(PacketData {
                header: PacketHeader {
                    ts_ns: (base_timestamp + relative_timestamp) * 1000,
                    batch_size: batch_size as u32,
                    meta: meta.clone(),
                },
                samples,
            });

            if tx.send(BridgeMsg::Data(packet)).is_err() {
                warn!("MockDriver bridge channel closed");
                break;
            }

            let sleep_time_ms = (batch_size as u64 * 1000) / config.sample_rate as u64;
            thread::sleep(Duration::from_millis(sleep_time_ms));
        }

        {
            let mut inner_guard = inner_arc.lock().unwrap();
            inner_guard.running = false;
            inner_guard.status = DriverStatus::Stopped;
        }

        info!("MockDriver synchronous acquisition stopped");
        Ok(())
    }

    fn get_status(&self) -> DriverStatus {
        self.inner.lock().unwrap().status.clone()
    }

    fn get_config(&self) -> Result<AdcConfig, DriverError> {
        Ok(self.inner.lock().unwrap().config.clone())
    }

    fn shutdown(&mut self) -> Result<(), DriverError> {
        debug!("Shutting down MockDriver");

        let mut inner = self.inner.lock().unwrap();
        inner.running = false;
        inner.status = DriverStatus::NotInitialized;
        inner.base_timestamp = None;
        inner.sample_count = 0;

        info!("MockDriver shutdown complete");
        Ok(())
    }
}

impl Drop for MockDriver {
    fn drop(&mut self) {
        // Release the hardware lock when the driver is dropped
        let mut hardware_in_use = HARDWARE_LOCK.lock().unwrap();
        *hardware_in_use = false;
        
        if self.get_status() != DriverStatus::NotInitialized {
             warn!("MockDriver dropped without calling shutdown() first.");
             let _ = self.shutdown();
        }
    }
}
