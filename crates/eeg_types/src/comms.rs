use std::sync::Arc;
use crate::data::PacketOwned;

// Message sent from a pipeline stage to the daemon's broker
#[derive(Debug)]
pub struct BrokerMessage {
    pub topic: String,
    pub packet: PacketOwned,
}