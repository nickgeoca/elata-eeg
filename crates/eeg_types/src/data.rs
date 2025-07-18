
/// Messages passed from the synchronous sensor thread to the asynchronous Tokio runtime.
///
/// This enum encapsulates the different types of events that can occur during sensor
/// data acquisition, allowing for structured communication across thread boundaries.
use pipeline::data::{PacketOwned, RtPacket};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BridgeMsg {
    /// Contains a packet of sensor data.
    Data(PacketOwned),
    /// Signals that an error occurred in the sensor driver.
    Error(SensorError),
}

impl From<RtPacket> for BridgeMsg {
    fn from(runtime_packet: RtPacket) -> Self {
        BridgeMsg::Data(runtime_packet.into())
    }
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
    /// A driver-level error.
    #[error("Driver error: {0}")]
    DriverError(String),
}

use serde::{Deserialize, Serialize};