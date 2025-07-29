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

use sensors::types::{AdcConfig, AdcDriver, DriverError};

use crate::config::StageConfig;
use crate::data::{PacketData, PacketHeader, RtPacket};
use crate::error::StageError;
use crate::registry::StageFactory;
use eeg_types::data::SensorMeta;
use crate::stage::{Stage, StageContext, StageInitCtx};
use crate::control::PipelineEvent;
use flume::{Receiver, Sender};

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

        let driver = init_ctx
            .driver
            .clone()
            .ok_or_else(|| StageError::BadConfig("Driver not available in context".to_string()))?;

        let (stage, rx) = EegSource::new(
            config.name.clone(),
            driver,
            params.batch_size,
            params.outputs,
            init_ctx.allocator.clone(),
            init_ctx.event_tx.clone(),
        )?;
        Ok((Box::new(stage), Some(rx)))
    }
}

#[derive(Debug, Deserialize)]
struct EegSourceParams {
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
        driver: Arc<std::sync::Mutex<Box<dyn AdcDriver>>>,
        batch_size: usize,
        outputs: Vec<String>,
        allocator: crate::allocator::SharedPacketAllocator,
        event_tx: Sender<PipelineEvent>,
    ) -> Result<(Self, Receiver<Arc<RtPacket>>), StageError> {
        let (packet_tx, packet_rx) = flume::bounded(128);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();
        let driver_clone = driver.clone();
        let output_name = format!(
            "{}.{}",
            id,
            outputs.get(0).cloned().unwrap_or_else(|| "0".to_string())
        );
        let dropped_packets = Arc::new(AtomicUsize::new(0));
        let dropped_packets_clone = dropped_packets.clone();

        let thread_id = id.clone();
        let handle = thread::Builder::new()
            .name(format!("{}_acq", id))
            .spawn(move || {
                let mut driver = driver_clone.lock().unwrap();

                let config = driver.get_config().unwrap();

                // Generate channel names based on their numbers, as the config only provides numbers.
                let channel_names: Vec<String> = config
                    .chips
                    .iter()
                    .flat_map(|chip| chip.channels.iter().map(|&c| format!("CH{}", c)))
                    .collect();

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
                    channel_names, // Use the extracted channel names
                    #[cfg(feature = "meta-tags")]
                    tags: HashMap::new(),
                };
                let sensor_meta = Arc::new(sensor_meta);

                // EMIT THE SOURCE READY EVENT
                let event = PipelineEvent::SourceReady {
                    meta: (*sensor_meta).clone(),
                };
                if event_tx.send(event).is_err() {
                    log::error!("Failed to send source ready event");
                }

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
