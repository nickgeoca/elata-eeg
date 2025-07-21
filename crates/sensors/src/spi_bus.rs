//! A thread-safe SPI bus abstraction for managing multiple devices on a single bus.

use rppal::gpio::OutputPin;
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use std::sync::{Arc, Mutex};

use crate::DriverError;

/// A thread-safe wrapper around an SPI bus that handles chip-select logic.
/// This ensures that only one device is active on the bus at any given time.
#[derive(Clone)]
pub struct SpiBus {
    // The Spi device is wrapped in a Mutex to allow safe sharing across threads.
    // Made public to allow low-level access for debugging.
    pub spi: Arc<Mutex<Spi>>,
}

impl SpiBus {
    /// Creates a new `SpiBus`.
    ///
    /// IMPORTANT: We use a dummy `SlaveSelect::Ss0` because `SlaveSelect::None`
    /// is not available or working as expected in this environment. The physical
    /// SS0/CE0 pin (GPIO 8) MUST be left unconnected or tied high to prevent it
    /// from interfering with the manual chip select logic.
    pub fn new(bus: Bus, clock_speed: u32, mode: Mode) -> Result<Self, DriverError> {
        let spi = Spi::new(bus, SlaveSelect::Ss0, clock_speed, mode)
            .map_err(|e| DriverError::SpiError(e.to_string()))?;

        Ok(Self {
            spi: Arc::new(Mutex::new(spi)),
        })
    }

    /// Performs a read/write transfer on the SPI bus.
    ///
    /// This method locks the bus, asserts the given chip-select pin,
    /// performs the transfer, and then de-asserts the pin.
    pub fn transfer(
        &self,
        cs_pin: &mut OutputPin,
        buffer: &mut [u8],
    ) -> Result<(), DriverError> {
        let spi = self.spi.lock().unwrap();
        // Create a separate, owned buffer for writing to avoid the E0502 borrow error.
        let write_buffer = buffer.to_vec();

        cs_pin.set_low();
        // The `transfer` method writes from `write_buffer` and reads into `buffer`.
        let result = spi.transfer(buffer, &write_buffer);
        cs_pin.set_high();

        // Map the Result<usize> to Result<()>
        result.map(|_| ()).map_err(|e| DriverError::SpiError(e.to_string()))
    }

    /// Performs a write-only transfer on the SPI bus.
    pub fn write(
        &self,
        cs_pin: &mut OutputPin,
        buffer: &[u8],
    ) -> Result<(), DriverError> {
        let mut spi = self.spi.lock().unwrap();

        cs_pin.set_low();
        let result = spi
            .write(buffer)
            .map_err(|e| DriverError::SpiError(e.to_string()))
            .map(|_| ()); // Discard the usize on success
        cs_pin.set_high();

        result
    }
}