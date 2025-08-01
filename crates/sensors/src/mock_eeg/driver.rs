use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use log::{info, warn, debug};
use crate::types::AdcDriver;
use lazy_static::lazy_static;

use crate::types::{AdcConfig, DriverStatus, DriverError};
use super::mock_data_generator::{gen_realistic_eeg_data};
use eeg_types::SensorError;

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

        let chip_config = config.chips.get(0);
        let default_channels = vec![];
        let channels = chip_config.map(|c| &c.channels).unwrap_or(&default_channels);

        if channels.is_empty() {
            *hardware_in_use = false;
            return Err(DriverError::ConfigurationError(
                "At least one channel must be configured".to_string(),
            ));
        }

        let mut unique_channels = std::collections::HashSet::new();
        for &channel in channels {
            if !unique_channels.insert(channel) {
                *hardware_in_use = false;
                return Err(DriverError::ConfigurationError(format!(
                    "Duplicate channel detected: {}",
                    channel
                )));
            }
        }

        for &channel in channels {
            if channel > 31 {
                *hardware_in_use = false;
                return Err(DriverError::ConfigurationError(format!(
                    "Invalid channel index: {}. MockDriver supports channels 0-31",
                    channel
                )));
            }
        }


        let populated_config = config.clone();

        let inner = MockInner {
            config: populated_config,
            running: false,
            status: DriverStatus::Ok,
            base_timestamp: None,
            sample_count: 0,
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

    fn acquire_batched(
        &mut self,
        batch_size: usize,
        stop_flag: &AtomicBool,
    ) -> Result<(Vec<i32>, u64), SensorError> {

        let inner_arc = self.inner.clone();
        let mut base_timestamp = 0;

        // Lock the inner state once to get the necessary info
        let config = {
            let mut inner_guard = inner_arc.lock().unwrap();
            inner_guard.running = true;
            inner_guard.status = DriverStatus::Running;
            base_timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
            inner_guard.base_timestamp = Some(base_timestamp);
            inner_guard.sample_count = 0;
            inner_guard.config.clone()
        };

        let sample_interval_ns = (1_000_000_000.0 / config.sample_rate as f64) as u64;

        let total_channels: usize = config.chips.iter().map(|chip| chip.channels.len()).sum();
        let mut batch_buffer = Vec::with_capacity(batch_size * total_channels);

        for _ in 0..batch_size {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            let sample_num = {
                let mut inner_guard = inner_arc.lock().unwrap();
                let count = inner_guard.sample_count;
                inner_guard.sample_count += 1;
                count
            };

            let relative_timestamp_us = (sample_num * sample_interval_ns) / 1000;
            let sample_slice = gen_realistic_eeg_data(&config, relative_timestamp_us);
            batch_buffer.extend_from_slice(&sample_slice);
        }

        let sleep_time_ms = (batch_size as u64 * 1000) / config.sample_rate as u64;
        thread::sleep(Duration::from_millis(sleep_time_ms));

        {
            let mut inner_guard = inner_arc.lock().unwrap();
            inner_guard.running = false;
            inner_guard.status = DriverStatus::Stopped;
        }

        Ok((batch_buffer, base_timestamp))
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
