
/// Messages passed from the synchronous sensor thread to the asynchronous Tokio runtime.
///
/// This enum encapsulates the different types of events that can occur during sensor
/// data acquisition, allowing for structured communication across thread boundaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BridgeMsg {
    /// Contains a packet of sensor data.
    Data(Packet<i32>),
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
#[cfg(feature = "meta-tags")]
use std::collections::HashMap;
use std::sync::Arc;

/// Metadata describing the sensor and its data format.
///
/// This struct is designed to be immutable and shared via an `Arc`, ensuring
/// that every data packet is self-describing without significant overhead.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SensorMeta {
    pub schema_ver: u8,
    pub source_type: String,
    pub v_ref: f32,
    pub adc_bits: u8,
    pub gain: f32,
    pub sample_rate: u32,

    // v2 additions based on feedback
    /// The digital value corresponding to 0V.
    #[serde(default)]
    pub offset_code: i32,
    /// True if the ADC output is two's complement.
    #[serde(default = "true_default")]
    pub is_twos_complement: bool,
    /// Optional feature-gated tags for user-defined metadata.
    #[cfg(feature = "meta-tags")]
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

impl Default for SensorMeta {
    fn default() -> Self {
        Self {
            schema_ver: 1,
            source_type: "default".to_string(),
            v_ref: 5.0,
            adc_bits: 24,
            gain: 1.0,
            sample_rate: 1000,
            offset_code: 0,
            is_twos_complement: true,
            #[cfg(feature = "meta-tags")]
            tags: HashMap::new(),
        }
    }
}

fn true_default() -> bool {
    true
}

/// The header for a data packet, containing metadata and a timestamp.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PacketHeader {
    /// Monotonic timestamp from the driver's sample acquisition clock (in nanoseconds).
    pub ts_ns: u64,
    /// The number of samples in the `samples` field of the `Packet`.
    pub batch_size: u32,
    /// A shared pointer to the immutable sensor metadata.
    #[serde(with = "arc_sensor_meta")]
    pub meta: Arc<SensorMeta>,
}

mod arc_sensor_meta {
    use super::SensorMeta;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<S>(arc: &Arc<SensorMeta>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        arc.as_ref().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<SensorMeta>, D::Error>
    where
        D: Deserializer<'de>,
    {
        SensorMeta::deserialize(deserializer).map(Arc::new)
    }
}

/// A generic data packet that flows through the pipeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Packet<T> {
    pub header: PacketHeader,
    /// The sample data.
    /// Note: Consider `Arc<[T]>` or `Box<[T]>` in the future for true zero-copy from DMA buffers.
    pub samples: Vec<T>,
}