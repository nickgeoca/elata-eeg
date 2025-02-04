use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use anyhow::Result;

fn main() -> Result<()> {
    env_logger::init();

    // Example SPI init: Bus 0, SlaveSelect 0, 1 MHz, Mode0
    let mut spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, 1_000_000, Mode::Mode0)
        .expect("Failed to initialize SPI");

    // Example: Write some bytes
    let write_data = [0xAA, 0xBB, 0xCC];
    spi.write(&write_data)?;

    // Example: Read response from device (dummy read here)
    let mut read_data = [0u8; 3];
    spi.read(&mut read_data)?;
    println!("SPI read: {:?}", read_data);

    Ok(())
}
