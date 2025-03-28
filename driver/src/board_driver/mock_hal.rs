pub mod mock_impl {
    use super::super::hal::{SpiPort, DmaSpiPort, InterruptPin, Edge};
    use std::io;
    use std::time::Duration;
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    use std::thread;
    use log::{debug, info, error};

    /// Mock implementation of SPI port with DMA support
    pub struct MockSpi {
        dma_thread: Option<thread::JoinHandle<()>>,
        dma_running: Arc<AtomicBool>,
    }

    impl SpiPort for MockSpi {
        fn write(&mut self, _buffer: &[u8]) -> Result<(), io::Error> {
            // Simulate successful write
            Ok(())
        }

        fn transfer(&mut self, read_buffer: &mut [u8], _write_buffer: &[u8]) -> Result<(), io::Error> {
            // Fill read buffer with dummy data (zeros)
            read_buffer.fill(0);
            Ok(())
        }
        
        fn supports_dma(&self) -> bool {
            // Mock implementation supports DMA
            true
        }
        
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }
    
    impl DmaSpiPort for MockSpi {
        fn start_dma_transfer<F>(&mut self,
                                buffer: &mut [u8],
                                sample_size: usize,
                                batch_size: usize,
                                mut callback: F) -> Result<(), io::Error>
            where F: FnMut(&[u8], usize) + Send + 'static {
            
            if self.dma_thread.is_some() {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "DMA transfer already in progress"));
            }
            
            // Create a shared buffer for DMA
            let buffer_size = sample_size * batch_size;
            if buffer.len() < buffer_size {
                return Err(io::Error::new(io::ErrorKind::InvalidInput,
                    format!("Buffer too small: {} bytes, need at least {} bytes", buffer.len(), buffer_size)));
            }
            
            // Create a shared buffer that can be accessed from the DMA thread
            let shared_buffer = Arc::new(buffer.to_vec());
            
            // Set the DMA running flag
            self.dma_running.store(true, Ordering::SeqCst);
            let dma_running = self.dma_running.clone();
            
            // Create a thread for simulating DMA transfer
            let _buffer_clone = shared_buffer.clone();
            let dma_thread = thread::spawn(move || {
                let mut sample_count = 0;
                let mut batch_count = 0;
                
                info!("Mock DMA thread started, simulating samples");
                
                while dma_running.load(Ordering::SeqCst) {
                    // Simulate data acquisition at a reasonable rate
                    thread::sleep(Duration::from_millis(10));
                    
                    // Generate a sample of random data
                    let mut sample_buffer = vec![0u8; sample_size];
                    for i in 0..sample_size {
                        sample_buffer[i] = (i % 256) as u8; // Simple pattern for testing
                    }
                    
                    // Call the callback with the buffer
                    callback(&sample_buffer, 1);
                    
                    sample_count += 1;
                    
                    // If we've collected a full batch, log it
                    if sample_count % batch_size == 0 {
                        batch_count += 1;
                        debug!("Mock DMA batch {} complete ({} samples)", batch_count, batch_size);
                    }
                }
                
                info!("Mock DMA thread terminated");
            });
            
            self.dma_thread = Some(dma_thread);
            info!("Mock DMA transfer started");
            
            Ok(())
        }
        
        fn stop_dma_transfer(&mut self) -> Result<(), io::Error> {
            // Signal the DMA thread to stop
            self.dma_running.store(false, Ordering::SeqCst);
            
            // Wait for the DMA thread to complete
            if let Some(handle) = self.dma_thread.take() {
                match handle.join() {
                    Ok(_) => info!("Mock DMA thread completed successfully"),
                    Err(e) => error!("Mock DMA thread terminated with error: {:?}", e),
                }
            }
            
            Ok(())
        }
    }

    impl MockSpi {
        /// Create a new MockSpi instance
        pub fn new() -> Self {
            MockSpi {
                dma_thread: None,
                dma_running: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    /// Mock implementation of interrupt pin
    pub struct MockDrdyPin {
        // We'll use this to simulate occasional interrupts
        interrupt_counter: usize,
    }

    impl InterruptPin for MockDrdyPin {
        fn is_high(&self) -> bool {
            // Always return high when not in an interrupt state
            true
        }

        fn poll_interrupt(&mut self, _block: bool, _timeout: Option<Duration>) -> Result<Option<()>, io::Error> {
            // Simulate an interrupt every 10 calls
            self.interrupt_counter += 1;
            if self.interrupt_counter % 10 == 0 {
                Ok(Some(()))
            } else {
                Ok(None)
            }
        }

        fn clear_interrupt(&mut self) -> Result<(), io::Error> {
            // Nothing to do for mock
            Ok(())
        }

        fn set_interrupt_edge(&mut self, _edge: Edge) -> Result<(), io::Error> {
            // Nothing to do for mock
            Ok(())
        }
    }

    impl MockDrdyPin {
        /// Create a new MockDrdyPin instance
        pub fn new() -> Self {
            MockDrdyPin {
                interrupt_counter: 0,
            }
        }
    }

    /// Helper function to create mock SPI implementation
    pub fn create_spi() -> MockSpi {
        debug!("Creating mock SPI implementation");
        MockSpi::new()
    }

    /// Helper function to create mock DRDY pin implementation
    pub fn create_drdy() -> MockDrdyPin {
        debug!("Creating mock DRDY pin implementation");
        MockDrdyPin::new()
    }
}