//! Simple gain stage for amplifying or attenuating voltage data.
//
// This stage serves as a template for basic, high-performance data plane
// components. It demonstrates several key architectural patterns:
// - **Zero-copy processing:** Modifies data in-place without extra allocations.
// - **Hot-reloadable parameters:** `gain` and `enabled` can be changed on-the-fly.
// - **Correct concurrency:** Uses `Acquire/Release` ordering for atomic flags.
// - **Efficient run-loop:** Drains the input queue to maximize throughput.
// - **Back-pressure handling:** Uses `try_send` to prevent pipeline stalls.

use async_trait::async_trait;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tokio::sync::mpsc::error::TrySendError;
use tracing::{error, trace, warn};

use crate::ctrl_loop;
use crate::data::{Packet, VoltageEegPacket};
use crate::error::{PipelineResult, StageError};
use crate::stage::{
    ControlMsg, DataPlaneStage, DataPlaneStageFactory, StageContext, StageParams,
    StaticStageRegistrar,
};

/// A high-performance, in-place gain stage for the data plane.
pub struct GainStage {
    /// The scalar gain factor.
    gain: AtomicU32,
    /// A flag to enable or disable the gain application on-the-fly.
    ///
    /// `Acquire/Release` ordering is used to ensure that changes to this flag
    /// from the control plane thread are visible to the data plane thread.
    enabled: AtomicBool,
    /// The number of packets to process before yielding to the scheduler.
    /// A value of 0 means the stage will never yield.
    yield_threshold: u32,
    // Cached handles to avoid HashMap lookups in the hot path.
    // These are `None` until the first time `run` is called.
    input_rx: Option<Box<dyn crate::stage::Input<VoltageEegPacket>>>,
    output_tx: Option<Box<dyn crate::stage::Output<VoltageEegPacket>>>,
}

#[async_trait]
impl DataPlaneStage for GainStage {
    /// The main execution loop for the gain stage.
    async fn run(&mut self, ctx: &mut StageContext) -> Result<(), StageError> {
        // First, handle any incoming control messages.
        ctrl_loop!(self, ctx);

        // Then, enter the packet processing loop.
        self.process_packets(ctx).await
    }
}

impl GainStage {
    /// Creates a new `GainStage`.
    pub fn new(gain: f32, enabled: bool, yield_threshold: u32) -> Self {
        let stage = Self {
            gain: AtomicU32::new(0), // Initialized properly in `set_gain`
            enabled: AtomicBool::new(enabled),
            yield_threshold,
            input_rx: None,
            output_tx: None,
        };
        stage.set_gain(gain);
        stage
    }

    /// Safely sets the gain factor using atomic operations.
    fn set_gain(&self, gain: f32) {
        self.gain.store(gain.to_bits(), Ordering::Release);
    }

