//! # EEG Source Stage
//!
//! This stage is responsible for interfacing with EEG hardware drivers,
//! acquiring raw data in batches, and forwarding it into the pipeline.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::mpsc;
use tokio::task::{self, JoinHandle};

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

#[derive(Debug, Clone, Copy, PartialEq)]
enum SourceRunState {
    Running,
    Quiescing,
    Configuring,
}

struct SourceStateShared {
    run_state: SourceRunState,
    last_config: AdcConfig,
    sensor_meta: Arc<SensorMeta>,
    num_channels: usize,
}

enum InternalCommand {
    Reconfigure(AdcConfig),
}

/// The `EegSource` stage.
pub struct EegSource {
    id: String,
    stop_flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    cmd_tx: mpsc::Sender<InternalCommand>,
}

impl EegSource {
    pub fn new(
        id: String,
        driver: Arc<tokio::sync::Mutex<Box<dyn AdcDriver + Send>>>,
        batch_size: usize,
        outputs: Vec<String>,
        allocator: crate::allocator::SharedPacketAllocator,
        event_tx: Sender<PipelineEvent>,
    ) -> Result<(Self, Receiver<Arc<RtPacket>>), StageError> {
        let (packet_tx, packet_rx) = flume::bounded(1024);
        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();
        let driver_clone = driver.clone();
        let output_name = format!("{}.{}", id, outputs.get(0).cloned().unwrap_or_else(|| "0".to_string()));
        let meta_rev_counter = Arc::new(AtomicUsize::new(1));

        let thread_id = id.clone();
        let handle = task::spawn(async move {
            let mut frame_id_counter = 0;

            let mut driver_guard = driver_clone.lock().await;
            if let Err(e) = driver_guard.initialize() {
                log::error!("Failed to initialize driver: {}", e);
                return;
            }
            let initial_config = driver_guard.get_config().unwrap();
            let initial_num_channels: usize = initial_config.chips.iter().map(|chip| chip.channels.len()).sum();
            if initial_num_channels == 0 {
                log::error!("No channels configured for driver");
                return;
            }
            log::info!("Driver configured with {} total channels", initial_num_channels);

            let initial_sensor_meta = Arc::new(create_sensor_meta_from_config(&initial_config, &meta_rev_counter));
            if event_tx.send(PipelineEvent::SourceReady { meta: (*initial_sensor_meta).clone() }).is_err() {
                log::error!("Failed to send initial source ready event, stopping.");
                return;
            }

            let shared_state = Arc::new(tokio::sync::Mutex::new(SourceStateShared {
                run_state: SourceRunState::Running,
                last_config: initial_config,
                sensor_meta: initial_sensor_meta,
                num_channels: initial_num_channels,
            }));

            drop(driver_guard); // Release the lock before entering the main loop

            while !stop_clone.load(Ordering::Relaxed) {
                let mut state_guard = shared_state.lock().await;

                match state_guard.run_state {
                    SourceRunState::Running => {
                        tokio::select! {
                            biased;

                            cmd = cmd_rx.recv() => {
                                if let Some(InternalCommand::Reconfigure(new_config)) = cmd {
                                    log::info!("State -> Quiescing. Pausing data acquisition for reconfiguration...");
                                    state_guard.run_state = SourceRunState::Quiescing;

                                    // The acquisition loop will naturally stop. Now, we reconfigure.
                                    log::info!("State -> Configuring. Reconfiguring driver...");
                                    let mut driver = driver_clone.lock().await;
                                    if let Err(e) = driver.reconfigure(&new_config) {
                                        log::error!("Failed to reconfigure driver: {}", e);
                                    } else {
                                        meta_rev_counter.fetch_add(1, Ordering::Relaxed);
                                        let new_meta = Arc::new(create_sensor_meta_from_config(&new_config, &meta_rev_counter));
                                        log::info!("Sending SourceReady event with new metadata (rev: {})", new_meta.meta_rev);
                                        let _ = event_tx.send(PipelineEvent::SourceReady { meta: (*new_meta).clone() });

                                        state_guard.last_config = new_config;
                                        state_guard.sensor_meta = new_meta;
                                        state_guard.num_channels = state_guard.last_config.chips.iter().map(|c| c.channels.len()).sum();
                                    }
                                    state_guard.run_state = SourceRunState::Running;
                                    log::info!("State -> Running. Reconfiguration complete.");
                                } else {
                                    stop_clone.store(true, Ordering::Relaxed);
                                }
                            },

                            _ = async {} => {
                                let num_channels = state_guard.num_channels;
                                let sensor_meta = state_guard.sensor_meta.clone();
                                drop(state_guard); // Release lock for acquisition

                                let (samples, timestamp) = {
                                    let mut driver = driver_clone.lock().await;
                                    match driver.acquire_batched(batch_size, &stop_clone) {
                                        Ok((s, t, _)) => (s, t),
                                        Err(e) => {
                                            log::error!("Driver error in acquire_batched: {}", e);
                                            stop_clone.store(true, Ordering::Relaxed);
                                            (Vec::new(), 0)
                                        }
                                    }
                                };

                                if stop_clone.load(Ordering::Relaxed) { continue; }
                                if samples.is_empty() {
                                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                                    continue;
                                }

                                let num_samples_in_batch = if num_channels > 0 { samples.len() / num_channels } else { 0 };
                                let packet = Arc::new(RtPacket::RawI32(PacketData {
                                    header: PacketHeader {
                                        source_id: output_name.clone(),
                                        packet_type: "RawI32".to_string(),
                                        frame_id: { let prev = frame_id_counter; frame_id_counter += 1; prev },
                                        ts_ns: timestamp,
                                        batch_size: num_samples_in_batch as u32,
                                        num_channels: num_channels as u32,
                                        meta: sensor_meta,
                                    },
                                    samples,
                                }));

                                if packet_tx.try_send(packet).is_err() {
                                    log::debug!("Downstream channel full or disconnected; dropping packet.");
                                }
                            }
                        }
                    },
                    SourceRunState::Quiescing | SourceRunState::Configuring => {
                        // In these states, we only listen for the stop signal.
                        // The reconfiguration is handled synchronously within the Running state's select block.
                        tokio::select! {
                            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {},
                            cmd = cmd_rx.recv() => {
                                if cmd.is_none() {
                                    stop_clone.store(true, Ordering::Relaxed);
                                }
                            }
                        }
                    }
                }
            }
            log::info!("EegSource acquisition task finished.");
        });

        Ok((
            Self {
                id,
                stop_flag,
                handle: Some(handle),
                cmd_tx,
            },
            packet_rx,
        ))
    }
}

