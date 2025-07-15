//! Stage registry for creating pipeline stage instances.

use crate::config::StageConfig;
use crate::error::StageError;
use crate::stage::Stage;
use async_trait::async_trait;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

/// A factory for creating instances of a specific stage type.
#[async_trait]
pub trait StageFactory<I, O>: Send + Sync {
    /// Creates a new stage instance from a config.
    async fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage<I, O>>, StageError>;
}

/// A registry for stage factories.
pub struct StageRegistry<I, O> {
    factories: HashMap<String, Arc<dyn StageFactory<I, O>>>,
    _marker: PhantomData<(I, O)>,
}

impl<I, O> Default for StageRegistry<I, O> {
    fn default() -> Self {
        Self {
            factories: HashMap::new(),
            _marker: PhantomData,
        }
    }
}

impl<I, O> StageRegistry<I, O> {
    /// Creates a new, empty stage registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new stage factory.
    pub fn register<F>(&mut self, name: &str, factory: F)
    where
        F: StageFactory<I, O> + 'static,
    {
        self.factories.insert(name.to_string(), Arc::new(factory));
    }

    /// Creates a new stage instance from a config.
    pub async fn create_stage(
        &self,
        config: &StageConfig,
    ) -> Result<Box<dyn Stage<I, O>>, StageError> {
        self.factories
            .get(&config.stage_type)
            .ok_or_else(|| StageError::NotFound(config.stage_type.clone()))?
            .create(config)
            .await
    }
}