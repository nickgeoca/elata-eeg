//! # EEG Source Stage
//!
//! This stage is responsible for interfacing with EEG hardware drivers,
//! acquiring raw data in batches, and forwarding it into the pipeline.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};

use serde::Deserialize;

use boards::elata_v1::driver::ElataV1Driver;
use boards::elata_v2::driver::ElataV2Driver;
use sensors::{
    mock_eeg::driver::MockDriver,
    types::{AdcConfig, AdcDriver, DriverError},
};

use crate::config::StageConfig;
use crate::data::{PacketData, PacketHeader, RtPacket};
use crate::error::StageError;
use crate::registry::StageFactory;
use eeg_types::data::SensorMeta;
use crate::stage::{Stage, StageContext, StageInitCtx};
use flume::Receiver;

/// Factory for creating `EegSource` stages.
#[derive(Default)]
pub struct EegSourceFactory;

impl StageFactory for EegSourceFactory {
    fn create(
        &self,
        config: &StageConfig,
        init_ctx: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        let params: EegSourceParams = serde_json::from_value(serde_json::to_value(&config.params)?)
            .map_err(|e| StageError::BadConfig(format!("Failed to parse EegSource params: {}", e)))?;

        let driver = AdcDriverBuilder::new()
            .with_driver_type(params.driver.driver_type)
            .with_adc_config(params.driver.adc_config)
            .build()?;

        let (stage, rx) = EegSource::new(
            config.name.clone(),
            driver,
            params.batch_size,
            params.outputs,
            init_ctx.allocator.clone(),
        )?;
        Ok((Box::new(stage), Some(rx)))
    }
}

#[derive(Debug, Deserialize)]
struct DriverParams {
    #[serde(rename = "type")]
    driver_type: DriverType,
    #[serde(flatten)]
    adc_config: AdcConfig,
}

#[derive(Debug, Deserialize)]
struct EegSourceParams {
    driver: DriverParams,
    batch_size: usize,
    #[serde(default)]
    outputs: Vec<String>,
}

/// The `EegSource` stage.
pub struct EegSource {
    id: String,
    stop_flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    dropped_packets: Arc<AtomicUsize>,
}

impl EegSource {
    pub fn new(
        id: String,
        driver: Box<dyn AdcDriver>,
        batch_size: usize,
        outputs: Vec<String>,
        allocator: crate::allocator::SharedPacketAllocator,
    ) -> Result<(Self, Receiver<Arc<RtPacket>>), StageError> {
        let (packet_tx, packet_rx) = flume::bounded(128);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();
        let driver = Arc::new(std::sync::Mutex::new(driver));
        let driver_clone = driver.clone();
        let output_name = format!(
            "{}.{}",
            id,
            outputs.get(0).cloned().unwrap_or_else(|| "0".to_string())
        );
        let dropped_packets = Arc::new(AtomicUsize::new(0));
        let dropped_packets_clone = dropped_packets.clone();

        let handle = thread::Builder::new()
            .name(format!("{}_acq", id))
            .spawn(move || {
                let mut driver = driver_clone.lock().unwrap();
                driver.initialize().unwrap(); // Handle error properly

                let config = driver.get_config().unwrap();

                let sensor_meta = SensorMeta {
                    sensor_id: 1, // Set appropriate sensor ID
                    meta_rev: 1,
                    schema_ver: 1,
                    source_type: "eeg_source".to_string(),
                    v_ref: config.vref,
                    adc_bits: 24,
                    gain: config.gain, // Propagate gain from hardware config
                    sample_rate: config.sample_rate,
                    offset_code: 0,
                    is_twos_complement: true,
                    channel_names: vec![], // Populate if available
                    #[cfg(feature = "meta-tags")]
                    tags: HashMap::new(),
                };
                let sensor_meta = Arc::new(sensor_meta);

                // Calculate total channels from all chips
                let num_channels: usize = config.chips.iter().map(|chip| chip.channels.len()).sum();

                if num_channels == 0 {
                    log::error!("No channels configured for driver");
                    return;
                }

                log::info!("Driver configured with {} total channels", num_channels);
                let sample_interval_ns = (1_000_000_000.0 / config.sample_rate as f64) as u64;

                loop {
                    if stop_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    match driver.acquire_batched(batch_size, &stop_clone) {
                        Ok((samples, timestamp)) => {
                            if samples.is_empty() {
                                continue;
                            }

                            for (i, sample_chunk) in samples.chunks(num_channels).enumerate() {
                                let mut packet_samples =
                                    crate::allocator::RecycledI32Vec::new(allocator.clone());
                                packet_samples.extend_from_slice(sample_chunk);

                                let sample_timestamp = timestamp + (i as u64 * sample_interval_ns);

                                let packet = Arc::new(RtPacket::RawI32(PacketData {
                                    header: PacketHeader {
                                        source_id: output_name.clone(),
                                        ts_ns: sample_timestamp,
                                        batch_size: 1, // Each packet is now a single sample
                                        meta: sensor_meta.clone(),
                                    },
                                    samples: packet_samples,
                                }));

                                if packet_tx.send(packet).is_err() {
                                    dropped_packets_clone.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Driver error in acquire_batched: {}", e);
                            break;
                        }
                    }
                }
            })
            .unwrap();

        Ok((
            Self {
                id,
                stop_flag,
                handle: Some(handle),
                dropped_packets,
            },
            packet_rx,
        ))
    }
}

impl Stage for EegSource {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        _packet: Arc<RtPacket>, // Ignored, this is a source stage
        _ctx: &mut StageContext,
    ) -> Result<Option<Arc<RtPacket>>, StageError> {
        unreachable!("process should not be called on a producer stage");
    }
}

impl Drop for EegSource {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub enum DriverType {
    ElataV1,
    ElataV2,
    Mock,
}

#[derive(Default)]
pub struct AdcDriverBuilder {
    driver_type: Option<DriverType>,
    adc_config: Option<AdcConfig>,
}

impl AdcDriverBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_driver_type(mut self, driver_type: DriverType) -> Self {
        self.driver_type = Some(driver_type);
        self
    }

    pub fn with_adc_config(mut self, adc_config: AdcConfig) -> Self {
        self.adc_config = Some(adc_config);
        self
    }

    pub fn build(self) -> Result<Box<dyn AdcDriver>, DriverError> {
        let driver_type = self.driver_type.ok_or_else(|| {
            DriverError::ConfigurationError("Driver type must be specified".to_string())
        })?;
        let adc_config = self.adc_config.ok_or_else(|| {
            DriverError::ConfigurationError("ADC config must be specified".to_string())
        })?;

        match driver_type {
            DriverType::ElataV1 => Ok(Box::new(ElataV1Driver::new(adc_config).unwrap())),
            DriverType::ElataV2 => Ok(Box::new(ElataV2Driver::new(adc_config).unwrap())),
            DriverType::Mock => Ok(Box::new(MockDriver::new(adc_config).unwrap())),
        }
    }
}