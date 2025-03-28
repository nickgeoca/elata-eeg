use std::io;
use std::time::Duration;

/// Base trait for SPI port operations (object-safe)
pub trait SpiPort: Send + Sync + 'static {
    /// Write data to the SPI port
    fn write(&mut self, buffer: &[u8]) -> Result<(), io::Error>;
    
    /// Transfer data over SPI (simultaneous read/write)
    fn transfer(&mut self, read_buffer: &mut [u8], write_buffer: &[u8]) -> Result<(), io::Error>;
    
    /// Check if DMA is supported by this SPI implementation
    fn supports_dma(&self) -> bool {
        false // Default implementation returns false
    }
    
    /// Allow downcasting to concrete types
    fn as_any(&self) -> &dyn std::any::Any;
    
    /// Allow downcasting to concrete types (mutable)
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// Trait for DMA-capable SPI operations
/// This is separated from the base SpiPort trait to maintain object safety
pub trait DmaSpiPort: SpiPort {
    /// Start a DMA transfer for continuous data acquisition
    ///
    /// This method sets up a DMA transfer that will continuously read data from the SPI bus
    /// when triggered by the DRDY pin. The data will be written to the provided buffer.
    ///
    /// # Arguments
    /// * `buffer` - The buffer to write the data to. Must be large enough to hold multiple samples.
    /// * `sample_size` - The size of each sample in bytes (status bytes + channel data)
    /// * `batch_size` - The number of samples to collect before triggering the callback
    /// * `callback` - A callback function that will be called when a batch of samples is ready
    ///
    /// # Returns
    /// * `Ok(())` if the DMA transfer was started successfully
    /// * `Err(io::Error)` if the DMA transfer could not be started
    fn start_dma_transfer<F>(&mut self,
                            buffer: &mut [u8],
                            sample_size: usize,
                            batch_size: usize,
                            callback: F) -> Result<(), io::Error>
        where F: FnMut(&[u8], usize) + Send + 'static;
    
    /// Stop an ongoing DMA transfer
    fn stop_dma_transfer(&mut self) -> Result<(), io::Error>;
}

/// Edge trigger types for interrupt pins
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edge {
    Falling,
    Rising,
    Both,
}

/// Trait for interrupt pin operations
pub trait InterruptPin: Send + Sync + 'static {
    /// Check if the pin is in a high state
    fn is_high(&self) -> bool;
    
    /// Poll for an interrupt with optional blocking and timeout
    fn poll_interrupt(&mut self, block: bool, timeout: Option<Duration>) -> Result<Option<()>, io::Error>;
    
    /// Clear any pending interrupts
    fn clear_interrupt(&mut self) -> Result<(), io::Error>;
    
    /// Set the edge that triggers interrupts
    fn set_interrupt_edge(&mut self, edge: Edge) -> Result<(), io::Error>;
}