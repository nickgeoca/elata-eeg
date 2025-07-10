//! Data types for pipeline communication
//! 
//! This module defines the type-safe data structures used for communication
//! between pipeline stages, replacing the unsafe `Box<dyn Any>` approach.

use std::sync::{Arc, Weak, Mutex};
use serde::{Serialize, Deserialize};
use eeg_types::{EegPacket, FilteredEegPacket, FftPacket};
use crossbeam_queue::ArrayQueue;
use anyhow::{Result, anyhow};
use tracing::{debug, error};
use std::fmt::Debug; // Added for Debug trait bound

/// Pipeline data that can flow between stages
#[derive(Debug, Clone)]
pub enum PipelineData {
    /// Raw EEG data from acquisition
    RawEeg(Arc<EegPacket>),
    /// Filtered EEG data
    FilteredEeg(Arc<FilteredEegPacket>),
    /// FFT analysis results
    Fft(Arc<FftPacket>),
    /// Trigger signal for source stages (no data payload)
    Trigger,
    /// CSV record command with data
    CsvRecord {
        data: CsvData,
        file_path: String,
    },
    /// WebSocket broadcast command with data
    WebSocketBroadcast {
        data: WebSocketData,
        endpoint: String,
    },
}

/// Data for CSV recording
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsvData {
    /// Timestamp
    pub timestamp: u64,
    /// Frame ID for tracking
    pub frame_id: u64,
    /// Channel data (can be raw, voltage, or filtered)
    pub channels: Vec<ChannelData>,
    /// Sample rate
    pub sample_rate: f32,
}

/// Data for a single channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelData {
    /// Channel index
    pub channel: usize,
    /// Sample values
    pub samples: Vec<f32>,
}

/// Data for WebSocket broadcasting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketData {
    /// Timestamp
    pub timestamp: u64,
    /// Frame ID for tracking
    pub frame_id: u64,
    /// Data format (json, binary)
    pub format: String,
    /// Serialized payload
    pub payload: Vec<u8>,
}

impl PipelineData {
    /// Get the timestamp of the data (if applicable)
    pub fn timestamp(&self) -> Option<u64> {
        match self {
            PipelineData::RawEeg(packet) => packet.timestamps.first().copied(),
            PipelineData::FilteredEeg(packet) => packet.timestamps.first().copied(),
            PipelineData::Fft(packet) => Some(packet.timestamp),
            PipelineData::CsvRecord { data, .. } => Some(data.timestamp),
            PipelineData::WebSocketBroadcast { data, .. } => Some(data.timestamp),
            PipelineData::Trigger => None,
        }
    }

    /// Get the frame ID of the data (if applicable)
    pub fn frame_id(&self) -> Option<u64> {
        match self {
            PipelineData::RawEeg(packet) => Some(packet.frame_id),
            PipelineData::FilteredEeg(packet) => Some(packet.frame_id),
            PipelineData::Fft(packet) => Some(packet.source_frame_id),
            PipelineData::CsvRecord { data, .. } => Some(data.frame_id),
            PipelineData::WebSocketBroadcast { data, .. } => Some(data.frame_id),
            PipelineData::Trigger => None,
        }
    }

    /// Get a human-readable description of the data type
    pub fn data_type(&self) -> &'static str {
        match self {
            PipelineData::RawEeg(_) => "RawEeg",
            PipelineData::FilteredEeg(_) => "FilteredEeg",
            PipelineData::Fft(_) => "Fft",
            PipelineData::CsvRecord { .. } => "CsvRecord",
            PipelineData::WebSocketBroadcast { .. } => "WebSocketBroadcast",
            PipelineData::Trigger => "Trigger",
        }
    }
}

impl CsvData {
    /// Create CSV data from an EEG packet
    pub fn from_eeg_packet(packet: &EegPacket, fields: &[String]) -> Self {
        let mut channels = Vec::new();
        
        // Extract requested fields
        for field in fields {
            match field.as_str() {
                "raw_channels" => {
                    for ch in 0..packet.channel_count {
                        if let Some(samples) = packet.channel_raw_samples(ch) {
                            channels.push(ChannelData {
                                channel: ch,
                                samples: samples.iter().map(|&s| s as f32).collect(),
                            });
                        }
                    }
                }
                "voltage_channels" => {
                    for ch in 0..packet.channel_count {
                        if let Some(samples) = packet.channel_voltage_samples(ch) {
                            channels.push(ChannelData {
                                channel: ch,
                                samples: samples.to_vec(),
                            });
                        }
                    }
                }
                _ => {
                    // Skip unknown fields
                }
            }
        }

        Self {
            timestamp: packet.timestamps.first().copied().unwrap_or(0),
            frame_id: packet.frame_id,
            channels,
            sample_rate: packet.sample_rate,
        }
    }