fn create_sensor_meta_from_config(config: &AdcConfig, meta_rev: &Arc<AtomicUsize>) -> SensorMeta {
    let channel_names: Vec<String> = config
        .chips
        .iter()
        .flat_map(|chip| chip.channels.iter().map(|&c| format!("CH{}", c)))
        .collect();

    SensorMeta {
        sensor_id: 1, // Set appropriate sensor ID
        meta_rev: meta_rev.load(Ordering::Relaxed) as u32,
        schema_ver: 1,
        source_type: "eeg_source".to_string(),
        v_ref: config.vref,
        adc_bits: 24,
        gain: config.gain,
        sample_rate: config.sample_rate,
        offset_code: 0,
        is_twos_complement: true,
        channel_names,
        #[cfg(feature = "meta-tags")]
        tags: std::collections::HashMap::new(),
    }
}

impl Stage for EegSource {
    fn id(&self) -> &str {
        &self.id
    }

    fn control(&mut self, cmd: &ControlCommand, _ctx: &mut StageContext) -> Result<(), StageError> {
        if let ControlCommand::SetParameter { target_stage, parameters } = cmd {
            if target_stage == self.id() {
                log::info!("EegSource received SetParameter with parameters: {:?}", parameters);

                if let Some(driver_params) = parameters.get("driver") {
                    match serde_json::from_value(driver_params.clone()) {
                        Ok(new_config) => {
                            if self.cmd_tx.try_send(InternalCommand::Reconfigure(new_config)).is_err() {
                                log::warn!("Failed to send reconfigure command to EegSource task. Channel may be full.");
                                return Err(StageError::WouldBlock("Command channel full".to_string()));
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to deserialize driver parameters for reconfiguration: {}", e);
                            return Err(StageError::BadConfig("Invalid driver parameters".to_string()));
                        }
                    }
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
        // The executor is responsible for joining the thread handle.
        // We just need to signal it to stop.
    }
}
