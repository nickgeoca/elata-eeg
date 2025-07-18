//! A placeholder for a filter stage.

use crate::config::StageConfig;
use crate::data::RtPacket;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext};
use std::sync::Arc;

/// A placeholder filter stage.
#[derive(Default)]
pub struct FilterFactory;

impl StageFactory for FilterFactory {
    fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage>, StageError> {
        Ok(Box::new(Filter {
            id: config.name.clone(),
        }))
    }
}

pub struct Filter {
    id: String,
}

impl Stage for Filter {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Arc<RtPacket>>, StageError> {
        Ok(Some(packet))
    }
}