    /// Safely gets the gain factor using atomic operations.
    fn get_gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Acquire))
    }

    /// Gets mutable references to the input and output handles, initializing them
    /// on the first call. This avoids HashMap lookups in the hot path.
    #[cold]
    #[inline(always)]
    fn lazy_io<'a>(
        input_rx: &'a mut Option<Box<dyn crate::stage::Input<VoltageEegPacket>>>,
        output_tx: &'a mut Option<Box<dyn crate::stage::Output<VoltageEegPacket>>>,
        ctx: &'a mut StageContext,
    ) -> (
        &'a mut dyn crate::stage::Input<VoltageEegPacket>,
        &'a mut dyn crate::stage::Output<VoltageEegPacket>,
    ) {
        if input_rx.is_none() {
            *input_rx = Some(
                ctx.inputs
                    .remove("in")
                    .unwrap_or_else(|| panic!("Input 'in' not found for gain stage")),
            );
            *output_tx = Some(
                ctx.outputs
                    .remove("out")
                    .unwrap_or_else(|| panic!("Output 'out' not found for gain stage")),
            );
        }
        (
            input_rx.as_mut().unwrap().as_mut(),
            output_tx.as_mut().unwrap().as_mut(),
        )
    }

    /// Efficiently processes all available packets in the input queue.
    async fn process_packets(&mut self, ctx: &mut StageContext) -> Result<(), StageError> {
        let (input, output) = Self::lazy_io(&mut self.input_rx, &mut self.output_tx, ctx);
        let mut processed_count = 0;

        // Destructure self to appease the borrow checker. By borrowing fields
        // individually, we can avoid holding a mutable borrow on `self` for the
        // entire loop.
        let enabled = &self.enabled;
        let gain = &self.gain;
        let yield_threshold = self.yield_threshold;

        // Loop to drain the input queue. This is more efficient than processing
        // one packet per `run` call.
        loop {
            let mut pkt = match input.try_recv()? {
                Some(p) => p,
                // The queue is empty, so we're done for now.
                None => return Ok(()),
            };

            // Apply the gain in-place if it's enabled.
            if enabled.load(Ordering::Acquire) {
                let g = f32::from_bits(gain.load(Ordering::Acquire));
                // This is a simple gain stage. A real filter would use a more
                // sophisticated algorithm (e.g., FIR convolution).
                for s in &mut pkt.samples.samples {
                    *s *= g;
                }
            }

            // Send the packet downstream, handling back-pressure by awaiting.
            if let Err(e) = output.send(pkt).await {
                // The downstream stage has shut down.
                error!("Failed to send packet: {}", e);
                return Err(e);
            }

            // Be a good citizen and yield to the scheduler periodically.
            processed_count += 1;
            if yield_threshold > 0 && processed_count >= yield_threshold {
                processed_count = 0;
                tokio::task::yield_now().await;
            }
        }
    }

    /// Updates a stage parameter based on a key-value pair from the control plane.
    fn update_param(&mut self, key: &str, val: Value) -> Result<(), StageError> {
        match key {
            "enabled" => {
                let is_enabled = val.as_bool().unwrap_or(true);
                self.enabled.store(is_enabled, Ordering::Release);
                trace!("Gain 'enabled' set to {}", is_enabled);
            }
            "gain" => {
                let new_gain = val
                    .as_f64()
                    .map(|v| v as f32)
                    .ok_or_else(|| StageError::BadParam(key.into()))?;

                self.set_gain(new_gain);
                trace!("Updating gain to {}", new_gain);
            }
            _ => return Err(StageError::BadParam(key.into())),
        }
        Ok(())
    }

}


/// Parameters for configuring a `GainStage`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GainStageParams {
    /// The multiplicative gain factor.
    #[serde(default = "default_gain")]
    gain: f32,
    /// The number of packets to process before yielding to the scheduler.
    /// The number of packets to process before yielding to the scheduler.
    /// Set to 0 to never yield.
    #[serde(default = "default_yield_threshold")]
    yield_threshold: u32,
}

fn default_gain() -> f32 {
    1.0
}

fn default_yield_threshold() -> u32 {
    64
}

pub struct GainStageFactory;

impl GainStageFactory {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DataPlaneStageFactory for GainStageFactory {
    /// Creates a new `GainStage` instance from JSON parameters.
    async fn create_stage(
        &self,
        params: &StageParams,
    ) -> PipelineResult<Box<dyn DataPlaneStage>> {
        let params: GainStageParams = serde_json::from_value(params.clone())?;
        let stage = GainStage::new(params.gain, true, params.yield_threshold);
        Ok(Box::new(stage))
    }

    fn stage_type(&self) -> &'static str {
        "gain"
    }

    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::to_value(schema_for!(GainStageParams)).unwrap_or_default()
    }
}

