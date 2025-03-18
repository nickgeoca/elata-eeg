use rppal::spi::{Spi, Bus, SlaveSelect, Mode};
use rppal::gpio::{Gpio, InputPin, Level};
use std::error::Error;
use std::thread::sleep;
use std::time::Duration;

fn main() -> Result<(), Box<dyn Error>> {
    read_channel_data_test()
    id_register_test()
}


fn id_register_test() -> Result<(), Box<dyn Error>> {
    println!("Starting minimal ADS1299 ID register test...");
    
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

fn read_channel_data_test() -> Result<(), Box<dyn Error>> {
    println!("Starting ADS1299 channel 0 data reading test...");
    
    // Initialize SPI
    let mut spi = Spi::new(
        Bus::Spi0,
        SlaveSelect::Ss0,
        500_000,  // 500kHz
        Mode::Mode1  // CPOL=0, CPHA=1
    )?;
    
    // Initialize DRDY pin (GPIO25)
    let gpio = Gpio::new()?;
    let drdy_pin = gpio.get(25)?.into_input_pullup();
    
    println!("SPI and GPIO initialized");
    
    // Power-up sequence
    // 1. Send RESET command
    spi.write(&[0x06])?;
    sleep(Duration::from_millis(10));
    
    // 2. Send zeros
    spi.write(&[0x00, 0x00, 0x00])?;
    sleep(Duration::from_millis(10));
    
    // 3. Send SDATAC command
    spi.write(&[0x11])?;
    sleep(Duration::from_millis(10));
    
    // 4. Check device ID
    spi.write(&[0x20, 0x00])?;
    let mut buffer = [0u8];
    spi.transfer(&mut buffer, &[0u8])?;
    
    if buffer[0] != 0x3E {
        return Err(format!("Invalid device ID: 0x{:02X}, expected 0x3E", buffer[0]).into());
    }
    println!("Device ID verified: 0x{:02X}", buffer[0]);
    
    // 5. Configure registers
    println!("Configuring ADS1299 registers...");
    
    // CONFIG1: Set sample rate to 250 SPS
    spi.write(&[0x41, 0x00, 0x96])?;
    
    // Verify CONFIG1 was set correctly
    spi.write(&[0x21, 0x00])?; // RREG command for CONFIG1
    let mut buffer = [0u8];
    spi.transfer(&mut buffer, &[0u8])?;
    println!("CONFIG1 value after setting: 0x{:02X} (expected 0x96)", buffer[0]);
    
    // CONFIG2: Enable internal reference and test signal with larger amplitude
    // 0xD3 = 1101 0011 (Bit7-6=11: Internal reference, Bit5=0, Bit4=1: Test signal enabled,
    // Bit3-2=00, Bit1-0=11: Test signal frequency = fCLK / 2^19)
    spi.write(&[0x42, 0x00, 0xD3])?;
    
    // Verify CONFIG2 was set correctly
    spi.write(&[0x22, 0x00])?; // RREG command for CONFIG2
    spi.transfer(&mut buffer, &[0u8])?;
    println!("CONFIG2 value after setting: 0x{:02X} (expected 0xD3)", buffer[0]);
    
    // CONFIG3: Enable bias buffer and internal reference for bias
    // 0x66 = 0110 0110 (PD_REFBUF=0, Bit6=1, Bit5=1, BIAS_MEAS=0, BIASREF_INT=0, PD_BIAS=1, BIAS_LOFF_SENS=1, BIAS_STAT=0)
    spi.write(&[0x43, 0x00, 0x66])?;
    
    // Verify CONFIG3 was set correctly
    spi.write(&[0x23, 0x00])?; // RREG command for CONFIG3
    spi.transfer(&mut buffer, &[0u8])?;
    println!("CONFIG3 value after setting: 0x{:02X} (expected 0x66)", buffer[0]);
    
    // CH1SET: Configure channel 0 for test signal with gain=24 and SRB2 disabled
    // 0x05 = 0000 0101 (PD=0, GAIN=000 (gain=6), SRB2=0, MUX=101 (test signal))
    spi.write(&[0x45, 0x00, 0x05])?;
    
    // Verify CH1SET was set correctly
    spi.write(&[0x25, 0x00])?; // RREG command for CH1SET
    spi.transfer(&mut buffer, &[0u8])?;
    println!("CH1SET value after setting: 0x{:02X} (expected 0x05)", buffer[0]);
    
    // Also configure CH2SET to test signal to see if we get data on another channel
    spi.write(&[0x46, 0x00, 0x05])?;
    
    // Verify CH2SET was set correctly
    spi.write(&[0x26, 0x00])?; // RREG command for CH2SET
    spi.transfer(&mut buffer, &[0u8])?;
    println!("CH2SET value after setting: 0x{:02X} (expected 0x05)", buffer[0]);
    
    // CONFIG4: Disable lead-off comparators
    spi.write(&[0x57, 0x00, 0x00])?;
    
    // Verify CONFIG4 was set correctly
    spi.write(&[0x37, 0x00])?; // RREG command for CONFIG4
    spi.transfer(&mut buffer, &[0u8])?;
    println!("CONFIG4 value after setting: 0x{:02X} (expected 0x00)", buffer[0]);
    
    // Add a longer delay after configuration
    println!("Waiting for configuration to settle...");
    sleep(Duration::from_millis(100));
    
    // Make sure we're in SDATAC mode before starting
    spi.write(&[0x11])?;
    sleep(Duration::from_millis(10));
    
    // 6. Start data conversion
    spi.write(&[0x08])?;
    sleep(Duration::from_millis(10));
    
    // 7. Enter continuous data mode (RDATAC)
    spi.write(&[0x10])?;
    sleep(Duration::from_millis(10));
    
    println!("Data conversion started in continuous mode");
    
    // 8. Read data samples
    println!("Reading 10 samples from channel 0...");
    for i in 0..10 {
        // Wait for DRDY to go low
        let mut timeout = 1000;
        let drdy_start = std::time::Instant::now();
        let initial_drdy_state = drdy_pin.read();
        println!("Initial DRDY state: {:?}", initial_drdy_state);
        
        while drdy_pin.read() == Level::High && timeout > 0 {
            sleep(Duration::from_micros(10));
            timeout -= 1;
        }
        
        let drdy_duration = drdy_start.elapsed();
        
        if timeout == 0 {
            println!("DRDY timeout - pin never went low");
            continue;
        } else {
            println!("DRDY went low after {:?} (timeout count: {})", drdy_duration, 1000 - timeout);
        }
        
        // Read data directly without sending commands each time
        // 3 status bytes + (3 bytes per channel * 2 channels) = 9 bytes
        let mut read_buffer = [0u8; 9];
        let write_buffer = [0u8; 9];
        
        println!("Reading data frame directly...");
        match spi.transfer(&mut read_buffer, &write_buffer) {
            Ok(_) => println!("SPI transfer successful"),
            Err(e) => println!("SPI transfer error: {}", e),
        }
        
        println!("All bytes: [{:02X}, {:02X}, {:02X}, {:02X}, {:02X}, {:02X}, {:02X}, {:02X}, {:02X}]",
                 read_buffer[0], read_buffer[1], read_buffer[2],
                 read_buffer[3], read_buffer[4], read_buffer[5],
                 read_buffer[6], read_buffer[7], read_buffer[8]);
        
        println!("Status byte: 0x{:02X}", read_buffer[0]);
        
        // Parse channel 1 data (skip the first status byte)
        let ch1_msb = read_buffer[1] as i32;
        let ch1_mid = read_buffer[2] as i32;
        let ch1_lsb = read_buffer[3] as i32;
        
        // Combine bytes into a 24-bit signed integer
        let mut ch1_value = (ch1_msb << 16) | (ch1_mid << 8) | ch1_lsb;
        
        // Sign extension for negative values
        if (ch1_value & 0x800000) != 0 {
            ch1_value |= -16777216; // 0xFF000000 as signed
        }
        
        // Parse channel 2 data
        let ch2_msb = read_buffer[4] as i32;
        let ch2_mid = read_buffer[5] as i32;
        let ch2_lsb = read_buffer[6] as i32;
        
        // Combine bytes into a 24-bit signed integer
        let mut ch2_value = (ch2_msb << 16) | (ch2_mid << 8) | ch2_lsb;
        
        // Sign extension for negative values
        if (ch2_value & 0x800000) != 0 {
            ch2_value |= -16777216; // 0xFF000000 as signed
        }
        
        println!("Sample {}: Channel 1 raw bytes [{:02X} {:02X} {:02X}] = {}",
                 i, read_buffer[1], read_buffer[2], read_buffer[3], ch1_value);
        println!("Sample {}: Channel 2 raw bytes [{:02X} {:02X} {:02X}] = {}",
                 i, read_buffer[4], read_buffer[5], read_buffer[6], ch2_value);
    }
    
    // 9. Exit continuous data mode (SDATAC)
    spi.write(&[0x11])?;
    sleep(Duration::from_millis(10));
    
    // 10. Stop data conversion
    spi.write(&[0x0A])?;
    println!("Data conversion stopped");
    
    Ok(())
}