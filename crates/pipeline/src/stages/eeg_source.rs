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
use crate::control::{ControlCommand, PipelineEvent};
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
    driver: Arc<std::sync::Mutex<Box<dyn AdcDriver>>>,
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
                let mut last_config = {
                    let mut driver = driver_clone.lock().unwrap();
                    driver.initialize().unwrap();
                    driver.get_config().unwrap()
                };

                let mut sensor_meta = Arc::new({
                    let channel_names: Vec<String> = last_config
                        .chips
                        .iter()
                        .flat_map(|chip| chip.channels.iter().map(|&c| format!("CH{}", c)))
                        .collect();

                    SensorMeta {
                        sensor_id: 1, // Set appropriate sensor ID
                        meta_rev: 1,
                        schema_ver: 1,
                        source_type: "eeg_source".to_string(),
                        v_ref: last_config.vref,
                        adc_bits: 24,
                        gain: last_config.gain,
                        sample_rate: last_config.sample_rate,
                        offset_code: 0,
                        is_twos_complement: true,
                        channel_names,
                        #[cfg(feature = "meta-tags")]
                        tags: HashMap::new(),
                    }
                });

                // EMIT THE SOURCE READY EVENT
                if event_tx
                    .send(PipelineEvent::SourceReady {
                        meta: (*sensor_meta).clone(),
                    })
                    .is_err()
                {
                    log::error!("Failed to send source ready event");
                }

                let mut num_channels: usize =
                    last_config.chips.iter().map(|chip| chip.channels.len()).sum();

                if num_channels == 0 {
                    log::error!("No channels configured for driver");
                    return;
                }

                log::info!("Driver configured with {} total channels", num_channels);

                loop {
                    if stop_clone.load(Ordering::Relaxed) {
                        break;
                    }

                    let (samples, timestamp, current_config) = {
                        let mut driver = driver_clone.lock().unwrap();
                        match driver.acquire_batched(batch_size, &stop_clone) {
                            Ok(data) => data,
                            Err(e) => {
                                log::error!("Driver error in acquire_batched: {}", e);
                                break;
                            }
                        }
                    };

                    if current_config != last_config {
                        log::info!("Driver configuration changed. Updating sensor metadata.");
                        sensor_meta = Arc::new({
                            let channel_names: Vec<String> = current_config
                                .chips
                                .iter()
                                .flat_map(|chip| chip.channels.iter().map(|&c| format!("CH{}", c)))
                                .collect();

                            SensorMeta {
                                sensor_id: 1, // Set appropriate sensor ID
                                meta_rev: 1,
                                schema_ver: 1,
                                source_type: "eeg_source".to_string(),
                                v_ref: current_config.vref,
                                adc_bits: 24,
                                gain: current_config.gain,
                                sample_rate: current_config.sample_rate,
                                offset_code: 0,
                                is_twos_complement: true,
                                channel_names,
                                #[cfg(feature = "meta-tags")]
                                tags: HashMap::new(),
                            }
                        });
                        num_channels =
                            current_config.chips.iter().map(|chip| chip.channels.len()).sum();

                        if event_tx
                            .send(PipelineEvent::SourceReady {
                                meta: (*sensor_meta).clone(),
                            })
                            .is_err()
                        {
                            log::error!("Failed to send source ready event after reconfig");
                        }
                        last_config = current_config;
                    }

                    if samples.is_empty() {
                        continue;
                    }
                    log::info!("eeg_source acquired {} samples", samples.len());

                    let mut packet_samples =
                        crate::allocator::RecycledI32Vec::new(allocator.clone());
                    packet_samples.extend_from_slice(&samples);

                    let num_samples_in_batch = samples.len() / num_channels;

                    let packet = Arc::new(RtPacket::RawI32(PacketData {
                        header: PacketHeader {
                            source_id: output_name.clone(),
                            ts_ns: timestamp,
                            batch_size: num_samples_in_batch as u32,
                            num_channels: num_channels as u32,
                            meta: sensor_meta.clone(),
                        },
                        samples: packet_samples,
                    }));

                    if packet_tx.send(packet).is_err() {
                        dropped_packets_clone.fetch_add(1, Ordering::Relaxed);
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
                driver,
            },
            packet_rx,
        ))
    }
}

impl Stage for EegSource {
    fn id(&self) -> &str {
        &self.id
    }

    fn control(&mut self, cmd: &ControlCommand, ctx: &mut StageContext) -> Result<(), StageError> {
        if let ControlCommand::SetParameter {
            target_stage,
            parameters,
        } = cmd
        {
            if target_stage == self.id() {
                log::info!(
                    "EegSource received SetParameter with parameters: {:?}",
                    parameters
                );
                let mut driver = self.driver.lock().unwrap();
                let mut config = driver.get_config()?;

                log::info!("Attempting to parse SetParameter command...");
                if let Some(driver_params) = parameters.get("driver") {
                    log::info!("Found 'driver' parameters: {:?}", driver_params);
                    let mut needs_reconfigure = false;

                    if let Some(chips) = driver_params.get("chips") {
                        log::info!("Found 'chips' parameters.");
                        if let Some(chips_array) = chips.as_array() {
                            if let Some(first_chip) = chips_array.get(0) {
                                if let Some(channels_val) = first_chip.get("channels") {
                                    if let Some(channels_array) = channels_val.as_array() {
                                        let channels: Vec<u8> = channels_array
                                            .iter()
                                            .filter_map(|v| v.as_u64().map(|n| n as u8))
                                            .collect();
                                        if !config.chips.is_empty() {
                                            config.chips[0].channels = channels;
                                            needs_reconfigure = true;
                                            log::info!(
                                                "Staged channels for reconfig: {:?}",
                                                config.chips[0].channels
                                            );
                                        }
                                    } else {
                                        log::warn!("'channels' parameter is not an array.");
                                    }
                                }
                            }
                        }
                    }

                    if let Some(sample_rate_val) = driver_params.get("sample_rate") {
                        if let Some(sample_rate) = sample_rate_val.as_u64() {
                            config.sample_rate = sample_rate as u32;
                            needs_reconfigure = true;
                            log::info!("Staged sample_rate for reconfig: {}", config.sample_rate);
                        } else {
                            log::warn!("'sample_rate' parameter is not a valid number.");
                        }
                    }

                    if needs_reconfigure {
                        log::info!("Reconfiguring driver with new settings: {:?}", config);
                        if let Err(e) = driver.reconfigure(&config) {
                            log::error!("Failed to reconfigure driver: {}", e);
                            let _ = ctx.event_tx.send(PipelineEvent::ErrorOccurred {
                                stage_id: self.id.clone(),
                                error_message: format!("Reconfiguration failed: {}", e),
                            });
                        }
                    } else {
                        log::warn!("No valid parameters found to update in 'driver' config.");
                    }
                } else {
                    log::warn!("'driver' parameter not found.");
                }
            }
        }
        Ok(())
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
        if let Ok(mut driver) = self.driver.lock() {
            let _ = driver.shutdown();
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
