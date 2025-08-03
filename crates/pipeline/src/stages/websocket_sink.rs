use std::sync::Arc;

use eeg_types::comms::BrokerMessage;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::{
    data::{PacketData, PacketOwned, RtPacket},
    error::{StageError},
    registry::StageFactory,
    stage::{Stage, StageContext, StageInitCtx},
};
use anyhow::Result;

#[derive(Debug, Deserialize)]
pub struct WebsocketSinkParams {
    pub topic: String,
}

pub struct WebsocketSink {
    id: String,
    topic: String,
    sender: broadcast::Sender<Arc<BrokerMessage>>,
}

impl Stage for WebsocketSink {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Arc<RtPacket>,
        _ctx: &mut StageContext,
    ) -> std::result::Result<Option<Arc<RtPacket>>, StageError> {
        let message = BrokerMessage {
            topic: self.topic.clone(),
            packet: from_rt_packet(&packet),
        };

        // Send errors are ignored, since they just mean there are no
        // active subscribers. This is not a pipeline-terminating error.
        let _ = self.sender.send(Arc::new(message));

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
                id: sink_params.topic.clone(),
                topic: sink_params.topic,
                sender,
            }),
            None,
        ))
    }
}

fn from_rt_packet(packet: &RtPacket) -> PacketOwned {
    match &*packet {
        RtPacket::RawI32(data) => {
            let owned_data = PacketData {
                header: data.header.clone(),
                samples: data.samples.to_vec(),
            };
            PacketOwned::RawI32(owned_data)
        }
        RtPacket::Voltage(data) => {
            let owned_data = PacketData {
                header: data.header.clone(),
                samples: data.samples.to_vec(),
            };
            PacketOwned::Voltage(owned_data)
        }
        RtPacket::RawAndVoltage(data) => {
            let owned_data = PacketData {
                header: data.header.clone(),
                samples: data.samples.to_vec(),
            };
            PacketOwned::RawAndVoltage(owned_data)
        }
    }
}