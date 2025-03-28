pub mod mock_impl {
    use super::super::hal::{SpiPort, InterruptPin, Edge};
    use std::io;
    use std::time::Duration;
    use log::debug;

    /// Mock implementation of SPI port
    pub struct MockSpi;

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
    }

    impl MockSpi {
        /// Create a new MockSpi instance
        pub fn new() -> Self {
            MockSpi
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