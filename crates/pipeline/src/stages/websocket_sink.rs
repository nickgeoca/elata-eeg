use std::sync::Arc;

use eeg_types::comms::pipeline::{BrokerMessage, BrokerPayload};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::{
    data::RtPacket,
    daemon_protocol::MetaUpdateMsg,
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
    ) -> Result<Vec<(String, Arc<RtPacket>)>, StageError> {
        // 1. Determine the packet type and get header/samples.
        // This also serves as our primary assertion. The `match` is exhaustive,
        // but we only handle the `Voltage` case. Any other packet type will
        // trigger the panic, immediately revealing a pipeline wiring issue.
        let (header, samples_bytes, packet_type) = match &*packet {
            RtPacket::Voltage(data) => (
                &data.header,
                bytemuck::cast_slice(&data.samples),
                "Voltage",
            ),
            RtPacket::VoltageF32(data) => (
                &data.header,
                bytemuck::cast_slice(&data.samples),
                "VoltageF32",
            ),
            other => {
                // Panic if we receive any packet type other than Voltage.
                panic!(
                    "websocket_sink received unexpected packet type: {:?}. This indicates a misconfigured pipeline.",
                    other
                );
            }
        };

        tracing::debug!(
            topic = %self.topic,
            packet_type = packet_type,
            meta_rev = header.meta.meta_rev,
            "sink_got_packet"
        );

        // 2. Send metadata update if revision has changed
        if self.last_meta_rev != Some(header.meta.meta_rev) {
            let meta_msg = MetaUpdateMsg {
                message_type: "meta_update",
                topic: &self.topic,
                meta: &header.meta,
            };
            let json_payload = serde_json::to_string(&meta_msg)?;
            let data_broker_msg = Arc::new(BrokerMessage::Data {
                topic: self.topic.clone(),
                payload: BrokerPayload::Meta {
                    json: json_payload,
                    meta_rev: header.meta.meta_rev,
                },
            });
            let _ = self.sender.send(data_broker_msg);

            self.last_meta_rev = Some(header.meta.meta_rev);
        }

        // 3. Construct the final binary payload using the standardized header format.
        // [u32 meta_rev][u64 seq][u64 t0_ns][u32 n_samples][payload...]
        let n_samples = header.batch_size;
        let mut binary_payload =
        	Vec::with_capacity(4 + 8 + 8 + 4 + samples_bytes.len());
        
        binary_payload.extend_from_slice(&header.meta.meta_rev.to_le_bytes());
        binary_payload.extend_from_slice(&header.frame_id.to_le_bytes());
        binary_payload.extend_from_slice(&header.ts_ns.to_le_bytes());
        binary_payload.extend_from_slice(&n_samples.to_le_bytes());
        binary_payload.extend_from_slice(samples_bytes);
      
        // 4. Send the data packet as a single binary message
        let broker_msg = Arc::new(BrokerMessage::Data {
            topic: self.topic.clone(),
            payload: BrokerPayload::Data(binary_payload.into()),
        });
        let _ = self.sender.send(broker_msg);

        // This is a sink, so it does not produce any output packets
        Ok(vec![])
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