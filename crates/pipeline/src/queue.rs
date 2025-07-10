//! Queue implementations for inter-stage communication
//!
//! This module provides a unified interface over different queue backends
//! (rtrb, crossbeam) for lock-free, bounded communication between pipeline stages.

use async_trait::async_trait;
use std::fmt::Debug;
use tokio::sync::mpsc::error::TrySendError;
use tracing::{debug, warn};

use crate::data::{Packet, AnyPacketType};
use crate::error::StageError;
use crate::stage::{Input, Output};

/// Configuration for queue creation
#[derive(Debug, Clone)]
pub struct QueueConfig {
    /// Queue capacity (number of packets)
    pub capacity: usize,
    /// Queue backend to use
    pub backend: QueueBackend,
}

/// Available queue backends
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueBackend {
    /// Use crossbeam ArrayQueue (lock-free, bounded)
    Crossbeam,
    /// Use rtrb (real-time ring buffer)
    #[cfg(feature = "rtrb")]
    Rtrb,
    /// Use tokio mpsc (for testing/fallback)
    TokioMpsc,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            capacity: 64,
            backend: QueueBackend::Crossbeam,
        }
    }
}

/// A unified queue interface that wraps different backend implementations
pub struct StageQueue<T: AnyPacketType> {
    inner: Box<dyn StageQueueInner<T>>,
}

/// Internal trait for queue implementations
trait StageQueueInner<T: AnyPacketType>: Send + Sync {
    fn try_send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>>;
    fn try_recv(&mut self) -> Result<Option<Packet<T>>, StageError>;
    fn capacity(&self) -> usize;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn is_full(&self) -> bool;
}

impl<T: AnyPacketType> StageQueue<T> {
    /// Create a new queue with the specified configuration
    pub fn new(config: QueueConfig) -> (StageQueueSender<T>, StageQueueReceiver<T>) {
        match config.backend {
            QueueBackend::Crossbeam => {
                let (sender, receiver) = CrossbeamQueue::new(config.capacity);
                (
                    StageQueueSender { inner: Box::new(sender) },
                    StageQueueReceiver { inner: Box::new(receiver) },
                )
            }
            #[cfg(feature = "rtrb")]
            QueueBackend::Rtrb => {
                let (sender, receiver) = RtrbQueue::new(config.capacity);
                (
                    StageQueueSender { inner: Box::new(sender) },
                    StageQueueReceiver { inner: Box::new(receiver) },
                )
            }
            QueueBackend::TokioMpsc => {
                let (sender, receiver) = TokioMpscQueue::new(config.capacity);
                (
                    StageQueueSender { inner: Box::new(sender) },
                    StageQueueReceiver { inner: Box::new(receiver) },
                )
            }
        }
    }
}

/// Sending half of a stage queue
pub struct StageQueueSender<T: AnyPacketType> {
    inner: Box<dyn StageQueueSenderInner<T>>,
}

/// Receiving half of a stage queue
pub struct StageQueueReceiver<T: AnyPacketType> {
    inner: Box<dyn StageQueueReceiverInner<T>>,
}

trait StageQueueSenderInner<T: AnyPacketType>: Send + Sync {
    fn try_send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>>;
    fn capacity(&self) -> usize;
    fn len(&self) -> usize;
    fn is_full(&self) -> bool;
}

trait StageQueueReceiverInner<T: AnyPacketType>: Send + Sync {
    fn try_recv(&mut self) -> Result<Option<Packet<T>>, StageError>;
    fn capacity(&self) -> usize;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
}

#[async_trait]
impl<T: AnyPacketType> Output<T> for StageQueueSender<T> {
    async fn send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>> {
        // For now, just use try_send. In a real implementation, we might want to
        // implement backpressure handling here.
        self.try_send(packet)
    }

    fn try_send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>> {
        self.inner.try_send(packet)
    }
}

#[async_trait]
impl<T: AnyPacketType> Input<T> for StageQueueReceiver<T> {
    async fn recv(&mut self) -> Result<Option<Packet<T>>, StageError> {
        // For now, just use try_recv. In a real implementation, we might want to
        // implement async waiting here.
        self.try_recv()
    }

    fn try_recv(&mut self) -> Result<Option<Packet<T>>, StageError> {
        self.inner.try_recv()
    }
}

// --- Crossbeam Implementation ---

use crossbeam_queue::ArrayQueue;
use std::sync::Arc;

struct CrossbeamQueueSender<T: AnyPacketType> {
    queue: Arc<ArrayQueue<Packet<T>>>,
}

struct CrossbeamQueueReceiver<T: AnyPacketType> {
    queue: Arc<ArrayQueue<Packet<T>>>,
}

