//! Event bus implementation for the EEG daemon
//! 
//! This module provides a high-performance, non-blocking event distribution system
//! that allows plugins to communicate through events while maintaining system stability
//! through back-pressure handling.

use tokio::sync::{mpsc, RwLock};
use tracing::{debug, warn};
use async_trait::async_trait;

use eeg_types::{SensorEvent, EventFilter, event_matches_filter};

/// Subscriber information for the event bus
#[derive(Debug)]
struct Subscriber {
    /// Channel sender for this subscriber
    sender: mpsc::Sender<SensorEvent>,
    /// Event filters for this subscriber
    filters: Vec<EventFilter>,
    /// Subscriber name for logging
    name: String,
}

/// High-performance event bus for distributing sensor events to plugins
/// 
/// The EventBus uses a non-blocking design with back-pressure handling:
/// - Uses RwLock for concurrent read access during broadcast
/// - Uses try_send to avoid blocking on slow consumers
/// - Automatically removes dead subscribers
/// - Tracks metrics for monitoring
pub struct EventBus {
    /// List of active subscribers
    subscribers: RwLock<Vec<Subscriber>>,
    /// Metrics for monitoring
    metrics: RwLock<EventBusMetrics>,
}

/// Event bus performance metrics
#[derive(Debug, Default)]
pub struct EventBusMetrics {
    /// Total events broadcast
    pub events_broadcast: u64,
    /// Total events delivered successfully
    pub events_delivered: u64,
    /// Total events dropped due to full buffers
    pub events_dropped: u64,
    /// Number of dead subscribers removed
    pub dead_subscribers_removed: u64,
    /// Current number of active subscribers
    pub active_subscribers: usize,
}

impl EventBus {
    /// Create a new event bus
    pub fn new() -> Self {
        Self {
            subscribers: RwLock::new(Vec::new()),
            metrics: RwLock::new(EventBusMetrics::default()),
        }
    }

    /// Subscribe to events with optional filters
    /// 
    /// Returns a receiver channel that will receive events matching the filters.
    /// If no filters are provided, all events will be received.
    pub async fn subscribe(
        &self,
        name: String,
        buffer_size: usize,
        filters: Vec<EventFilter>,
    ) -> mpsc::Receiver<SensorEvent> {
        let (sender, receiver) = mpsc::channel(buffer_size);
        
        let subscriber = Subscriber {
            sender,
            filters,
            name: name.clone(),
        };
        
        self.subscribers.write().await.push(subscriber);
        
        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.active_subscribers = self.subscribers.read().await.len();
        }
        
