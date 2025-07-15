use rppal::spi::{Spi, Bus, SlaveSelect, Mode};
use rppal::gpio::{Gpio, InputPin, Trigger, Event};
use std::io::Error as IoError;
use log::{info, error};

// SPI abstraction trait
pub trait SpiDevice: Send {
    fn write(&mut self, data: &[u8]) -> Result<(), IoError>;
    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), IoError>;
}

// GPIO abstraction trait
pub trait InputPinDevice: Send {
    fn is_high(&self) -> bool;
    fn set_interrupt(&mut self, trigger: Trigger, timeout: Option<std::time::Duration>) -> Result<(), IoError>;
    fn poll_interrupt(&mut self, clear: bool, timeout: Option<std::time::Duration>) -> Result<Option<Event>, IoError>;
    fn clear_interrupt(&mut self) -> Result<(), IoError>;
}

// Implement SpiDevice for rppal::spi::Spi
impl SpiDevice for Spi {
    fn write(&mut self, data: &[u8]) -> Result<(), IoError> {
        Spi::write(self, data)
            .map(|_| ())
            .map_err(|e| IoError::new(std::io::ErrorKind::Other, e.to_string()))
    }
    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), IoError> {
        Spi::transfer(self, read, write)
            .map(|_| ())
            .map_err(|e| IoError::new(std::io::ErrorKind::Other, e.to_string()))
    }
}

// Implement InputPinDevice for rppal::gpio::InputPin
// Implement SpiDevice for Box<dyn SpiDevice>
impl<T: SpiDevice + ?Sized> SpiDevice for Box<T> {
    fn write(&mut self, data: &[u8]) -> Result<(), IoError> {
        (**self).write(data)
    }
    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), IoError> {
        (**self).transfer(read, write)
    }
}

// Implement InputPinDevice for Box<dyn InputPinDevice>
impl<T: InputPinDevice + ?Sized> InputPinDevice for Box<T> {
    fn is_high(&self) -> bool {
        (**self).is_high()
    }
    fn set_interrupt(&mut self, trigger: Trigger, timeout: Option<std::time::Duration>) -> Result<(), IoError> {
        (**self).set_interrupt(trigger, timeout)
    }
    fn poll_interrupt(&mut self, clear: bool, timeout: Option<std::time::Duration>) -> Result<Option<Event>, IoError> {
        (**self).poll_interrupt(clear, timeout)
    }
    fn clear_interrupt(&mut self) -> Result<(), IoError> {
        (**self).clear_interrupt()
    }
}

impl InputPinDevice for InputPin {
    fn is_high(&self) -> bool {
        InputPin::is_high(self)
    }
    fn set_interrupt(&mut self, trigger: Trigger, timeout: Option<std::time::Duration>) -> Result<(), IoError> {
        InputPin::set_interrupt(self, trigger, timeout)
            .map_err(|e| IoError::new(std::io::ErrorKind::Other, e.to_string()))
    }
    fn poll_interrupt(&mut self, clear: bool, timeout: Option<std::time::Duration>) -> Result<Option<Event>, IoError> {
        InputPin::poll_interrupt(self, clear, timeout)
            .map_err(|e| IoError::new(std::io::ErrorKind::Other, e.to_string()))
    }
    fn clear_interrupt(&mut self) -> Result<(), IoError> {
        InputPin::clear_interrupt(self)
            .map_err(|e| IoError::new(std::io::ErrorKind::Other, e.to_string()))
    }
}

/// Initialize SPI communication with the ADS1299.
pub fn init_spi() -> Result<Box<dyn SpiDevice>, crate::types::DriverError> {
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

            Err(crate::types::DriverError::SpiError(format!("SPI initialization error: {}", e)))
        }
    }
}

/// Initialize the DRDY pin for detecting when new data is available.
pub fn init_drdy_pin() -> Result<Box<dyn InputPinDevice>, crate::types::DriverError> {
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

                    Err(crate::types::DriverError::GpioError(format!("GPIO pin error: {}", e)))
                }
            }
        },
        Err(e) => {
            error!("GPIO initialization error: {}", e);
            error!("This could be because the GPIO interface is not available or the user doesn't have permission to access it.");
            error!("Make sure the GPIO interface is enabled and the user has permission to access it.");

            Err(crate::types::DriverError::GpioError(format!("GPIO initialization error: {}", e)))
        }
    }
}

// Helper function to send a command to SPI
pub fn send_command_to_spi<T: SpiDevice + ?Sized>(spi: &mut T, command: u8) -> Result<(), crate::types::DriverError> {
    let buffer = [command];
    spi.write(&buffer).map_err(|e| crate::types::DriverError::SpiError(format!("SPI write error: {}", e)))?;
    Ok(())
}

// Helper function to write a value to a register in the ADS1299
pub fn write_register<T: SpiDevice + ?Sized>(spi: &mut T, register: u8, value: u8) -> Result<(), crate::types::DriverError> {
    // Command: WREG (0x40) + register address
    let command = 0x40 | (register & 0x1F);

    // First byte: command, second byte: number of registers to write minus 1 (0 for single register)
    // Third byte: value to write
    let write_buffer = [command, 0x00, value];

    spi.write(&write_buffer).map_err(|e| crate::types::DriverError::SpiError(format!("SPI write error: {}", e)))?;

    Ok(())
}
/// Waits for a falling edge interrupt on the DRDY pin with a specified timeout.
///
/// This function is a synchronous, blocking call that waits for the DRDY signal
/// from the ADS1299, indicating that new data is ready.
///
/// # Returns
/// - `Ok(true)` if the interrupt was received within the timeout.
/// - `Ok(false)` if the timeout occurred.
/// - `Err(DriverError)` if there was a GPIO error.
pub fn wait_irq(
    pin: &mut dyn InputPinDevice,
    timeout: std::time::Duration,
) -> Result<bool, crate::types::DriverError> {
    match pin.poll_interrupt(true, Some(timeout)) {
        Ok(Some(event)) if event.trigger == Trigger::FallingEdge => Ok(true),
        Ok(Some(_)) => {
            // Unexpected trigger, but we'll treat it as a timeout for simplicity
            Ok(false)
        }
        Ok(None) => {
            // Timeout
            Ok(false)
        }
        Err(e) => {
            error!("DRDY pin poll_interrupt error: {}", e);
            Err(crate::types::DriverError::GpioError(e.to_string()))
        }
    }
}