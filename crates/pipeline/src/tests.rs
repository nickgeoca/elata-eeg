//! Rigorous testing for core data structures
//!
//! This module contains comprehensive tests for the memory pool and packet system,
//! including concurrency tests with loom and undefined behavior detection with miri.

#[cfg(test)]
mod memory_pool_tests {
    use super::super::data::{MemoryPool, PacketHeader, VoltageEegPacket};
    use std::sync::{Arc, Mutex};
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_memory_pool_basic_operations() {
        let pool = Arc::new(Mutex::new(MemoryPool::<VoltageEegPacket>::new(4)));
        
        // Test initial state
        assert_eq!(pool.lock().unwrap().len(), 4);
        assert_eq!(pool.lock().unwrap().capacity(), 4);
        assert!(!pool.lock().unwrap().is_empty());
        assert!(pool.lock().unwrap().is_full());

        // Test acquire
        let header = PacketHeader { batch_size: 10, timestamp: 123 };
        let packet = MemoryPool::acquire(&pool, header).await.unwrap();
        assert_eq!(pool.lock().unwrap().len(), 3);
        assert_eq!(packet.header.batch_size, 10);
        assert_eq!(packet.header.timestamp, 123);

        // Test automatic return on drop
        drop(packet);
        // Give some time for the drop to complete
        tokio::task::yield_now().await;
        assert_eq!(pool.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn test_memory_pool_try_acquire() {
        let pool = Arc::new(Mutex::new(MemoryPool::<VoltageEegPacket>::new(2)));
        let header = PacketHeader { batch_size: 5, timestamp: 456 };

        // Acquire all packets
        let _packet1 = MemoryPool::try_acquire(&pool, header).unwrap();
        let _packet2 = MemoryPool::try_acquire(&pool, header).unwrap();
        
        // Pool should be empty now
        assert!(pool.lock().unwrap().is_empty());
        
        // Try to acquire when empty should return None
        let packet3 = MemoryPool::try_acquire(&pool, header);
        assert!(packet3.is_none());
    }

    #[tokio::test]
    async fn test_memory_pool_acquire_waits() {
        let pool = Arc::new(Mutex::new(MemoryPool::<VoltageEegPacket>::new(1)));
        let header = PacketHeader { batch_size: 1, timestamp: 789 };

        // Acquire the only packet
        let packet = MemoryPool::try_acquire(&pool, header).unwrap();
        assert!(pool.lock().unwrap().is_empty());

        // Start an async acquire that should wait
        let pool_clone = pool.clone();
        let acquire_task = tokio::spawn(async move {
            MemoryPool::acquire(&pool_clone, header).await
        });

        // Give the task a moment to start waiting
        tokio::task::yield_now().await;

        // Drop the packet to return it to the pool
        drop(packet);

        // The waiting acquire should now complete
        let result = timeout(Duration::from_millis(100), acquire_task).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());
    }

    #[tokio::test]
    async fn test_packet_header_preservation() {
        let pool = Arc::new(Mutex::new(MemoryPool::<VoltageEegPacket>::new(2)));
        
        let header1 = PacketHeader { batch_size: 16, timestamp: 1000 };
        let header2 = PacketHeader { batch_size: 32, timestamp: 2000 };

        let packet1 = MemoryPool::acquire(&pool, header1).await.unwrap();
        let packet2 = MemoryPool::acquire(&pool, header2).await.unwrap();

        assert_eq!(packet1.header.batch_size, 16);
        assert_eq!(packet1.header.timestamp, 1000);
        assert_eq!(packet2.header.batch_size, 32);
        assert_eq!(packet2.header.timestamp, 2000);
    }
}

#[cfg(test)]
#[cfg(loom)]
mod loom_tests {
    use loom::sync::{Arc, Mutex};
    use loom::thread;
    use super::super::data::{MemoryPool, PacketHeader, VoltageEegPacket};

    #[test]
    fn test_memory_pool_concurrent_access() {
        loom::model(|| {
            let pool = Arc::new(Mutex::new(MemoryPool::<VoltageEegPacket>::new(4)));
            let header = PacketHeader { batch_size: 8, timestamp: 123 };

            let pool1 = pool.clone();
            let pool2 = pool.clone();

            let t1 = thread::spawn(move || {
                // Try to acquire a packet
                if let Some(packet) = MemoryPool::try_acquire(&pool1, header) {
                    // Do some work with the packet
                    let _samples = &packet.samples;
                    // Packet is automatically returned on drop
                }
            });

            let t2 = thread::spawn(move || {
                // Try to acquire a packet
                if let Some(packet) = MemoryPool::try_acquire(&pool2, header) {
                    // Do some work with the packet
                    let _samples = &packet.samples;
                    // Packet is automatically returned on drop
                }
            });

            t1.join().unwrap();
            t2.join().unwrap();

            // Pool should have all packets back
            assert_eq!(pool.lock().unwrap().len(), 4);
        });
    }

