#[cfg(feature = "pi-hardware")]
pub mod rppal_impl {
    use super::super::hal::{SpiPort, InterruptPin, Edge};
    use rppal::spi::{Spi, Bus, SlaveSelect, Mode};
    use rppal::gpio::{Gpio, InputPin, Trigger, Event};
    use std::io;
    use std::time::Duration;
    use log::{debug, error};

    /// SPI implementation using rppal
    pub struct RppalSpi(Spi);

    impl SpiPort for RppalSpi {
        fn write(&mut self, buffer: &[u8]) -> Result<(), io::Error> {
            self.0.write(buffer)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }

        fn transfer(&mut self, read_buffer: &mut [u8], write_buffer: &[u8]) -> Result<(), io::Error> {
            self.0.transfer(read_buffer, write_buffer)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }
    }

    impl RppalSpi {
        /// Create a new RppalSpi instance
        pub fn new() -> Result<Self, io::Error> {
            // SPI0, CS0, 1MHz, mode 1
            Spi::new(Bus::Spi0, SlaveSelect::Ss0, 1_000_000, Mode::Mode1)
                .map(RppalSpi)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
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