//! Plugin loading and management for the pipeline.
//
// This module defines the Application Binary Interface (ABI) for pipeline plugins,
// allowing them to be loaded dynamically at runtime.

use libloading::{Library, Symbol};
use std::path::Path;
use crate::stage::{StageRegistry, StageFactory};
use crate::error::PipelineResult;

/// The ABI that all plugins must implement.
#[repr(C)]
pub struct PluginAbi {
    /// A function that registers the plugin's stage factories.
    pub register_factories: unsafe extern "C" fn(&mut dyn PluginRegistrar),
}

/// A trait that allows plugins to register their stage factories.
pub trait PluginRegistrar {
    /// Registers a stage factory with the pipeline's stage registry.
    fn register_stage_factory(&mut self, factory: Box<dyn StageFactory>);
}

impl PluginRegistrar for StageRegistry {
    fn register_stage_factory(&mut self, factory: Box<dyn StageFactory>) {
        self.register(factory);
    }
}

/// Manages the loading and registration of plugins.
pub struct PluginManager {
    /// The loaded plugin libraries.
    loaded_libraries: Vec<Library>,
}

impl PluginManager {
    /// Creates a new `PluginManager`.
    pub fn new() -> Self {
        Self {
            loaded_libraries: Vec::new(),
        }
    }

    /// Loads all plugins from a given directory and registers their stage factories.
    pub fn load_plugins_from_dir(
        &mut self,
        dir: &Path,
        registry: &mut StageRegistry,
    ) -> PipelineResult<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "so" || ext == "dll" || ext == "dylib" {
                        unsafe {
                            self.load_plugin(&path, registry)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Loads a single plugin and registers its stage factories.
    unsafe fn load_plugin(
        &mut self,
        path: &Path,
        registry: &mut dyn PluginRegistrar,
    ) -> PipelineResult<()> {
        let lib = Library::new(path)?;

        let register_factories: Symbol<unsafe extern "C" fn(&mut dyn PluginRegistrar)> =
            lib.get(b"register_factories")?;

        register_factories(registry);

        self.loaded_libraries.push(lib);

        Ok(())
    }
}