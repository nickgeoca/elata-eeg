//! Helper functions for the ADS1299 driver.

use crate::board_drivers::types::DriverError;
use log::debug;

/// Convert 24-bit SPI data to a signed 32-bit integer (sign-extended)
pub fn ch_sample_to_raw(msb: u8, mid: u8, lsb: u8) -> i32 {
    let raw_value = ((msb as u32) << 16) | ((mid as u32) << 8) | (lsb as u32);
    ((raw_value as i32) << 8) >> 8
}

/// Convert signed raw ADC value to voltage using VREF and gain
/// Formula: voltage = (raw * (VREF / Gain)) / 2^23
pub fn ch_raw_to_voltage(raw: i32, vref: f32, gain: f32) -> f32 {
    let voltage = ((raw as f64) * ((vref / gain) as f64) / (1 << 23) as f64) as f32;
    // Add detailed logging (using debug! instead of trace!)
    debug!("ch_raw_to_voltage: raw={}, vref={}, gain={}, calculated_voltage={}", raw, vref, gain, voltage);
    voltage
}

/// Helper function to get current timestamp in microseconds
pub fn current_timestamp_micros() -> Result<u64, DriverError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .map_err(|e| DriverError::Other(format!("Failed to get timestamp: {}", e)))
}

/// Helper function to read data from SPI in continuous mode (RDATAC)
pub fn read_data_from_spi(spi: &mut dyn crate::board_drivers::ads1299::spi::SpiDevice, num_channels: usize) -> Result<Vec<i32>, DriverError> {
    debug!("Reading data from ADS1299 via SPI for {} channels in continuous mode", num_channels);

    // In continuous mode (RDATAC), we don't need to send RDATA command before each read
    // We just read the data directly when DRDY goes low

    // Calculate total bytes to read: 3 status bytes + (3 bytes per channel * num_channels)
    let total_bytes = 3 + (3 * num_channels);
    debug!("Reading {} total bytes (3 status + {} data bytes)", total_bytes, 3 * num_channels);

    // Prepare buffers for SPI transfer
    let mut read_buffer = vec![0u8; total_bytes];
    let write_buffer = vec![0u8; total_bytes];

    // Perform SPI transfer
    match spi.transfer(&mut read_buffer, &write_buffer) {
        Ok(_) => debug!("SPI transfer successful, read {} bytes", read_buffer.len()),
        Err(e) => {
            log::error!("SPI transfer error: {}", e);
            return Err(DriverError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("SPI transfer error: {}", e)
            )));
        }
    }

    // Log raw data for debugging
    debug!("Raw SPI data: {:02X?}", read_buffer);

    // Log status bytes
    debug!("Status bytes: [{:02X} {:02X} {:02X}]",
           read_buffer[0], read_buffer[1], read_buffer[2]);

    // Parse the data (skip the first 3 status bytes)
    let mut samples = Vec::with_capacity(num_channels);

    for ch in 0..num_channels {
        let start_idx = 3 + (ch * 3); // Skip 3 status bytes, then 3 bytes per channel

        // Extract the 3 bytes for this channel
        let msb = read_buffer[start_idx];
        let mid = read_buffer[start_idx + 1];
        let lsb = read_buffer[start_idx + 2];

        // Log raw bytes BEFORE conversion
        debug!("Channel {}: raw bytes [{:02X} {:02X} {:02X}]", ch, msb, mid, lsb);

        // Convert to i32 using the ch_sample_to_raw function
        let value = ch_sample_to_raw(msb, mid, lsb);

        // Log converted value
        debug!("Channel {}: converted raw value = {}", ch, value);

        samples.push(value);
    }

    Ok(samples)
}