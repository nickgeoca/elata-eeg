//! Stage registry for creating pipeline stage instances.

use crate::config::StageConfig;
use crate::error::StageError;
use crate::stage::Stage;
use std::collections::HashMap;
/// A factory for creating instances of a specific stage type.
pub trait StageFactory: Send + Sync {
    /// Creates a new stage instance from a config.
    fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage>, StageError>;
}

/// A registry for stage factories.
#[derive(Default)]
pub struct StageRegistry {
    factories: HashMap<String, Box<dyn StageFactory>>,
}

impl StageRegistry {
    /// Creates a new, empty stage registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new stage factory.
    pub fn register(&mut self, name: &str, factory: Box<dyn StageFactory>) {
        self.factories.insert(name.to_string(), factory);
    }

    /// Creates a new stage instance from a config.
    pub fn create_stage(&self, config: &StageConfig) -> Result<Box<dyn Stage>, StageError> {
        self.factories
            .get(&config.stage_type)
            .ok_or_else(|| StageError::NotFound(config.stage_type.clone()))?
            .create(config)
    }
}