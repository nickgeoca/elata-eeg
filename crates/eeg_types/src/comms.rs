use serde::Serialize;

/// The payload sent from a pipeline stage to the broker.
/// This can be either a metadata update or a data packet.
#[derive(Debug, Clone, Serialize)]
pub enum BrokerPayload {
    /// A JSON string containing a `MetaUpdateMsg`.
    Meta(String),
    /// A binary blob containing a `data_packet` (header + samples).
    Data(Vec<u8>),
}

/// Message sent from a pipeline stage to the daemon's broker.
#[derive(Debug, Clone, Serialize)]
pub struct BrokerMessage {
    pub topic: String,
    pub payload: BrokerPayload,
}