use rppal::spi::{Spi, Bus, SlaveSelect, Mode};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("Starting minimal ADS1299 test...");
    
    // Initialize SPI with the same settings as the working Python script
    let mut spi = Spi::new(
        Bus::Spi0,
        SlaveSelect::Ss0,
        500_000,  // 500kHz
        Mode::Mode1  // CPOL=0, CPHA=1
    )?;
    
    println!("SPI initialized");
    
    // Send RESET command (0x06)
    spi.write(&[0x06])?;
    println!("RESET command sent");
    
    // Send zeros (as in the Python script)
    spi.write(&[0x00, 0x00, 0x00])?;
    println!("Zeros sent");
    
    // Send SDATAC command (0x11)
    spi.write(&[0x11])?;
    println!("SDATAC command sent");
    
    // Send RREG command for ID register (0x20, 0x00)
    spi.write(&[0x20, 0x00])?;
    println!("RREG command sent");
    
    // Read the result
    let mut buffer = [0u8];
    spi.transfer(&mut buffer, &[0u8])?;
    
    println!("ID register value: 0x{:02x}", buffer[0]);
    
    if buffer[0] == 0x3E {
        println!("SUCCESS: Correct ID value (0x3E) read from ADS1299!");
    } else {
        println!("ERROR: Incorrect ID value. Expected 0x3E, got 0x{:02x}", buffer[0]);
    }
    
    Ok(())
}