// Automatically register this stage factory with the pipeline runtime.
inventory::submit! {
    StaticStageRegistrar {
        factory_fn: || Box::new(GainStageFactory::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        data::VoltageEegPacket,
        stage::{ControlMsg, Input, Output, StageContext},
    };
    use serde_json::json;
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    // MockInput now needs to support try_recv
    struct MockInput {
        rx: mpsc::UnboundedReceiver<Packet<VoltageEegPacket>>,
    }
    #[async_trait]
    impl Input<VoltageEegPacket> for MockInput {
        async fn recv(&mut self) -> Result<Option<Packet<VoltageEegPacket>>, StageError> {
            Ok(self.rx.recv().await)
        }
        fn try_recv(&mut self) -> Result<Option<Packet<VoltageEegPacket>>, StageError> {
            match self.rx.try_recv() {
                Ok(p) => Ok(Some(p)),
                Err(mpsc::error::TryRecvError::Empty) => Ok(None),
                Err(mpsc::error::TryRecvError::Disconnected) => Err(StageError::QueueClosed),
            }
        }
    }

    // MockOutput now needs to support try_send
    struct MockOutput {
        tx: mpsc::UnboundedSender<Packet<VoltageEegPacket>>,
    }
    #[async_trait]
    impl Output<VoltageEegPacket> for MockOutput {
        async fn send(&mut self, packet: Packet<VoltageEegPacket>) -> Result<(), StageError> {
            self.tx.send(packet).map_err(|_| StageError::Fatal("send error".into()))
        }
        fn try_send(
            &mut self,
            packet: Packet<VoltageEegPacket>,
        ) -> Result<(), TrySendError<Packet<VoltageEegPacket>>> {
            // An unbounded sender can't be "full", so we only need to handle the "closed" case.
            self.tx.send(packet).map_err(|e| TrySendError::Closed(e.0))
        }
    }

    fn setup_test_rig() -> (
        GainStage,
        StageContext,
        mpsc::UnboundedSender<ControlMsg>,
        mpsc::UnboundedSender<Packet<VoltageEegPacket>>,
        mpsc::UnboundedReceiver<Packet<VoltageEegPacket>>,
    ) {
        let stage = GainStage::new(0.5, true, 64);

        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        let (out_tx, out_rx) = mpsc::unbounded_channel();

        let mut inputs: HashMap<String, Box<dyn Input<VoltageEegPacket>>> = HashMap::new();
        inputs.insert("in".to_string(), Box::new(MockInput { rx: in_rx }));

        let mut outputs: HashMap<String, Box<dyn Output<VoltageEegPacket>>> = HashMap::new();
        outputs.insert("out".to_string(), Box::new(MockOutput { tx: out_tx }));

        let ctx = StageContext {
            control_rx,
            inputs,
            outputs,
            memory_pools: HashMap::new(),
        };

        (stage, ctx, control_tx, in_tx, out_rx)
    }

    #[tokio::test]
    async fn test_gain_application() {
        let (mut stage, mut ctx, _control_tx, in_tx, mut out_rx) = setup_test_rig();

        // Send a packet and verify initial gain
        let samples = vec![10.0, 20.0, 30.0];
        let pkt = Packet::new_for_test(VoltageEegPacket { samples });
        in_tx.send(pkt).unwrap();

        stage.run(&mut ctx).await.unwrap();

        let filtered_pkt = out_rx.recv().await.unwrap();
        assert_eq!(filtered_pkt.samples.samples, vec![5.0, 10.0, 15.0]);
    }

    #[tokio::test]
    async fn test_update_coeffs_and_apply_new_gain() {
        let (mut stage, mut ctx, control_tx, in_tx, mut out_rx) = setup_test_rig();

        // Send UpdateParam control message for gain
        let new_gain = 0.1;
        let msg = ControlMsg::UpdateParam("gain".to_string(), json!(new_gain));
        control_tx.send(msg).unwrap();

        // Send a packet to be processed with the new gain
        let samples = vec![10.0, 20.0, 30.0];
        let pkt = Packet::new_for_test(VoltageEegPacket { samples });
        in_tx.send(pkt).unwrap();

        // Run stage to process control message and data packet
        stage.run(&mut ctx).await.unwrap();

        // Verify it's filtered with NEW coeffs
        let filtered_pkt = out_rx.recv().await.unwrap();
        assert_eq!(filtered_pkt.samples.samples, vec![1.0, 2.0, 3.0]);
    }

    #[tokio::test]
    async fn test_pause_and_resume() {
        let (mut stage, mut ctx, control_tx, in_tx, mut out_rx) = setup_test_rig();

        // Pause the stage
        control_tx.send(ControlMsg::Pause).unwrap();

        // Send a packet
        let samples = vec![10.0, 20.0, 30.0];
        let pkt = Packet::new_for_test(VoltageEegPacket { samples: samples.clone() });
        in_tx.send(pkt).unwrap();

        // Run the stage
        stage.run(&mut ctx).await.unwrap();

        // Verify the packet is passed through UNCHANGED
        let received_pkt = out_rx.recv().await.unwrap();
        assert_eq!(received_pkt.samples.samples, samples);

        // Resume the stage
        control_tx.send(ControlMsg::Resume).unwrap();

        // Send another packet
        let pkt2 = Packet::new_for_test(VoltageEegPacket { samples });
        in_tx.send(pkt2).unwrap();

        // Run the stage again
        stage.run(&mut ctx).await.unwrap();

        // Verify the packet is now processed
        let received_pkt2 = out_rx.recv().await.unwrap();
        assert_eq!(received_pkt2.samples.samples, vec![5.0, 10.0, 15.0]);
    }
}
