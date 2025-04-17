use rppal::spi::{Spi, Bus, SlaveSelect, Mode};
use rppal::gpio::{Gpio, InputPin, Trigger, Event};
use std::io::Error;
use log::{info, error};

// SPI abstraction trait
pub trait SpiDevice: Send {
    fn write(&mut self, data: &[u8]) -> Result<(), Error>;
    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Error>;
}

// GPIO abstraction trait
pub trait InputPinDevice: Send {
    fn set_interrupt(&mut self, trigger: Trigger, timeout: Option<std::time::Duration>) -> Result<(), Error>;
    fn poll_interrupt(&mut self, clear: bool, timeout: Option<std::time::Duration>) -> Result<Option<Event>, Error>;
    fn clear_interrupt(&mut self) -> Result<(), Error>;
}

// Implement SpiDevice for rppal::spi::Spi
impl SpiDevice for Spi {
    fn write(&mut self, data: &[u8]) -> Result<(), Error> {
        Spi::write(self, data)
    }
    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Error> {
        Spi::transfer(self, read, write)
    }
}

// Implement InputPinDevice for rppal::gpio::InputPin
impl InputPinDevice for InputPin {
    fn set_interrupt(&mut self, trigger: Trigger, timeout: Option<std::time::Duration>) -> Result<(), Error> {
        InputPin::set_interrupt(self, trigger, timeout)
    }
    fn poll_interrupt(&mut self, clear: bool, timeout: Option<std::time::Duration>) -> Result<Option<Event>, Error> {
        InputPin::poll_interrupt(self, clear, timeout)
    }
    fn clear_interrupt(&mut self) -> Result<(), Error> {
        InputPin::clear_interrupt(self)
    }
}

/// Initialize SPI communication with the ADS1299.
pub fn init_spi() -> Result<Box<dyn SpiDevice>, crate::board_drivers::types::DriverError> {
    let spi_speed = 500_000; // 500kHz - confirmed working with Python script
    info!("Initializing SPI with speed: {} Hz, Mode: Mode1 (CPOL=0, CPHA=1)", spi_speed);

    match Spi::new(
        Bus::Spi0,
        SlaveSelect::Ss0,
        spi_speed,
        Mode::Mode1,  // CPOL=0, CPHA=1 for ADS1299
    ) {
        Ok(spi) => {
            info!("SPI initialization successful");
            Ok(Box::new(spi))
        },
        Err(e) => {
            error!("SPI initialization error: {}", e);
            error!("This could be because the SPI device is not available or the user doesn't have permission to access it.");
            error!("Make sure the SPI interface is enabled and the user has permission to access it.");

            Err(crate::board_drivers::types::DriverError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("SPI initialization error: {}", e)
            )))
        }
    }
}

/// Initialize the DRDY pin for detecting when new data is available.
pub fn init_drdy_pin() -> Result<Box<dyn InputPinDevice>, crate::board_drivers::types::DriverError> {
    info!("Initializing GPIO for DRDY pin (GPIO25)");

    match Gpio::new() {
        Ok(gpio) => {
            info!("GPIO initialization successful");

            // GPIO25 (Pin 22) is used for DRDY
            match gpio.get(25) {
                Ok(pin) => {
                    info!("GPIO pin 25 acquired successfully");
                    Ok(Box::new(pin.into_input_pullup()))
                },
                Err(e) => {
                    error!("GPIO pin 25 error: {}", e);
                    error!("This could be because the GPIO pin is already in use or the user doesn't have permission to access it.");
                    error!("Make sure the GPIO interface is enabled and the user has permission to access it.");

                    Err(crate::board_drivers::types::DriverError::IoError(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("GPIO pin error: {}", e)
                    )))
                }
            }
        },
        Err(e) => {
            error!("GPIO initialization error: {}", e);
            error!("This could be because the GPIO interface is not available or the user doesn't have permission to access it.");
            error!("Make sure the GPIO interface is enabled and the user has permission to access it.");

            Err(crate::board_drivers::types::DriverError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("GPIO initialization error: {}", e)
            )))
        }
    }
}

// Helper function to send a command to SPI
pub fn send_command_to_spi(spi: &mut dyn SpiDevice, command: u8) -> Result<(), crate::board_drivers::types::DriverError> {
    let buffer = [command];
    spi.write(&buffer).map_err(|e| crate::board_drivers::types::DriverError::IoError(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("SPI write error: {}", e)
    )))?;
    Ok(())
}

// Helper function to write a value to a register in the ADS1299
pub fn write_register(spi: &mut dyn SpiDevice, register: u8, value: u8) -> Result<(), crate::board_drivers::types::DriverError> {
    // Command: WREG (0x40) + register address
    let command = 0x40 | (register & 0x1F);

    // First byte: command, second byte: number of registers to write minus 1 (0 for single register)
    // Third byte: value to write
    let write_buffer = [command, 0x00, value];

    spi.write(&write_buffer).map_err(|e| crate::board_drivers::types::DriverError::IoError(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("SPI write error: {}", e)
    )))?;

    Ok(())
}