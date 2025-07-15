//! Data acquisition stage for EEG sensors.

use crate::config::StageConfig;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext};
use async_trait::async_trait;
use eeg_types::Packet;
use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::time::{interval, Duration};
use tracing::debug;

/// A source stage that generates mock raw EEG data.
#[derive(Default)]
pub struct AcquireFactory;

#[async_trait]
impl StageFactory<f32, f32> for AcquireFactory {
    async fn create(
        &self,
        config: &StageConfig,
    ) -> Result<Box<dyn Stage<f32, f32>>, StageError> {
        let params: AcquireParams = serde_json::from_value(serde_json::Value::Object(
            config
                .params
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ))?;
        let interval_ms = (params.samples_per_packet as f32 / params.sps as f32 * 1000.0) as u64;
        let interval = Duration::from_millis(interval_ms.max(1));

        Ok(Box::new(Acquire {
            id: config.name.clone(),
            samples_per_packet: params.samples_per_packet,
            interval,
            packet_counter: AtomicU64::new(0),
        }))
    }
}

pub struct Acquire {
    id: String,
    samples_per_packet: usize,
    interval: Duration,
    packet_counter: AtomicU64,
}

#[derive(Debug, Deserialize)]
struct AcquireParams {
    #[serde(default = "default_sps")]
    sps: u32,
    #[serde(default = "default_samples_per_packet")]
    samples_per_packet: usize,
}

fn default_sps() -> u32 {
    500
}

fn default_samples_per_packet() -> usize {
    50
}

#[async_trait]
impl Stage<f32, f32> for Acquire {
    fn id(&self) -> &str {
        &self.id
    }

    async fn process(
        &mut self,
        _packet: Packet<f32>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Packet<f32>>, StageError> {
        let mut interval = interval(self.interval);
        interval.tick().await;

        let frame_id = self.packet_counter.fetch_add(1, Ordering::Relaxed);
        let samples = (0..self.samples_per_packet)
            .map(|i| {
                let time = frame_id as f32 * (self.samples_per_packet as f32) + i as f32;
                (time * 0.1).sin()
            })
            .collect();

        debug!("Generated packet #{}", frame_id);

        Ok(Some(Packet {
            header: Default::default(),
            samples,
        }))
    }
}