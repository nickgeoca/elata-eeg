#[cfg(feature = "pi-hardware")]
pub mod rppal_impl {
    use super::super::hal::{SpiPort, DmaSpiPort, InterruptPin, Edge};
    use rppal::spi::{Spi, Bus, SlaveSelect, Mode};
    use rppal::gpio::{Gpio, InputPin, Trigger, Event};
    use std::io;
    use std::time::Duration;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::sync::atomic::{AtomicBool, Ordering};
    use log::{debug, error, info, warn};

    /// SPI implementation using rppal
    pub struct RppalSpi {
        spi: Spi,
        dma_thread: Option<thread::JoinHandle<()>>,
        dma_running: Arc<AtomicBool>,
    }

    impl SpiPort for RppalSpi {
        fn write(&mut self, buffer: &[u8]) -> Result<(), io::Error> {
            self.spi.write(buffer)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }

        fn transfer(&mut self, read_buffer: &mut [u8], write_buffer: &[u8]) -> Result<(), io::Error> {
            self.spi.transfer(read_buffer, write_buffer)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }
        
        fn supports_dma(&self) -> bool {
            // The Raspberry Pi's SPI hardware supports DMA
            true
        }
        
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }
    
    impl DmaSpiPort for RppalSpi {
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
            let shared_buffer = Arc::new(Mutex::new(Vec::from(buffer)));
            
            // Set up DRDY pin for DMA triggering
            let gpio = Gpio::new()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            
            // DRDY pin is connected to GPIO 17
            const DRDY_PIN: u8 = 17;
            
            let mut drdy_pin = gpio.get(DRDY_PIN)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
                .into_input();
            
            // Configure the pin for falling edge detection
            drdy_pin.set_interrupt(Trigger::FallingEdge, None)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            
            // Create a clone of the SPI instance for the DMA thread
            // Note: In a real implementation, we would use the BCM2835 DMA controller directly
            // through a C FFI or a dedicated Rust crate. This is a simplified version.
            let spi_clone = Spi::new(Bus::Spi0, SlaveSelect::Ss0, 1_000_000, Mode::Mode1)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            
            // Set the DMA running flag
            self.dma_running.store(true, Ordering::SeqCst);
            let dma_running = self.dma_running.clone();
            
            // Create a thread for handling the DMA transfer
            let buffer_clone = shared_buffer.clone();
            let dma_thread = thread::spawn(move || {
                let mut sample_count = 0;
                let mut batch_count = 0;
                
                // Create write buffer (all zeros for reading)
                let write_buffer = vec![0u8; sample_size];
                
                info!("DMA thread started, waiting for samples");
                
                while dma_running.load(Ordering::SeqCst) {
                    // Wait for DRDY pin to go low (data ready)
                    match drdy_pin.poll_interrupt(true, Some(Duration::from_secs(1))) {
                        Ok(Some(_)) => {
                            // DRDY pin went low, data is ready
                            // In a real DMA implementation, the hardware would handle this automatically
                            // Here we simulate it with a manual transfer
                            
                            // Lock the buffer to write the sample
                            if let Ok(mut buffer) = buffer_clone.lock() {
                                // Calculate the offset for this sample in the buffer
                                let offset = (sample_count % batch_size) * sample_size;
                                
                                // Read the sample into the buffer
                                let mut read_buffer = vec![0u8; sample_size];
                                if let Err(e) = spi_clone.transfer(&mut read_buffer, &write_buffer) {
                                    error!("SPI transfer error in DMA thread: {}", e);
                                    continue;
                                }
                                
                                // Copy the sample to the shared buffer
                                for i in 0..sample_size {
                                    if offset + i < buffer.len() {
                                        buffer[offset + i] = read_buffer[i];
                                    }
                                }
                                
                                sample_count += 1;
                                
                                // If we've collected a full batch, call the callback
                                if sample_count % batch_size == 0 {
                                    // Call the callback with the buffer
                                    callback(&buffer, batch_size);
                                    batch_count += 1;
                                    debug!("DMA batch {} complete ({} samples)", batch_count, batch_size);
                                }
                            } else {
                                error!("Failed to lock buffer in DMA thread");
                            }
                        },
                        Ok(None) => {
                            // Timeout occurred, no interrupt
                            debug!("DMA interrupt timeout - no data ready");
                        },
                        Err(e) => {
                            error!("Error polling for interrupt in DMA thread: {:?}", e);
                            // Sleep a bit to avoid tight loop on error
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
                
                info!("DMA thread terminated");
                
                // Clean up by disabling the interrupt
                if let Err(e) = drdy_pin.clear_interrupt() {
                    error!("Failed to clear interrupt in DMA thread: {:?}", e);
                }
            });
            
            self.dma_thread = Some(dma_thread);
            info!("DMA transfer started");
            
            Ok(())
        }
        
        fn stop_dma_transfer(&mut self) -> Result<(), io::Error> {
            // Signal the DMA thread to stop
            self.dma_running.store(false, Ordering::SeqCst);
            
            // Wait for the DMA thread to complete
            if let Some(handle) = self.dma_thread.take() {
                match handle.join() {
                    Ok(_) => info!("DMA thread completed successfully"),
                    Err(e) => warn!("DMA thread terminated with error: {:?}", e),
                }
            }
            
            Ok(())
        }
    }

    impl RppalSpi {
        /// Create a new RppalSpi instance
        pub fn new() -> Result<Self, io::Error> {
            // SPI0, CS0, 1MHz, mode 1
            let spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, 1_000_000, Mode::Mode1)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            
            Ok(RppalSpi {
                spi,
                dma_thread: None,
                dma_running: Arc::new(AtomicBool::new(false)),
            })
        }
    }

    /// DRDY pin implementation using rppal
    pub struct RppalDrdyPin(InputPin);

    impl InterruptPin for RppalDrdyPin {
        fn is_high(&self) -> bool {
            self.0.is_high()
        }

        fn poll_interrupt(&mut self, block: bool, timeout: Option<Duration>) -> Result<Option<()>, io::Error> {
            match self.0.poll_interrupt(block, timeout) {
                Ok(Some(_event)) => Ok(Some(())),
                Ok(None) => Ok(None),
                Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
            }
        }

        fn clear_interrupt(&mut self) -> Result<(), io::Error> {
            self.0.clear_interrupt()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }

        fn set_interrupt_edge(&mut self, edge: Edge) -> Result<(), io::Error> {
            let trigger = match edge {
                Edge::Falling => Trigger::FallingEdge,
                Edge::Rising => Trigger::RisingEdge,
                Edge::Both => Trigger::Both,
            };

            self.0.set_interrupt(trigger, None)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }
    }

    impl RppalDrdyPin {
        /// Create a new RppalDrdyPin instance
        pub fn new() -> Result<Self, io::Error> {
            // DRDY pin is connected to GPIO 17
            const DRDY_PIN: u8 = 17;

            let gpio = Gpio::new()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            let pin = gpio.get(DRDY_PIN)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
                .into_input();

            Ok(RppalDrdyPin(pin))
        }
    }

    /// Helper function to create SPI implementation
    pub fn create_spi() -> Result<RppalSpi, io::Error> {
        debug!("Creating hardware SPI implementation");
        RppalSpi::new()
    }

    /// Helper function to create DRDY pin implementation
    pub fn create_drdy() -> Result<RppalDrdyPin, io::Error> {
        debug!("Creating hardware DRDY pin implementation");
        RppalDrdyPin::new()
    }
}