    /// Create CSV data from a filtered EEG packet
    pub fn from_filtered_packet(packet: &FilteredEegPacket, fields: &[String]) -> Self {
        let mut channels = Vec::new();
        
        // Extract requested fields
        for field in fields {
            match field.as_str() {
                "filtered_channels" => {
                    for ch in 0..packet.channel_count {
                        if let Some(samples) = packet.channel_samples(ch) {
                            channels.push(ChannelData {
                                channel: ch,
                                samples: samples.to_vec(),
                            });
                        }
                    }
                }
                _ => {
                    // Skip unknown fields
                }
            }
        }

        Self {
            timestamp: packet.timestamps.first().copied().unwrap_or(0),
            frame_id: packet.frame_id,
            channels,
            sample_rate: packet.sample_rate,
        }
    }
}

impl WebSocketData {
    /// Create WebSocket data from an EEG packet
    pub fn from_eeg_packet(packet: &EegPacket, format: &str) -> Self {
        let payload = match format {
            "binary" => packet.to_binary(),
            "json" => {
                // Create a JSON representation
                let json_data = serde_json::json!({
                    "timestamp": packet.timestamps.first().copied().unwrap_or(0),
                    "frame_id": packet.frame_id,
                    "channel_count": packet.channel_count,
                    "sample_rate": packet.sample_rate,
                    "voltage_samples": packet.voltage_samples.as_ref(),
                });
                serde_json::to_vec(&json_data).unwrap_or_default()
            }
            _ => Vec::new(),
        };

        Self {
            timestamp: packet.timestamps.first().copied().unwrap_or(0),
            frame_id: packet.frame_id,
            format: format.to_string(),
            payload,
        }
    }

    /// Create WebSocket data from a filtered EEG packet
    pub fn from_filtered_packet(packet: &FilteredEegPacket, format: &str) -> Self {
        let payload = match format {
            "binary" => packet.to_binary(),
            "json" => {
                // Create a JSON representation
                let json_data = serde_json::json!({
                    "timestamp": packet.timestamps.first().copied().unwrap_or(0),
                    "frame_id": packet.frame_id,
                    "channel_count": packet.channel_count,
                    "sample_rate": packet.sample_rate,
                    "samples": packet.samples.as_ref(),
                });
                serde_json::to_vec(&json_data).unwrap_or_default()
            }
            _ => Vec::new(),
        };

        Self {
            timestamp: packet.timestamps.first().copied().unwrap_or(0),
            frame_id: packet.frame_id,
            format: format.to_string(),
            payload,
        }
    }
}

/// The header contains metadata about the packet.
#[derive(Debug, Clone, Copy)]
pub struct PacketHeader {
    pub batch_size: usize,
    pub timestamp: u64,
}

/// A smart pointer that contains a header and a buffer.
/// Its `Drop` implementation automatically returns the buffer to its origin pool.
#[derive(Debug)]
pub struct Packet<T: TakeForDrop + Default + Debug> { // Added required bounds
    pub header: PacketHeader,
    pub samples: T,
    // A weak reference to the pool this packet came from.
    // This allows the packet to return itself to the pool when dropped,
    // without creating a circular reference that would prevent the pool from being dropped.
    pool: Weak<Mutex<MemoryPool<T>>>,
}

impl<T: TakeForDrop + Default + Debug> Packet<T> { // Added required bounds
    /// Creates a new packet associated with a memory pool.
    pub fn new(header: PacketHeader, samples: T, pool: Weak<Mutex<MemoryPool<T>>>) -> Self {
        Self {
            header,
            samples,
            pool,
        }
    }

    /// Returns a weak reference to the memory pool this packet belongs to.
    pub fn pool(&self) -> Weak<Mutex<MemoryPool<T>>> {
        self.pool.clone()
    }

    /// A constructor for easily creating packets in unit tests.
    #[cfg(test)]
    pub fn new_for_test(samples: T) -> Self {
        Self {
            header: PacketHeader {
                batch_size: 0, // Not used in these tests
                timestamp: 0,  // Not used in these tests
            },
            samples,
            pool: Weak::new(), // No pool association for test packets
        }
    }

    /// Create a new packet for testing purposes with custom header (without a memory pool)
    #[cfg(test)]
    pub fn new_test(header: PacketHeader, samples: T) -> Self {
        Self {
            header,
            samples,
            pool: Weak::new(), // No pool association for test packets
        }
    }
}

