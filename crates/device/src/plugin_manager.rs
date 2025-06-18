//! Plugin Manager for the Elata EEG Device Daemon
//!
//! This module is responsible for loading, managing, and communicating
//! with the single active plugin according to the v0.6 architecture.

use eeg_sensor::AdcData;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Child;
use tokio::sync::mpsc;
use log::{info, warn, debug};

/// Plugin manifest structure (plugin.toml)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub backend: BackendConfig,
    pub ui: Option<UiConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BackendConfig {
    pub executable: String,
    pub args: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiConfig {
    pub entry_point: String,
    pub assets: Option<Vec<String>>,
}

/// Discovered plugin information
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub manifest: PluginManifest,
    pub path: PathBuf,
}

/// Active plugin state
struct ActivePlugin {
    info: PluginInfo,
    process: Option<Child>,
    data_sender: Option<mpsc::UnboundedSender<AdcData>>,
}

/// Manages the single active plugin according to v0.6 architecture.
pub struct PluginManager {
    plugins_dir: PathBuf,
    discovered_plugins: HashMap<String, PluginInfo>,
    active_plugin: Option<ActivePlugin>,
}

impl PluginManager {
    /// Creates a new `PluginManager` and discovers available plugins.
    pub async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let plugins_dir = PathBuf::from("plugins");
        let mut manager = Self {
            plugins_dir,
            discovered_plugins: HashMap::new(),
            active_plugin: None,
        };
        
        manager.discover_plugins().await?;
        info!("PluginManager initialized with {} plugins discovered", manager.discovered_plugins.len());
        
        Ok(manager)
    }

    /// Discovers all available plugins by scanning the plugins directory.
    async fn discover_plugins(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.plugins_dir.exists() {
            warn!("Plugins directory {:?} does not exist", self.plugins_dir);
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(&self.plugins_dir).await?;
        
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(plugin_info) = self.load_plugin_manifest(&path).await {
                    let name = plugin_info.manifest.name.clone();
                    self.discovered_plugins.insert(name, plugin_info);
                }
            }
        }

        info!("Discovered plugins: {:?}", self.discovered_plugins.keys().collect::<Vec<_>>());
        Ok(())
    }

    /// Loads and parses a plugin manifest from a directory.
    async fn load_plugin_manifest(&self, plugin_dir: &Path) -> Result<PluginInfo, Box<dyn std::error::Error + Send + Sync>> {
        let manifest_path = plugin_dir.join("plugin.toml");
        
        if !manifest_path.exists() {
            return Err(format!("No plugin.toml found in {:?}", plugin_dir).into());
        }

        let manifest_content = tokio::fs::read_to_string(&manifest_path).await?;
        let manifest: PluginManifest = toml::from_str(&manifest_content)?;
        
        Ok(PluginInfo {
            manifest,
            path: plugin_dir.to_path_buf(),
        })
    }

    /// Returns a list of all discovered plugins.
    pub fn list_plugins(&self) -> Vec<&PluginInfo> {
        self.discovered_plugins.values().collect()
    }

    /// Gets the currently active plugin name, if any.
    pub fn get_active_plugin(&self) -> Option<&str> {
        self.active_plugin.as_ref().map(|p| p.info.manifest.name.as_str())
    }

    /// Activates a plugin by name. Only one plugin can be active at a time.
    pub async fn activate_plugin(&mut self, plugin_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Deactivate current plugin if any
        if let Some(_) = &self.active_plugin {
            self.deactivate_current_plugin().await?;
        }

        // Find the requested plugin
        let plugin_info = self.discovered_plugins.get(plugin_name)
            .ok_or_else(|| format!("Plugin '{}' not found", plugin_name))?
            .clone();

        info!("Activating plugin: {}", plugin_name);

        // For now, we'll implement a simple logging-based plugin
        // In the future, this would spawn the actual backend process
        let active_plugin = ActivePlugin {
            info: plugin_info,
            process: None,
            data_sender: None,
        };

        self.active_plugin = Some(active_plugin);
        info!("Plugin '{}' activated successfully", plugin_name);
        
        Ok(())
    }

    /// Deactivates the currently active plugin.
    pub async fn deactivate_current_plugin(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(mut active_plugin) = self.active_plugin.take() {
            info!("Deactivating plugin: {}", active_plugin.info.manifest.name);
            
            // Close data sender
            if let Some(sender) = active_plugin.data_sender.take() {
                drop(sender);
            }
            
            // Terminate process if running
            if let Some(mut process) = active_plugin.process.take() {
                if let Err(e) = process.kill() {
                    warn!("Failed to kill plugin process: {}", e);
                }
            }
            
            info!("Plugin deactivated successfully");
        }
        
        Ok(())
    }

    /// Sends raw ADC data to the active plugin.
    pub async fn send_data(&self, data: AdcData) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(active_plugin) = &self.active_plugin {
            // For now, just log the data reception
            // In a full implementation, this would send data via IPC to the plugin backend
            debug!("Forwarding AdcData to plugin '{}': timestamp={}, channel={}, value={}",
                   active_plugin.info.manifest.name,
                   data.timestamp,
                   data.channel,
                   data.value);
            
            // TODO: Implement actual IPC mechanism (WebSocket, named pipes, or shared memory)
            // For now, we'll just demonstrate the data flow
            
            Ok(())
        } else {
            debug!("No active plugin - dropping AdcData with timestamp {}", data.timestamp);
            Ok(())
        }
    }

    /// Shuts down the plugin manager and any active plugins.
    pub async fn shutdown(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Shutting down PluginManager");
        self.deactivate_current_plugin().await?;
        Ok(())
    }
}

impl Drop for PluginManager {
    fn drop(&mut self) {
        if let Some(active_plugin) = &mut self.active_plugin {
            if let Some(mut process) = active_plugin.process.take() {
                let _ = process.kill();
            }
        }
    }
}