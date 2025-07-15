//! Data acquisition stage for EEG sensors.

use crate::config::StageConfig;
use crate::control::ControlCommand;
use crate::data::Packet;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext};
use serde::Deserialize;
use tracing::debug;

/// A source stage that generates mock raw EEG data.
#[derive(Default)]
pub struct AcquireFactory;

impl StageFactory for AcquireFactory {
    fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage>, StageError> {
        // We still parse the params to validate the config, but they are not used in this pass-through version.
        let _: AcquireParams = serde_json::from_value(serde_json::Value::Object(
            config
                .params
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ))?;

        Ok(Box::new(Acquire {
            id: config.name.clone(),
        }))
    }
}

pub struct Acquire {
    id: String,
}

#[derive(Debug, Deserialize)]
struct AcquireParams {
    #[serde(default = "default_samples_per_packet")]
    samples_per_packet: usize,
}

fn default_samples_per_packet() -> usize {
    50
}

impl Stage for Acquire {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Packet,
        _ctx: &mut StageContext,
    ) -> Result<Option<Packet>, StageError> {
        // In this test setup, the Acquire stage is just a pass-through.
        // The test itself is the source of the data.
        debug!("Acquire stage passing packet through.");
        Ok(Some(packet))
    }
    fn control(
        &mut self,
        cmd: &ControlCommand,
        _ctx: &mut StageContext,
    ) -> Result<(), StageError> {
        if let ControlCommand::Start = cmd {
            debug!("Acquire stage received Start command. Data generation from the control path is deprecated.");
        }
        Ok(())
    }
}