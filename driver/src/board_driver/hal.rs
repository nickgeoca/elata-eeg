use std::io;
use std::time::Duration;

/// Trait for SPI port operations
pub trait SpiPort: Send + Sync + 'static {
    /// Write data to the SPI port
    fn write(&mut self, buffer: &[u8]) -> Result<(), io::Error>;
    
    /// Transfer data over SPI (simultaneous read/write)
    fn transfer(&mut self, read_buffer: &mut [u8], write_buffer: &[u8]) -> Result<(), io::Error>;
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