impl<T: TakeForDrop + Default + Debug> Drop for Packet<T> { // Added required bounds
    fn drop(&mut self) {
        if let Some(pool_arc) = self.pool.upgrade() {
            let pool_mutex = pool_arc.lock().unwrap();
            // SAFETY: We are returning the `samples` back to the pool.
            // The `samples` field is moved out of the Packet, and the Packet is then dropped.
            // The MemoryPool ensures that the capacity is correct for the type T.
            // This relies on the invariant that the `samples` field was originally acquired from this pool.
            let samples_to_return = std::mem::replace(&mut self.samples, T::default()).take_for_drop();
            match pool_mutex.queue.push(samples_to_return) {
                Ok(_) => debug!("Packet returned to pool"),
                Err(_) => error!("Failed to return packet to pool: queue is full or disconnected"),
            }
        } else {
            debug!("Packet dropped without an associated pool or pool was already dropped.");
        }
    }
}

// Helper trait to allow taking ownership of `samples` in `Drop`
// This is a workaround because `Drop` takes `&mut self`, and we need to move `samples` out.
// In a real scenario, `T` would likely be a `Vec<f32>` or similar, and we'd manage its capacity.
// For now, we'll assume `T` can be "emptied" or reset.
pub trait TakeForDrop: Sized { // Added Sized bound
    fn take_for_drop(self) -> Self;
}

impl TakeForDrop for Vec<f32> {
    fn take_for_drop(mut self) -> Self {
        self.clear(); // Clear the vector, but keep capacity
        self
    }
}

// Placeholder for other types that might be used as `T` in `Packet`
impl TakeForDrop for VoltageEegPacket {
    fn take_for_drop(mut self) -> Self {
        self.samples.clear();
        self
    }
}

/// A thread-safe, lock-free memory pool for pre-allocated packets.
#[derive(Debug)]
pub struct MemoryPool<T: Default + TakeForDrop + Debug> { // Added Debug bound
    queue: ArrayQueue<T>,
    capacity: usize,
}

impl<T: Default + TakeForDrop + Debug> MemoryPool<T> { // Added Debug bound
    /// Creates a new `MemoryPool` with a given capacity.
    pub fn new(capacity: usize) -> Self {
        let queue = ArrayQueue::new(capacity);
        for _ in 0..capacity {
            queue.push(T::default()).expect("Failed to pre-fill memory pool");
        }
        debug!("MemoryPool created with capacity: {}", capacity);
        Self { queue, capacity }
    }

    /// Asynchronously acquires a packet from the pool. Waits until a packet is available.
    pub async fn acquire(self_arc: &Arc<Mutex<Self>>, header: PacketHeader) -> Result<Packet<T>> { // Changed self to self_arc
        let pool_weak = Arc::downgrade(self_arc);
        loop {
            let packet_option = {
                let pool = self_arc.lock().unwrap();
                pool.queue.pop()
            };
            
            match packet_option {
                Some(mut samples) => {
                    // Reset samples before returning
                    samples = samples.take_for_drop();
                    let remaining = {
                        let pool = self_arc.lock().unwrap();
                        pool.queue.len()
                    };
                    debug!("Packet acquired from pool. Remaining: {}", remaining);
                    return Ok(Packet::new(header, samples, pool_weak));
                },
                None => {
                    debug!("MemoryPool is empty, waiting for a packet to be returned.");
                    tokio::task::yield_now().await; // Yield to allow other tasks to run and potentially return packets
                }
            }
        }
    }

    /// Tries to acquire a packet from the pool without blocking.
    pub fn try_acquire(self_arc: &Arc<Mutex<Self>>, header: PacketHeader) -> Option<Packet<T>> { // Changed self to self_arc
        let pool_weak = Arc::downgrade(self_arc);
        match self_arc.lock().unwrap().queue.pop() { // Changed self to self_arc
            Some(mut samples) => {
                // Reset samples before returning
                samples = samples.take_for_drop();
                debug!("Packet acquired from pool (non-blocking). Remaining: {}", self_arc.lock().unwrap().queue.len()); // Changed self to self_arc
                Some(Packet::new(header, samples, pool_weak))
            },
            None => {
                debug!("MemoryPool is empty (non-blocking).");
                None
            }
        }
    }

    /// Returns the current number of available packets in the pool.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns the total capacity of the pool.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Checks if the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Checks if the pool is full.
    pub fn is_full(&self) -> bool {
        self.queue.is_full()
    }
}

/// Example data packet for raw EEG data.
#[derive(Debug, Default)]
pub struct RawEegPacket {
    pub samples: Vec<i32>, // Raw ADC values
}

/// Example data packet for voltage data.
#[derive(Debug, Default)]
pub struct VoltageEegPacket {
    pub samples: Vec<f32>,
}

impl TakeForDrop for RawEegPacket {
    fn take_for_drop(mut self) -> Self {
        self.samples.clear();
        self
    }
}

/// A trait for any type that can be used as the `samples` field in a `Packet`.
/// This allows for type erasure in `StageContext` and other generic contexts.
pub trait AnyPacketType: TakeForDrop + Default + Debug + Send + Sync + 'static {}

impl AnyPacketType for RawEegPacket {}
impl AnyPacketType for VoltageEegPacket {}

