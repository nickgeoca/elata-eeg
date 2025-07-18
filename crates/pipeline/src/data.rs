use crate::allocator::{RecycledF32Vec, RecycledI32F32TupleVec, RecycledI32Vec};
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
    // Stable identity
    /// A unique, stable identifier for the sensor source.
    #[serde(default)]
    pub sensor_id: u32,
    /// A revision number for the metadata itself. Should be incremented
    /// whenever a value that affects processing (like gain) changes.
    #[serde(default)]
    pub meta_rev: u32,

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
    /// Optional list of names for each channel in the data stream.
    #[serde(default)]
    pub channel_names: Vec<String>,
    /// Optional feature-gated tags for user-defined metadata.
    #[cfg(feature = "meta-tags")]
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

impl Default for SensorMeta {
    fn default() -> Self {
        Self {
            sensor_id: 0,
            meta_rev: 0,
            schema_ver: 1,
            source_type: "default".to_string(),
            v_ref: 4.5,
            adc_bits: 24,
            gain: 1.0,
            sample_rate: 1000,
            offset_code: 0,
            is_twos_complement: true,
            channel_names: Vec::new(),
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
// No `PartialEq` here. It's expensive and rarely needed for the runtime data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketData<T> {
    pub header: PacketHeader,
    /// The sample data, held in a vector that will be returned to a pool on drop.
    pub samples: T,
}

/// The different types of data packets that can exist in the pipeline at runtime.
/// This enum is intentionally NOT `Clone`. Cloning a packet should be a deliberate
/// act of either cloning the `Arc<RtPacket>` (cheap) or performing a deep copy
/// into a `PacketOwned` (expensive).
#[derive(Debug)]
pub enum RtPacket {
    RawI32(PacketData<RecycledI32Vec>),
    Voltage(PacketData<RecycledF32Vec>),
    RawAndVoltage(PacketData<RecycledI32F32TupleVec>),
}

impl RtPacket {
    /// Performs an explicit, deep clone of the packet data, allocating new recycled buffers.
    pub fn deep_clone(&self) -> Self {
        match self {
            RtPacket::RawI32(data) => RtPacket::RawI32(PacketData {
                header: data.header.clone(),
                samples: (data.samples.to_vec(), data.samples.allocator().clone()).into(),
            }),
            RtPacket::Voltage(data) => RtPacket::Voltage(PacketData {
                header: data.header.clone(),
                samples: (data.samples.to_vec(), data.samples.allocator().clone()).into(),
            }),
            RtPacket::RawAndVoltage(data) => RtPacket::RawAndVoltage(PacketData {
                header: data.header.clone(),
                samples: (data.samples.to_vec(), data.samples.allocator().clone()).into(),
            }),
        }
    }
}

/// A serializable, owned version of a packet for inter-thread/network communication,
/// logging, or any time a deep copy is explicitly required.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PacketOwned {
    RawI32(PacketData<Vec<i32>>),
    Voltage(PacketData<Vec<f32>>),
    RawAndVoltage(PacketData<Vec<(i32, f32)>>),
}

impl From<RtPacket> for PacketOwned {
    fn from(runtime_packet: RtPacket) -> Self {
        match runtime_packet {
            RtPacket::RawI32(data) => PacketOwned::RawI32(PacketData {
                header: data.header,
                samples: data.samples.to_vec(),
            }),
            RtPacket::Voltage(data) => PacketOwned::Voltage(PacketData {
                header: data.header,
                samples: data.samples.to_vec(),
            }),
            RtPacket::RawAndVoltage(data) => PacketOwned::RawAndVoltage(PacketData {
                header: data.header,
                samples: data.samples.to_vec(),
            }),
        }
    }
}

impl From<PacketOwned> for RtPacket {
    fn from(owned_packet: PacketOwned) -> Self {
        match owned_packet {
            PacketOwned::RawI32(data) => RtPacket::RawI32(PacketData {
                header: data.header,
                samples: (data.samples, Default::default()).into(),
            }),
            PacketOwned::Voltage(data) => RtPacket::Voltage(PacketData {
                header: data.header,
                samples: (data.samples, Default::default()).into(),
            }),
            PacketOwned::RawAndVoltage(data) => RtPacket::RawAndVoltage(PacketData {
                header: data.header,
                samples: (data.samples, Default::default()).into(),
            }),
        }
    }
}
/// A read-only, zero-copy view into a runtime packet's data.
///
/// This is the primary way that stages and plugins should interact with packet
/// data. It exposes the data as simple slices, completely hiding the underlying
/// allocator-specific vector types.
pub enum PacketView<'a> {
    RawI32 {
        header: &'a PacketHeader,
        data: &'a [i32],
    },
    Voltage {
        header: &'a PacketHeader,
        data: &'a [f32],
    },
    RawAndVoltage {
        header: &'a PacketHeader,
        data: &'a [(i32, f32)],
    },
}

impl<'a> From<&'a RtPacket> for PacketView<'a> {
    fn from(packet: &'a RtPacket) -> Self {
        match packet {
            RtPacket::RawI32(d) => PacketView::RawI32 {
                header: &d.header,
                data: &d.samples,
            },
            RtPacket::Voltage(d) => PacketView::Voltage {
                header: &d.header,
                data: &d.samples,
            },
            RtPacket::RawAndVoltage(d) => PacketView::RawAndVoltage {
                header: &d.header,
                data: &d.samples,
            },
        }
    }
}