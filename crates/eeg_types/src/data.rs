
/// Messages passed from the synchronous sensor thread to the asynchronous Tokio runtime.
///
/// This enum encapsulates the different types of events that can occur during sensor
/// data acquisition, allowing for structured communication across thread boundaries.
use pipeline::data::Packet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BridgeMsg {
    /// Contains a packet of sensor data.
    Data(Packet),
    /// Signals that an error occurred in the sensor driver.
    Error(SensorError),
}

/// Represents errors that can occur within a sensor driver.
///
/// These errors are intended to be propagated to the UI to provide feedback on the
/// state of the hardware.
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum SensorError {
    /// A hardware-related fault.
    #[error("Sensor hardware fault: {0}")]
    HardwareFault(String),
    /// The internal buffer was overrun.
    #[error("Sensor buffer overrun")]
    BufferOverrun,
    /// The sensor was disconnected.
    #[error("Sensor disconnected")]
    Disconnected,
}
use serde::{Deserialize, Serialize};