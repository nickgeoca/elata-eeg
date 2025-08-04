use std::sync::Arc;
use bytemuck;
use eeg_types::comms::{BrokerMessage, BrokerPayload};
use eeg_types::data::PacketHeader;
use crate::data::RtPacket;
use crate::daemon_protocol::{DataPacketHeader, MetaUpdateMsg};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::{
    error::StageError,
    registry::StageFactory,
    stage::{Stage, StageContext, StageInitCtx},
};

#[derive(Debug, Deserialize)]
pub struct WebsocketSinkParams {
    pub topic: String,
}

pub struct WebsocketSink {
    topic: String,
    sender: broadcast::Sender<Arc<BrokerMessage>>,
    last_meta_rev: Option<u32>,
}

impl Stage for WebsocketSink {
    fn id(&self) -> &str {
        &self.topic
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        _ctx: &mut StageContext,
    ) -> std::result::Result<Option<Arc<RtPacket>>, StageError> {
        let (header, samples_bytes, packet_type) = match &*packet {
            RtPacket::Voltage(data) => (&data.header, bytemuck::cast_slice(&data.samples), "Voltage"),
            RtPacket::RawI32(data) => (&data.header, bytemuck::cast_slice(&data.samples), "RawI32"),
            // Note: RawAndVoltage is not handled in this protocol version for simplicity.
            // It would require a more complex sample layout.
            _ => return Ok(Some(packet)),
        };

        // Send metadata if it's the first packet or if the metadata has changed.
        if self.last_meta_rev != Some(header.meta.meta_rev) {
            let meta_msg = MetaUpdateMsg {
                message_type: "meta_update",
                topic: &self.topic,
                meta: &header.meta,
            };
            let json_payload = serde_json::to_string(&meta_msg)
                .map_err(|e| StageError::Processing(e.to_string()))?;

            let broker_msg = Arc::new(BrokerMessage {
                topic: self.topic.clone(),
                payload: BrokerPayload::Meta(json_payload),
            });
            let _ = self.sender.send(broker_msg); // Ignore error if no subscribers
            self.last_meta_rev = Some(header.meta.meta_rev);
        }

        // Create and serialize the data packet header
        let data_header = DataPacketHeader::new(header, &self.topic, packet_type);
        let json_header = serde_json::to_string(&data_header)
            .map_err(|e| StageError::Processing(e.to_string()))?;
        let json_header_bytes = json_header.as_bytes();
        let json_len = json_header_bytes.len() as u32;

        // Construct the final binary payload
        let json_padding = (4 - (json_header_bytes.len() % 4)) % 4;
        let mut binary_payload = Vec::with_capacity(4 + json_header_bytes.len() + json_padding + samples_bytes.len());
        binary_payload.extend_from_slice(&json_len.to_le_bytes());
        binary_payload.extend_from_slice(json_header_bytes);
        
        // Add padding to ensure samples data is 4-byte aligned for Float32Array
        for _ in 0..json_padding {
            binary_payload.push(0);
        }
        
        binary_payload.extend_from_slice(samples_bytes);

        // Send the data packet
        let broker_msg = Arc::new(BrokerMessage {
            topic: self.topic.clone(),
            payload: BrokerPayload::Data(binary_payload),
        });
        let _ = self.sender.send(broker_msg); // Ignore error if no subscribers

        Ok(Some(packet))
    }
}

#[derive(Default)]
pub struct WebsocketSinkFactory;

impl StageFactory for WebsocketSinkFactory {
    fn create(
        &self,
        config: &crate::config::StageConfig,
        ctx: &StageInitCtx,
    ) -> std::result::Result<(Box<dyn Stage>, Option<flume::Receiver<Arc<RtPacket>>>), StageError> {
        let params_value = serde_json::to_value(&config.params)
            .map_err(|e| StageError::BadConfig(e.to_string()))?;
        let sink_params: WebsocketSinkParams = serde_json::from_value(params_value)
            .map_err(|e| StageError::BadConfig(e.to_string()))?;
        let sender = ctx
            .websocket_sender
            .clone()
            .ok_or_else(|| StageError::NotReady("WebSocket broker not available".to_string()))?;

        Ok((
            Box::new(WebsocketSink {
                topic: sink_params.topic,
                sender,
                last_meta_rev: None,
            }),
            None,
        ))
    }
}