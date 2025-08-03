use crate::data::PacketOwned;
use serde::Serialize;

// Message sent from a pipeline stage to the daemon's broker
#[derive(Debug, Serialize)]
pub struct BrokerMessage {
    pub topic: String,
    pub packet: PacketOwned,
}