//! Connection Manager for tracking WebSocket clients and their DSP requirements
//! 
//! This module manages WebSocket connections and maps them to DSP processing requirements,
//! enabling demand-based processing that only activates DSP components when needed.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use tokio::sync::Mutex;
use eeg_driver::dsp::coordinator::{ClientId, DspRequirements, DspCoordinator};

/// Types of WebSocket clients with different DSP needs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ClientType {
    /// Basic EEG monitoring client (needs filtering only)
    EegMonitor,
    /// Configuration client (no DSP needed)
    Config,
    /// Command/control client (no DSP needed)
    Command,
    /// FFT analysis client (needs filtering + FFT)
    FftAnalysis,
    /// Raw data recording client (needs raw data only)
    RawRecording,
    /// Filtered data client (needs basic filtering)
    FilteredData,
}

impl ClientType {
    /// Convert client type to DSP requirements
    pub fn to_dsp_requirements(&self, channels: Vec<usize>) -> DspRequirements {
        match self {
            ClientType::EegMonitor => DspRequirements::basic_monitoring(channels),
            ClientType::Config => DspRequirements {
                needs_filtering: false,
                needs_fft: false,
                needs_raw: false,
                channels,
            },
            ClientType::Command => DspRequirements {
                needs_filtering: false,
                needs_fft: false,
                needs_raw: false,
                channels,
            },
            ClientType::FftAnalysis => DspRequirements::fft_analysis(channels),
            ClientType::RawRecording => DspRequirements::raw_recording(channels),
            ClientType::FilteredData => DspRequirements::basic_monitoring(channels),
        }
    }
}

/// Manages WebSocket connections and their DSP requirements
pub struct ConnectionManager {
    /// Active connections mapped to their client types
    connections: Arc<Mutex<HashMap<ClientId, ClientType>>>,
    /// Reference to the DSP coordinator
    dsp_coordinator: Arc<Mutex<DspCoordinator>>,
    /// Default channels for new connections
    default_channels: Vec<usize>,
}

impl ConnectionManager {
    /// Create a new connection manager
    pub fn new(dsp_coordinator: Arc<Mutex<DspCoordinator>>, default_channels: Vec<usize>) -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            dsp_coordinator,
            default_channels,
        }
    }

    /// Register a new WebSocket connection
    pub async fn register_connection(&self, client_id: ClientId, client_type: ClientType) -> Result<(), String> {
        println!("ConnectionManager: Registering client {} as {:?}", client_id, client_type);
        
        // Add to connections map
        {
            let mut connections = self.connections.lock().await;
            connections.insert(client_id.clone(), client_type.clone());
        }

        // Register with DSP coordinator if client needs DSP processing
        let requirements = client_type.to_dsp_requirements(self.default_channels.clone());
        if requirements.needs_filtering || requirements.needs_fft || requirements.needs_raw {
            let mut coordinator = self.dsp_coordinator.lock().await;
            coordinator.register_client(client_id, requirements).await?;
        }

        Ok(())
    }

    /// Unregister a WebSocket connection
    pub async fn unregister_connection(&self, client_id: &ClientId) -> Result<(), String> {
        println!("ConnectionManager: Unregistering client {}", client_id);
        
        // Remove from connections map
        let client_type = {
            let mut connections = self.connections.lock().await;
            connections.remove(client_id)
        };

        // Unregister from DSP coordinator if client was using DSP
        if let Some(client_type) = client_type {
            let requirements = client_type.to_dsp_requirements(self.default_channels.clone());
            if requirements.needs_filtering || requirements.needs_fft || requirements.needs_raw {
                let mut coordinator = self.dsp_coordinator.lock().await;
                coordinator.unregister_client(client_id).await?;
            }
        }

        Ok(())
    }

    /// Get the current number of active connections
    pub async fn get_connection_count(&self) -> usize {
        let connections = self.connections.lock().await;
        connections.len()
    }

    /// Get connections by type
    pub async fn get_connections_by_type(&self, client_type: ClientType) -> Vec<ClientId> {
        let connections = self.connections.lock().await;
        connections
            .iter()
            .filter(|(_, &ref ct)| *ct == client_type)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get all active connection types
    pub async fn get_active_client_types(&self) -> HashMap<ClientType, usize> {
        let connections = self.connections.lock().await;
        let mut type_counts = HashMap::new();
        
        for client_type in connections.values() {
            *type_counts.entry(client_type.clone()).or_insert(0) += 1;
        }
        
        type_counts
    }

    /// Update default channels (when ADC config changes)
    pub fn update_default_channels(&mut self, channels: Vec<usize>) {
        self.default_channels = channels;
    }

    /// Get current DSP coordinator state
    pub async fn get_dsp_state(&self) -> String {
        let coordinator = self.dsp_coordinator.lock().await;
        format!("{:?}", coordinator.get_state())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eeg_driver::{AdcConfig, DriverType};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    async fn create_test_coordinator() -> Arc<Mutex<DspCoordinator>> {
        let config = Arc::new(Mutex::new(AdcConfig {
            board_driver: DriverType::Mock,
            sample_rate: 250,
            channels: vec![0, 1, 2, 3],
            gain: 1.0,
            batch_size: 10,
            Vref: 4.5,
        }));
        
        let coordinator = DspCoordinator::new(config).await;
        Arc::new(Mutex::new(coordinator))
    }

    #[tokio::test]
    async fn test_connection_registration() {
        let coordinator = create_test_coordinator().await;
        let manager = ConnectionManager::new(coordinator, vec![0, 1, 2, 3]);
        
        // Register EEG monitor client
        let result = manager.register_connection("client1".to_string(), ClientType::EegMonitor).await;
        assert!(result.is_ok());
        
        // Check connection count
        assert_eq!(manager.get_connection_count().await, 1);
        
        // Unregister client
        let result = manager.unregister_connection(&"client1".to_string()).await;
        assert!(result.is_ok());
        
        // Check connection count
        assert_eq!(manager.get_connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_client_type_requirements() {
        let channels = vec![0, 1];
        
        // Test EEG monitor requirements
        let req = ClientType::EegMonitor.to_dsp_requirements(channels.clone());
        assert!(req.needs_filtering);
        assert!(!req.needs_fft);
        assert!(!req.needs_raw);
        
        // Test FFT analysis requirements
        let req = ClientType::FftAnalysis.to_dsp_requirements(channels.clone());
        assert!(req.needs_filtering);
        assert!(req.needs_fft);
        assert!(!req.needs_raw);
        
        // Test config client requirements (no DSP)
        let req = ClientType::Config.to_dsp_requirements(channels.clone());
        assert!(!req.needs_filtering);
        assert!(!req.needs_fft);
        assert!(!req.needs_raw);
    }

    #[tokio::test]
    async fn test_multiple_clients() {
        let coordinator = create_test_coordinator().await;
        let manager = ConnectionManager::new(coordinator, vec![0, 1, 2, 3]);
        
        // Register multiple clients
        manager.register_connection("monitor1".to_string(), ClientType::EegMonitor).await.unwrap();
        manager.register_connection("fft1".to_string(), ClientType::FftAnalysis).await.unwrap();
        manager.register_connection("config1".to_string(), ClientType::Config).await.unwrap();
        
        assert_eq!(manager.get_connection_count().await, 3);
        
        // Check client type counts
        let type_counts = manager.get_active_client_types().await;
        assert_eq!(type_counts.get(&ClientType::EegMonitor), Some(&1));
        assert_eq!(type_counts.get(&ClientType::FftAnalysis), Some(&1));
        assert_eq!(type_counts.get(&ClientType::Config), Some(&1));
    }
}