impl<T: AnyPacketType> CrossbeamQueue<T> {
    fn new(capacity: usize) -> (CrossbeamQueueSender<T>, CrossbeamQueueReceiver<T>) {
        let queue = Arc::new(ArrayQueue::new(capacity));
        (
            CrossbeamQueueSender { queue: queue.clone() },
            CrossbeamQueueReceiver { queue },
        )
    }
}

struct CrossbeamQueue<T: AnyPacketType> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T: AnyPacketType> StageQueueSenderInner<T> for CrossbeamQueueSender<T> {
    fn try_send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>> {
        match self.queue.push(packet) {
            Ok(()) => {
                debug!("Packet sent via crossbeam queue");
                Ok(())
            }
            Err(packet) => {
                warn!("Crossbeam queue is full, dropping packet");
                Err(TrySendError::Full(packet))
            }
        }
    }

    fn capacity(&self) -> usize {
        self.queue.capacity()
    }

    fn len(&self) -> usize {
        self.queue.len()
    }

    fn is_full(&self) -> bool {
        self.queue.is_full()
    }
}

impl<T: AnyPacketType> StageQueueReceiverInner<T> for CrossbeamQueueReceiver<T> {
    fn try_recv(&mut self) -> Result<Option<Packet<T>>, StageError> {
        match self.queue.pop() {
            Some(packet) => {
                debug!("Packet received via crossbeam queue");
                Ok(Some(packet))
            }
            None => Ok(None),
        }
    }

    fn capacity(&self) -> usize {
        self.queue.capacity()
    }

    fn len(&self) -> usize {
        self.queue.len()
    }

    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

// --- Tokio MPSC Implementation (for testing/fallback) ---

use tokio::sync::mpsc;

struct TokioMpscQueueSender<T: AnyPacketType> {
    sender: mpsc::Sender<Packet<T>>,
    capacity: usize,
}

struct TokioMpscQueueReceiver<T: AnyPacketType> {
    receiver: mpsc::Receiver<Packet<T>>,
    capacity: usize,
}

impl<T: AnyPacketType> TokioMpscQueue<T> {
    fn new(capacity: usize) -> (TokioMpscQueueSender<T>, TokioMpscQueueReceiver<T>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (
            TokioMpscQueueSender { sender, capacity },
            TokioMpscQueueReceiver { receiver, capacity },
        )
    }
}

struct TokioMpscQueue<T: AnyPacketType> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T: AnyPacketType> StageQueueSenderInner<T> for TokioMpscQueueSender<T> {
    fn try_send(&mut self, packet: Packet<T>) -> Result<(), TrySendError<Packet<T>>> {
        self.sender.try_send(packet)
    }

    fn capacity(&self) -> usize {
        self.capacity
    }

    fn len(&self) -> usize {
        // Tokio mpsc doesn't expose len(), so we approximate
        if self.sender.is_closed() {
            0
        } else {
            // We can't get the exact length, so return 0 as a safe approximation
            0
        }
    }

    fn is_full(&self) -> bool {
        // Tokio mpsc doesn't expose is_full(), so we approximate
        false
    }
}

impl<T: AnyPacketType> StageQueueReceiverInner<T> for TokioMpscQueueReceiver<T> {
    fn try_recv(&mut self) -> Result<Option<Packet<T>>, StageError> {
        match self.receiver.try_recv() {
            Ok(packet) => Ok(Some(packet)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(StageError::QueueClosed),
        }
    }

    fn capacity(&self) -> usize {
        self.capacity
    }

    fn len(&self) -> usize {
        // Tokio mpsc doesn't expose len(), so we approximate
        0
    }

    fn is_empty(&self) -> bool {
        // We can't determine this exactly with tokio mpsc
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{PacketHeader, VoltageEegPacket};

    #[tokio::test]
    async fn test_crossbeam_queue() {
        let config = QueueConfig {
            capacity: 4,
            backend: QueueBackend::Crossbeam,
        };
        
        let (mut sender, mut receiver) = StageQueue::<VoltageEegPacket>::new(config);
        
        // Test sending and receiving
        let header = PacketHeader { batch_size: 10, timestamp: 123 };
        let packet = crate::data::Packet::new_for_test(VoltageEegPacket::default());
        
        sender.try_send(packet).unwrap();
        let received = receiver.try_recv().unwrap().unwrap();
        assert_eq!(received.samples.samples.len(), 0); // Default VoltageEegPacket has empty samples
    }

    #[tokio::test]
    async fn test_tokio_mpsc_queue() {
        let config = QueueConfig {
            capacity: 4,
            backend: QueueBackend::TokioMpsc,
        };
        
        let (mut sender, mut receiver) = StageQueue::<VoltageEegPacket>::new(config);
        
        // Test sending and receiving
        let packet = crate::data::Packet::new_for_test(VoltageEegPacket::default());
        
        sender.try_send(packet).unwrap();
        let received = receiver.try_recv().unwrap().unwrap();
        assert_eq!(received.samples.samples.len(), 0); // Default VoltageEegPacket has empty samples
    }
}