#[cfg(test)]
mod tests {
    use super::*;
    use eeg_types::EegPacket;
    use tokio::time::{timeout, Duration};

    #[test]
    fn test_pipeline_data_timestamp() {
        let packet = Arc::new(EegPacket::new(
            vec![1000, 1001],
            42,
            vec![100, 200],
            vec![1.0, 2.0],
            1,
            250.0,
        ));
        
        let data = PipelineData::RawEeg(packet);
        assert_eq!(data.timestamp(), Some(1000));
        assert_eq!(data.frame_id(), Some(42));
        assert_eq!(data.data_type(), "RawEeg");
    }

    #[test]
    fn test_csv_data_from_eeg_packet() {
        let packet = EegPacket::new(
            vec![1000, 1001],
            42,
            vec![100, 200],
            vec![1.0, 2.0],
            1,
            250.0,
        );
        
        let csv_data = CsvData::from_eeg_packet(&packet, &["voltage_channels".to_string()]);
        assert_eq!(csv_data.timestamp, 1000);
        assert_eq!(csv_data.frame_id, 42);
        assert_eq!(csv_data.channels.len(), 1);
        assert_eq!(csv_data.channels[0].samples, vec![1.0, 2.0]);
    }

    #[tokio::test]
    async fn test_memory_pool_acquire_release() {
        let pool_capacity = 2;
        let pool = Arc::new(Mutex::new(MemoryPool::<Vec<f32>>::new(pool_capacity)));

        assert_eq!(pool.lock().unwrap().len(), pool_capacity);

        let header = PacketHeader { batch_size: 10, timestamp: 123 };
        let pkt1 = MemoryPool::acquire(&pool, header).await.unwrap(); // Corrected call
        assert_eq!(pool.lock().unwrap().len(), pool_capacity - 1);
        assert_eq!(pkt1.header.batch_size, 10);

        let pkt2 = MemoryPool::acquire(&pool, header).await.unwrap(); // Corrected call
        assert_eq!(pool.lock().unwrap().len(), pool_capacity - 2);

        // Pool should be empty now
        assert!(pool.lock().unwrap().is_empty());

        // Dropping pkt1 should return it to the pool
        drop(pkt1);
        assert_eq!(pool.lock().unwrap().len(), pool_capacity - 1);

        // Dropping pkt2 should return it to the pool
        drop(pkt2);
        assert_eq!(pool.lock().unwrap().len(), pool_capacity);
    }

    #[tokio::test]
    async fn test_memory_pool_try_acquire() {
        let pool_capacity = 1;
        let pool = Arc::new(Mutex::new(MemoryPool::<Vec<f32>>::new(pool_capacity)));

        let header = PacketHeader { batch_size: 10, timestamp: 123 };
        let pkt1 = MemoryPool::try_acquire(&pool, header).unwrap(); // Corrected call
        assert_eq!(pool.lock().unwrap().len(), 0);

        // Try to acquire when empty, should return None
        assert!(MemoryPool::try_acquire(&pool, header).is_none()); // Corrected call

        drop(pkt1);
        assert_eq!(pool.lock().unwrap().len(), 1);

        let pkt2 = MemoryPool::try_acquire(&pool, header).unwrap(); // Corrected call
        assert_eq!(pool.lock().unwrap().len(), 0);
        drop(pkt2);
    }

    #[tokio::test]
    async fn test_memory_pool_acquire_waits() {
        let pool_capacity = 1;
        let pool = Arc::new(Mutex::new(MemoryPool::<Vec<f32>>::new(pool_capacity)));

        let header = PacketHeader { batch_size: 10, timestamp: 123 };
        let _pkt1 = MemoryPool::acquire(&pool, header).await.unwrap(); // Takes the only packet // Corrected call

        let pool_clone = Arc::clone(&pool);
        let acquire_task = tokio::spawn(async move {
            // This should wait until a packet is returned
            MemoryPool::acquire(&pool_clone, header).await // Corrected call
        });

        // Give the acquire_task a moment to start waiting
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(pool.lock().unwrap().len(), 0); // Still empty

        // Drop the first packet, which should unblock the acquire_task
        drop(_pkt1);

        // The acquire_task should now complete successfully
        let result = timeout(Duration::from_millis(100), acquire_task).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());
        assert_eq!(pool.lock().unwrap().len(), 0); // Packet acquired by the task
    }

    #[tokio::test]
    async fn test_packet_new_for_test() {
        let samples = vec![1.0, 2.0, 3.0];
        let pkt = Packet::new_for_test(samples.clone());
        assert_eq!(pkt.samples, samples);
        assert_eq!(pkt.header.batch_size, 0);
        assert_eq!(pkt.header.timestamp, 0);
        // Ensure it doesn't panic on drop without a real pool
        drop(pkt);
    }
}