    #[test]
    fn test_memory_pool_multiple_producers_consumers() {
        loom::model(|| {
            let pool = Arc::new(Mutex::new(MemoryPool::<VoltageEegPacket>::new(2)));
            let header = PacketHeader { batch_size: 4, timestamp: 456 };

            let handles: Vec<_> = (0..3).map(|_| {
                let pool_clone = pool.clone();
                thread::spawn(move || {
                    // Each thread tries to acquire and immediately release
                    if let Some(_packet) = MemoryPool::try_acquire(&pool_clone, header) {
                        // Packet is automatically returned on drop
                    }
                })
            }).collect();

            for handle in handles {
                handle.join().unwrap();
            }

            // All packets should be back in the pool
            assert_eq!(pool.lock().unwrap().len(), 2);
        });
    }
}

#[cfg(test)]
mod stress_tests {
    use super::super::data::{MemoryPool, PacketHeader, VoltageEegPacket};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_high_throughput_acquire_release() {
        let pool = Arc::new(Mutex::new(MemoryPool::<VoltageEegPacket>::new(16)));
        let header = PacketHeader { batch_size: 64, timestamp: 0 };

        // Simulate high-throughput packet processing
        for i in 0..1000 {
            let mut header = header;
            header.timestamp = i;
            
            let packet = MemoryPool::acquire(&pool, header).await.unwrap();
            assert_eq!(packet.header.timestamp, i);
            
            // Simulate some processing time
            if i % 100 == 0 {
                tokio::task::yield_now().await;
            }
            
            // Packet is automatically returned on drop
        }

        // All packets should be back
        assert_eq!(pool.lock().unwrap().len(), 16);
    }

    #[tokio::test]
    async fn test_concurrent_producers_consumers() {
        let pool = Arc::new(Mutex::new(MemoryPool::<VoltageEegPacket>::new(8)));
        let header = PacketHeader { batch_size: 32, timestamp: 0 };

        let mut handles = Vec::new();

        // Spawn multiple producer/consumer tasks
        for task_id in 0..4 {
            let pool_clone = pool.clone();
            let handle = tokio::spawn(async move {
                for i in 0..100 {
                    let mut header = header;
                    header.timestamp = (task_id * 100 + i) as u64;
                    
                    // Try to acquire with timeout to avoid infinite waiting
                    let result = timeout(
                        Duration::from_millis(100),
                        MemoryPool::acquire(&pool_clone, header)
                    ).await;
                    
                    if let Ok(Ok(packet)) = result {
                        // Simulate processing
                        assert_eq!(packet.header.timestamp, (task_id * 100 + i) as u64);
                        
                        // Occasionally yield to other tasks
                        if i % 10 == 0 {
                            tokio::task::yield_now().await;
                        }
                        
                        // Packet is automatically returned on drop
                    }
                }
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // All packets should be back in the pool
        assert_eq!(pool.lock().unwrap().len(), 8);
    }

    #[tokio::test]
    async fn test_pool_exhaustion_recovery() {
        let pool = Arc::new(Mutex::new(MemoryPool::<VoltageEegPacket>::new(2)));
        let header = PacketHeader { batch_size: 16, timestamp: 123 };

        // Exhaust the pool
        let packet1 = MemoryPool::try_acquire(&pool, header).unwrap();
        let packet2 = MemoryPool::try_acquire(&pool, header).unwrap();
        assert!(pool.lock().unwrap().is_empty());

        // Verify no more packets can be acquired
        assert!(MemoryPool::try_acquire(&pool, header).is_none());

        // Return one packet
        drop(packet1);
        tokio::task::yield_now().await;

        // Should be able to acquire again
        let packet3 = MemoryPool::try_acquire(&pool, header).unwrap();
        assert!(packet3.header.batch_size == 16);

        // Clean up
        drop(packet2);
        drop(packet3);
        tokio::task::yield_now().await;
        assert_eq!(pool.lock().unwrap().len(), 2);
    }
}

#[cfg(test)]
mod safety_tests {
    use super::super::data::{MemoryPool, PacketHeader, VoltageEegPacket, TakeForDrop};

    #[test]
    fn test_take_for_drop_implementation() {
        let mut packet = VoltageEegPacket {
            samples: vec![1.0, 2.0, 3.0, 4.0],
        };

        // Test that take_for_drop clears but preserves capacity
        let original_capacity = packet.samples.capacity();
        packet = packet.take_for_drop();
        
        assert_eq!(packet.samples.len(), 0);
        assert_eq!(packet.samples.capacity(), original_capacity);
    }

    #[test]
    fn test_memory_pool_invariants() {
        let pool = MemoryPool::<VoltageEegPacket>::new(5);
        
        // Test initial invariants
        assert_eq!(pool.len(), 5);
        assert_eq!(pool.capacity(), 5);
        assert!(!pool.is_empty());
        assert!(pool.is_full());
        
        // Test that capacity is immutable
        assert_eq!(pool.capacity(), 5);
    }
}