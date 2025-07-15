//! A placeholder for a filter stage.

use crate::config::StageConfig;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext};
use std::any::Any;

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
        packet: Box<dyn Any + Send>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Box<dyn Any + Send>>, StageError> {
        Ok(Some(packet))
    }
}