        debug!(subscriber = %name, buffer_size, "New subscriber registered");
        receiver
    }

    /// Subscribe to all events (convenience method)
    pub async fn subscribe_all(
        &self,
        name: String,
        buffer_size: usize,
    ) -> mpsc::Receiver<SensorEvent> {
        self.subscribe(name, buffer_size, vec![EventFilter::All]).await
    }

    /// Broadcast an event to all matching subscribers
    /// 
    /// This method is non-blocking and uses back-pressure handling:
    /// - Checks channel capacity before attempting to send
    /// - Uses try_send to avoid blocking
    /// - Removes dead subscribers automatically
    /// - Logs warnings for dropped events
    pub async fn broadcast(&self, event: SensorEvent) {
        let subscribers = self.subscribers.read().await;
        let mut dead_indices = Vec::new();
        let mut delivered_count = 0;
        let mut dropped_count = 0;

        debug!(
            event_type = event.event_type_name(),
            timestamp = event.timestamp(),
            subscriber_count = subscribers.len(),
            "Broadcasting event"
        );

        for (i, subscriber) in subscribers.iter().enumerate() {
            // Check if this subscriber wants this event
            let wants_event = subscriber.filters.is_empty() || 
                subscriber.filters.iter().any(|filter| event_matches_filter(&event, filter));
            
            if !wants_event {
                continue;
            }

            // Optimization: Check capacity first to avoid clone if channel is full
            if subscriber.sender.capacity() == 0 {
                warn!(
                    subscriber = %subscriber.name,
                    event_type = event.event_type_name(),
                    "Subscriber buffer full, dropping event"
                );
                dropped_count += 1;
                continue;
            }

            // try_send is instantaneous and will fail if channel is full or closed
            match subscriber.sender.try_send(event.clone()) {
                Ok(()) => {
                    delivered_count += 1;
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    // This shouldn't happen due to capacity check above, but handle it
                    warn!(
                        subscriber = %subscriber.name,
                        event_type = event.event_type_name(),
                        "Subscriber buffer became full during send"
                    );
                    dropped_count += 1;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    debug!(
                        subscriber = %subscriber.name,
                        "Subscriber channel closed, marking for removal"
                    );
                    dead_indices.push(i);
                }
            }
        }

        // Remove dead subscribers if any were found
        if !dead_indices.is_empty() {
            drop(subscribers); // Release read lock
            let mut subscribers_write = self.subscribers.write().await;
            
            // Remove in reverse order to maintain indices
            for &i in dead_indices.iter().rev() {
                let removed = subscribers_write.remove(i);
                debug!(subscriber = %removed.name, "Removed dead subscriber");
            }
            
            // Update metrics
            let mut metrics = self.metrics.write().await;
            metrics.dead_subscribers_removed += dead_indices.len() as u64;
            metrics.active_subscribers = subscribers_write.len();
        }

        // Update broadcast metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.events_broadcast += 1;
            metrics.events_delivered += delivered_count;
            metrics.events_dropped += dropped_count;
        }

        debug!(
            event_type = event.event_type_name(),
            delivered = delivered_count,
            dropped = dropped_count,
            dead_removed = dead_indices.len(),
            "Event broadcast complete"
        );
    }

    /// Get current metrics
    pub async fn get_metrics(&self) -> EventBusMetrics {
        self.metrics.read().await.clone()
    }

    /// Reset metrics (useful for testing)
    pub async fn reset_metrics(&self) {
        let mut metrics = self.metrics.write().await;
        *metrics = EventBusMetrics::default();
        metrics.active_subscribers = self.subscribers.read().await.len();
    }

    /// Get current subscriber count
    pub async fn subscriber_count(&self) -> usize {
        self.subscribers.read().await.len()
    }

    /// Get subscriber names (for debugging)
    pub async fn subscriber_names(&self) -> Vec<String> {
        self.subscribers.read().await
            .iter()
            .map(|s| s.name.clone())
            .collect()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for EventBusMetrics {
    fn clone(&self) -> Self {
        Self {
            events_broadcast: self.events_broadcast,
            events_delivered: self.events_delivered,
            events_dropped: self.events_dropped,
            dead_subscribers_removed: self.dead_subscribers_removed,
            active_subscribers: self.active_subscribers,
        }
    }
}

#[async_trait]
impl eeg_types::EventBus for EventBus {
    async fn broadcast(&self, event: SensorEvent) {
        self.broadcast(event).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use eeg_types::{EegPacket, SensorEvent};
    use super::EventFilter;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_event_bus_basic_functionality() {
        let bus = EventBus::new();
        
        // Subscribe to events
        let mut receiver = bus.subscribe_all("test_plugin".to_string(), 10).await;
        
        // Create and broadcast an event
        let packet = Arc::new(EegPacket::new(1000, 1, vec![1.0, 2.0], 1, 250.0));
        let event = SensorEvent::RawEeg(packet);
        
        bus.broadcast(event.clone()).await;
        
        // Receive the event
        let received = timeout(Duration::from_millis(100), receiver.recv())
            .await
            .expect("Should receive event within timeout")
            .expect("Should receive an event");
        
        assert_eq!(received.timestamp(), event.timestamp());
    }

    #[tokio::test]
    async fn test_event_filtering() {
        let bus = EventBus::new();
        
        // Subscribe only to raw EEG events
        let mut raw_receiver = bus.subscribe(
            "raw_only".to_string(),
            10,
            vec![EventFilter::RawEegOnly]
        ).await;
        
        // Subscribe only to system events
        let mut system_receiver = bus.subscribe(
            "system_only".to_string(),
            10,
            vec![EventFilter::SystemOnly]
        ).await;
        
        // Broadcast a raw EEG event
        let packet = Arc::new(EegPacket::new(1000, 1, vec![1.0], 1, 250.0));
        let raw_event = SensorEvent::RawEeg(packet);
        bus.broadcast(raw_event).await;
        
        // Raw subscriber should receive it
        assert!(timeout(Duration::from_millis(100), raw_receiver.recv()).await.is_ok());
        
        // System subscriber should not receive it
        assert!(timeout(Duration::from_millis(100), system_receiver.recv()).await.is_err());
    }

    #[tokio::test]
    async fn test_back_pressure_handling() {
        let bus = EventBus::new();
        
        // Create a subscriber with a very small buffer
        let mut receiver = bus.subscribe_all("small_buffer".to_string(), 1).await;
        
        // Fill the buffer
        let packet1 = Arc::new(EegPacket::new(1000, 1, vec![1.0], 1, 250.0));
        bus.broadcast(SensorEvent::RawEeg(packet1)).await;
        
        // Try to send another event (should be dropped due to full buffer)
        let packet2 = Arc::new(EegPacket::new(2000, 2, vec![2.0], 1, 250.0));
        bus.broadcast(SensorEvent::RawEeg(packet2)).await;
        
        // Check metrics
        let metrics = bus.get_metrics().await;
        assert_eq!(metrics.events_broadcast, 2);
        assert_eq!(metrics.events_delivered, 1);
        assert_eq!(metrics.events_dropped, 1);
        
        // Drain the receiver
        let _ = receiver.recv().await;
    }

    #[tokio::test]
    async fn test_dead_subscriber_removal() {
        let bus = EventBus::new();
        
        // Create a subscriber and then drop the receiver
        {
            let _receiver = bus.subscribe_all("temp_plugin".to_string(), 10).await;
            assert_eq!(bus.subscriber_count().await, 1);
        } // receiver is dropped here
        
        // Broadcast an event, which should trigger dead subscriber removal
        let packet = Arc::new(EegPacket::new(1000, 1, vec![1.0], 1, 250.0));
        bus.broadcast(SensorEvent::RawEeg(packet)).await;
        
        // Subscriber should be removed
        assert_eq!(bus.subscriber_count().await, 0);
        
        let metrics = bus.get_metrics().await;
        assert_eq!(metrics.dead_subscribers_removed, 1);
    }
}