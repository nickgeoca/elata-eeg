//! Digital filtering stage for EEG data - NEW DATAPLANE IMPLEMENTATION
//
// Note: This is the new implementation based on the `DataPlaneStage` trait.
// The old `PipelineStage` logic has been removed. The factory and tests
// have been commented out and will need to be updated to work with this new design.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering::{self, Relaxed}};
use tracing::{info, error};

// TODO: These are placeholder imports based on the architecture documents.
// The actual paths may need to be adjusted once the new traits are integrated
// into the `crate::stage` and `crate::error` modules.
use crate::stage::{DataPlaneStage, StageContext, ControlMsg};
use crate::error::StageError;
use crate::data::{Packet, VoltageEegPacket}; // Assuming a VoltageEegPacket type exists

/// A high-performance, in-place filter stage for the data plane.
pub struct FilterStage {
    /// The filter coefficients.
    coeffs: Vec<f32>,
    /// A flag to enable or disable the filter on-the-fly.
    ///
    /// `Relaxed` ordering is sufficient because this is only ever accessed by the
    /// single data plane thread. There's no risk of race conditions with other
    /// threads that would require stricter orderings like `Acquire` or `Release`.
    enabled: AtomicBool,
}

#[async_trait]
impl DataPlaneStage for FilterStage {
    /// The main execution loop for the filter stage.
    async fn run(&mut self, ctx: &mut StageContext) -> Result<(), StageError> {
        self.handle_ctrl(ctx)?;
        self.process_packet(ctx).await
    }
}

impl FilterStage {
    /// Creates a new `FilterStage` with pre-calculated coefficients.
    pub fn new(coeffs: Vec<f32>, enabled: bool) -> Self {
        Self {
            coeffs,
            enabled: AtomicBool::new(enabled),
        }
    }

    /// Handles all incoming messages from the control plane.
    fn handle_ctrl(&mut self, ctx: &mut StageContext) -> Result<(), StageError> {
        while let Ok(msg) = ctx.control_rx.try_recv() {
            match msg {
                ControlMsg::Pause => {
                    info!("Filter stage paused");
                    self.enabled.store(false, Relaxed);
                }
                ControlMsg::Resume => {
                    info!("Filter stage resumed");
                    self.enabled.store(true, Relaxed);
                }
                ControlMsg::UpdateParam(key, val) => {
                    if let Err(e) = self.update_param(&key, val) {
                        error!("Failed to update parameter: {}", e);
                        // Optionally, notify the control plane of the error.
                    }
                }
                // Other control messages can be handled here.
                _ => {}
            }
        }
        Ok(())
    }

    /// Handles receiving, processing, and sending a single data packet.
    async fn process_packet(&mut self, ctx: &mut StageContext) -> Result<(), StageError> {
        // Wait for a packet to arrive from the input queue.
        let mut pkt = match ctx.inputs["in"].recv().await? {
            Some(p) => p,
            // If the input queue is empty, we are idle. Return Ok and the runtime
            // will call us again later.
            None => return Ok(()),
        };

        // Apply the filter in-place if it's enabled.
        if self.enabled.load(Relaxed) {
            let g = self.gain();
            // This is a simplified FIR filter for demonstration purposes.
            // A real implementation would use a more sophisticated algorithm.
            for s in &mut pkt.samples {
                *s *= g;
            }
        }

        // Send the (potentially modified) packet to the output queue.
        // Ownership of the packet is transferred to the next stage.
        ctx.outputs["out"].send(pkt).await?;

        Ok(())
    }

    /// Updates a stage parameter based on a key-value pair from the control plane.
    fn update_param(&mut self, key: &str, val: Value) -> Result<(), StageError> {
        match key {
            "enabled" => {
                let is_enabled = val.as_bool().unwrap_or(true);
                self.enabled.store(is_enabled, Relaxed);
                info!("Filter 'enabled' set to {}", is_enabled);
            }
            "coeffs" => {
                let new_coeffs: Vec<f32> = serde_json::from_value(val)
                    .map_err(|_| StageError::BadParam(key.into()))?;

                if new_coeffs.is_empty() {
                    return Err(StageError::Fatal("Filter coefficients cannot be empty".into()));
                }

                info!("Updating filter coefficients ({} taps)", new_coeffs.len());
                self.coeffs = new_coeffs;
            }
            _ => return Err(StageError::BadParam(key.into())),
        }
        Ok(())
    }

    /// Fast helper to get the gain coefficient, avoiding checks in the hot loop.
    #[inline(always)]
    fn gain(&self) -> f32 {
        // SAFETY: The `update_param` function ensures `coeffs` is never empty.
        // The debug_assert provides an additional layer of verification during testing.
        debug_assert!(!self.coeffs.is_empty(), "Filter coefficients cannot be empty");
        unsafe { *self.coeffs.get_unchecked(0) }
    }
}


use crate::stage::{StageFactory, StageParams};
use crate::error::PipelineResult;

/// Factory for creating `FilterStage` instances.
///
/// This factory is part of the **Control Plane**. It reads configuration from a JSON
/// object, calculates the necessary filter coefficients, and constructs the `FilterStage`.
pub struct FilterStageFactory;

impl FilterStageFactory {
    pub fn new() -> Self {
        Self
    }

