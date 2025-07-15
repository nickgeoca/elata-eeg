//! A placeholder for a filter stage.

use crate::config::StageConfig;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext};
use async_trait::async_trait;
use eeg_types::Packet;

/// A placeholder filter stage.
#[derive(Default)]
pub struct FilterFactory;

#[async_trait]
impl StageFactory<f32, f32> for FilterFactory {
    async fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage<f32, f32>>, StageError> {
        Ok(Box::new(Filter {
            id: config.name.clone(),
        }))
    }
}

pub struct Filter {
    id: String,
}

#[async_trait]
impl Stage<f32, f32> for Filter {
    fn id(&self) -> &str {
        &self.id
    }

    async fn process(
        &mut self,
        packet: Packet<f32>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Packet<f32>>, StageError> {
        Ok(Some(packet))
    }
}
