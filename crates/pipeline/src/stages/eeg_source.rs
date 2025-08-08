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
        let (packet_tx, packet_rx) = flume::bounded(1024); // Increased buffer size
        let (cmd_tx, mut cmd_rx) = mpsc::channel(16);

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();
        let driver_clone = driver.clone();
        let output_name = format!(
            "{}.{}",
            id,
            outputs.get(0).cloned().unwrap_or_else(|| "0".to_string())
        );
        let dropped_packets = Arc::new(AtomicUsize::new(0));
        let meta_rev_counter = Arc::new(AtomicUsize::new(1));
        let frame_counter = Arc::new(AtomicUsize::new(0));

        let thread_id = id.clone();
        let handle = task::spawn(async move {
            let mut frame_id_counter = 0;
            macro_rules! send_or_stop {
                ($expr:expr, $desc:expr) => {
                    if $expr.is_err() {
                        log::warn!("{}: receiver gone, stopping EegSource", $desc);
                        stop_clone.store(true, Ordering::Relaxed);
                    }
                };
            }

            let mut driver = driver_clone.lock().await;
            if let Err(e) = driver.initialize() {
                log::error!("Failed to initialize driver: {}", e);
                return;
            }
            let mut last_config = driver.get_config().unwrap();
            let mut num_channels: usize =
                last_config.chips.iter().map(|chip| chip.channels.len()).sum();

            if num_channels == 0 {
                log::error!("No channels configured for driver");
                return;
            }
            log::info!("Driver configured with {} total channels", num_channels);

            let mut sensor_meta = Arc::new(create_sensor_meta_from_config(&last_config, &meta_rev_counter));
            if event_tx.send(PipelineEvent::SourceReady { meta: (*sensor_meta).clone() }).is_err() {
                log::error!("Failed to send initial source ready event, stopping.");
                stop_clone.store(true, Ordering::Relaxed);
            }

            // Drop the initial lock before entering the loop
            drop(driver);

            while !stop_clone.load(Ordering::Relaxed) {
                tokio::select! {
                    biased; // Prioritize command processing over data acquisition

                    cmd = cmd_rx.recv() => {
                        if let Some(cmd) = cmd {
                            match cmd {
                                InternalCommand::Reconfigure(new_config) => {
                                    log::info!("Reconfiguring driver with new settings: {:?}", new_config);
                                    let mut driver = driver_clone.lock().await;
                                    if let Err(e) = driver.reconfigure(&new_config) {
                                        log::error!("Failed to reconfigure driver: {}", e);
                                        send_or_stop!(
                                            event_tx.send(PipelineEvent::ErrorOccurred {
                                                stage_id: thread_id.clone(),
                                                error_message: format!("Reconfiguration failed: {}", e),
                                            }),
                                            "reconfig error"
                                        );
                                    } else {
                                        last_config = new_config;
                                        meta_rev_counter.fetch_add(1, Ordering::Relaxed);
                                        sensor_meta = Arc::new(create_sensor_meta_from_config(&last_config, &meta_rev_counter));
                                        num_channels = last_config.chips.iter().map(|chip| chip.channels.len()).sum();
                                        send_or_stop!(
                                            event_tx.send(PipelineEvent::SourceReady { meta: (*sensor_meta).clone() }),
                                            "reconfig SourceReady"
                                        );
                                    }
                                }
                            }
                        } else {
                            // Command channel closed, exit the task
                            stop_clone.store(true, Ordering::Relaxed);
                        }
                    },

                    // Default branch for data acquisition
                    _ = async {
                        let (samples, timestamp) = {
                            let mut driver = driver_clone.lock().await;
                            match driver.acquire_batched(batch_size, &stop_clone) {
                                Ok((s, t, _)) => (s, t),
                                Err(e) => {
                                    log::error!("Driver error in acquire_batched: {}", e);
                                    stop_clone.store(true, Ordering::Relaxed); // Stop on error
                                    (Vec::new(), 0) // Return dummy data, loop will terminate
                                }
                            }
                        };

                        if stop_clone.load(Ordering::Relaxed) {
                            return; // Exit async block if we were stopped during acquire
                        }

                        if samples.is_empty() {
                            // Yield to the scheduler if no data was acquired
                            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                            return;
                        }
                        log::debug!("eeg_source acquired {} samples", samples.len());

                        let mut packet_samples = Vec::with_capacity(samples.len());
                        packet_samples.extend_from_slice(&samples);

                        let num_samples_in_batch = samples.len() / num_channels;

                        let packet = Arc::new(RtPacket::RawI32(PacketData {
                            header: PacketHeader {
                                source_id: output_name.clone(),
                                frame_id: {
                                    let prev = frame_id_counter;
                                    frame_id_counter += 1;
                                    prev
                                },
                                ts_ns: timestamp,
                                batch_size: num_samples_in_batch as u32,
                                num_channels: num_channels as u32,
                                meta: sensor_meta.clone(),
                            },
                            samples: packet_samples,
                        }));

                        if let Err(e) = packet_tx.try_send(packet) {
                            // This error is expected if the channel is full (backpressure) or
                            // disconnected (no subscribers). In either case, we just drop the
                            // packet and log it. The stage should not stop.
                            match e {
                                flume::TrySendError::Full(_) => {
                                    log::debug!("Downstream channel full; dropping packet.");
                                }
                                flume::TrySendError::Disconnected(_) => {
                                    log::debug!("Downstream channel disconnected; dropping packet.");
                                }
                            }
                        }
                    } => {}
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