    /// A placeholder for a real digital filter design function.
    /// In a real implementation, this would use a library like `scipy.signal` or a
    /// Rust equivalent to design a proper FIR or IIR filter.
    fn design_filter_coeffs(
        &self,
        lowpass: Option<f32>,
        highpass: Option<f32>,
        sample_rate: f32,
        order: usize,
    ) -> Vec<f32> {
        info!(
            "Designing filter: lowpass={:?}, highpass={:?}, order={}, sample_rate={}",
            lowpass, highpass, order, sample_rate
        );
        // Placeholder: This is NOT a real filter design. It just creates a simple
        // averaging filter. A real implementation would involve complex math.
        vec![1.0 / order as f32; order]
    }
}

#[async_trait]
impl StageFactory for FilterStageFactory {
    /// Creates a new `FilterStage` instance from JSON parameters.
    async fn create_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn DataPlaneStage>> {
        let lowpass = params.get("lowpass").and_then(|v| v.as_f64()).map(|v| v as f32);
        let highpass = params.get("highpass").and_then(|v| v.as_f64()).map(|v| v as f32);
        let order = params.get("order").and_then(|v| v.as_u64()).unwrap_or(4) as usize;

        // In a real system, the sample rate would likely come from a global
        // pipeline configuration context.
        let sample_rate = params.get("sample_rate").and_then(|v| v.as_f64()).unwrap_or(250.0) as f32;

        // The factory's core responsibility: translating user-friendly parameters
        // into the low-level coefficients the stage needs to operate.
        let coeffs = self.design_filter_coeffs(lowpass, highpass, sample_rate, order);

        let stage = FilterStage::new(coeffs, true);

        Ok(Box::new(stage))
    }

    fn stage_type(&self) -> &'static str {
        "filter"
    }

    fn parameter_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "lowpass": {
                    "type": "number",
                    "description": "Low-pass cutoff frequency in Hz.",
                    "minimum": 0.1
                },
                "highpass": {
                    "type": "number",
                    "description": "High-pass cutoff frequency in Hz.",
                    "minimum": 0.1
                },
                "order": {
                    "type": "integer",
                    "description": "Filter order.",
                    "minimum": 1,
                    "default": 4
                },
                "sample_rate": {
                    "type": "number",
                    "description": "The sample rate of the incoming data, in Hz.",
                    "default": 250.0
                }
            },
            "required": ["sample_rate"]
        })
    }
}

// TODO: Unit tests need to be rewritten for the new DataPlaneStage architecture.
// They will need to mock a StageContext and test the `run` method directly,
// including sending control messages and data packets.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Packet, VoltageEegPacket};
    use crate::stage::{ControlMsg, StageContext};
    use serde_json::json;
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    // A mock implementation of the Input trait for testing purposes.
    struct MockInput {
        rx: mpsc::UnboundedReceiver<Packet<VoltageEegPacket>>,
    }
    #[async_trait]
    impl crate::stage::Input<VoltageEegPacket> for MockInput {
        async fn recv(&mut self) -> Result<Option<Packet<VoltageEegPacket>>, StageError> {
            Ok(self.rx.recv().await)
        }
    }

    // A mock implementation of the Output trait for testing purposes.
    struct MockOutput {
        tx: mpsc::UnboundedSender<Packet<VoltageEegPacket>>,
    }
    #[async_trait]
    impl crate::stage::Output<VoltageEegPacket> for MockOutput {
        async fn send(&mut self, packet: Packet<VoltageEegPacket>) -> Result<(), StageError> {
            self.tx.send(packet).map_err(|_| StageError::Fatal("send error".into()))
        }
    }

    #[tokio::test]
    async fn test_update_coeffs_and_filter() {
        // 1. Setup stage and mock context
        let initial_coeffs = vec![0.5];
        let mut stage = FilterStage::new(initial_coeffs, true);

        let (mut control_tx, control_rx) = mpsc::unbounded_channel();
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel();

        let mut inputs: HashMap<String, Box<dyn crate::stage::Input<VoltageEegPacket>>> = HashMap::new();
        inputs.insert("in".to_string(), Box::new(MockInput { rx: in_rx }));

        let mut outputs: HashMap<String, Box<dyn crate::stage::Output<VoltageEegPacket>>> = HashMap::new();
        outputs.insert("out".to_string(), Box::new(MockOutput { tx: out_tx }));

        let mut ctx = StageContext {
            control_rx,
            inputs,
            outputs,
            // memory_pools is not used in this test
            memory_pools: HashMap::new(),
        };

        // 2. Send a packet and verify initial filtering
        let samples = vec![10.0, 20.0, 30.0];
        let pkt = Packet::new_for_test(VoltageEegPacket { samples });
        in_tx.send(pkt).unwrap();

        stage.run(&mut ctx).await.unwrap();

        let filtered_pkt = out_rx.recv().await.unwrap();
        assert_eq!(filtered_pkt.samples, vec![5.0, 10.0, 15.0]);

        // 3. Send UpdateParam control message for coefficients
        let new_coeffs = vec![0.1];
        let msg = ControlMsg::UpdateParam("coeffs".to_string(), json!(new_coeffs));
        control_tx.send(msg).unwrap();

        // Run stage to process control message (no data packet this time)
        stage.run(&mut ctx).await.unwrap();

        // 4. Send another packet and verify it's filtered with NEW coeffs
        let samples2 = vec![10.0, 20.0, 30.0];
        let pkt2 = Packet::new_for_test(VoltageEegPacket { samples: samples2 });
        in_tx.send(pkt2).unwrap();

        stage.run(&mut ctx).await.unwrap();

        let filtered_pkt2 = out_rx.recv().await.unwrap();
        assert_eq!(filtered_pkt2.samples, vec![1.0, 2.0, 3.0]);
    }
}
