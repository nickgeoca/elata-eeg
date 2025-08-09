use serde::{Deserialize, Serialize};

pub mod pipeline {
    use super::*;
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
}

pub mod client {
    use super::*;
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub enum ServerMessage {
        Subscribed(String),
        Error(String),
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    #[serde(tag = "type", rename_all = "camelCase")]
    pub enum ClientMessage {
        Subscribe { topic: String },
        Unsubscribe { topic: String },
    }
}