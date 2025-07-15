#[cfg(feature = "elata_v1")]
pub mod elata_v1;

#[cfg(feature = "elata_v2")]
pub mod elata_v2;

use sensors::{AdcDriver, AdcConfig, DriverType};
use std::error::Error;

/// Factory function to create drivers based on configuration
pub fn create_driver(config: AdcConfig) -> Result<Box<dyn AdcDriver>, Box<dyn Error>> {
    match config.board_driver {
        #[cfg(feature = "elata_v1")]
        DriverType::ElataV1 => {
            let driver = elata_v1::ElataV1Driver::new(config)?;
            Ok(Box::new(driver))
        }
        #[cfg(feature = "elata_v2")]
        DriverType::ElataV2 => {
            let driver = elata_v2::ElataV2Driver::new(config)?;
            Ok(Box::new(driver))
        }
        // Note: The Ads1299 and MockEeg drivers are now internal to the sensors crate
        // and are not directly exposed by the boards crate.
        _ => Err(Box::new(sensors::DriverError::ConfigurationError(
            "Unsupported or disabled board driver type".to_string(),
        ))),
    }
}
