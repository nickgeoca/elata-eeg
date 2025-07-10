//! Voltage conversion stage for converting raw ADC values to voltages

use async_trait::async_trait;
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use serde_json::Value;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::sync::mpsc::error::TrySendError;
use tracing::{error, info, trace, warn};

use crate::ctrl_loop;
use crate::data::{Packet, PacketHeader, RawEegPacket, VoltageEegPacket};
use crate::error::{PipelineResult, StageError};
use crate::stage::{
    ControlMsg, DataPlaneStage, DataPlaneStageFactory, DataPlaneStageErased, ErasedDataPlaneStageFactory,
    ErasedStageContext, StageContext, StageParams, StaticStageRegistrar,
};

/// Stage that converts raw ADC values to voltage values
pub struct ToVoltageStage {
    /// Reference voltage for conversion
    vref: AtomicU32,
    /// ADC resolution (bits)
    adc_bits: u8,
    /// A flag to enable or disable the stage on-the-fly.
    enabled: std::sync::atomic::AtomicBool,
    /// The number of packets to process before yielding to the scheduler.
    /// A value of 0 means the stage will never yield.
    yield_threshold: u32,
    // Cached handles to avoid HashMap lookups in the hot path.
    // These are `None` until the first time `run` is called.
    input_rx: Option<Box<dyn crate::stage::Input<RawEegPacket>>>,
    output_tx: Option<Box<dyn crate::stage::Output<VoltageEegPacket>>>,
}

#[async_trait]
impl DataPlaneStage<RawEegPacket, VoltageEegPacket> for ToVoltageStage {
    /// The main execution loop for the voltage conversion stage.
    async fn run(&mut self, ctx: &mut StageContext<RawEegPacket, VoltageEegPacket>) -> Result<(), StageError> {
        // First, handle any incoming control messages.
        ctrl_loop!(self, ctx);

        // Then, enter the packet processing loop.
        self.process_packets(ctx).await
    }
}

impl ToVoltageStage {
    /// Create a new to_voltage stage
    pub fn new(vref: f32, adc_bits: u8, yield_threshold: u32) -> Self {
        let stage = Self {
            vref: AtomicU32::new(0), // Initialized properly in `set_vref`
            adc_bits,
            enabled: std::sync::atomic::AtomicBool::new(true),
            yield_threshold,
            input_rx: None,
            output_tx: None,
        };
        stage.set_vref(vref);
        stage
    }

    /// Set the reference voltage
    pub fn set_vref(&self, vref: f32) {
        self.vref.store(vref.to_bits(), Ordering::Release);
    }

    /// Safely gets the reference voltage using atomic operations.
    fn get_vref(&self) -> f32 {
        f32::from_bits(self.vref.load(Ordering::Acquire))
    }

    /// Convert raw ADC value to voltage
    fn raw_to_voltage(&self, raw_value: i32) -> f32 {
        let max_value = (1 << (self.adc_bits - 1)) - 1; // For signed values
        (raw_value as f32 / max_value as f32) * self.get_vref()
    }

    /// Gets mutable references to the input and output handles, initializing them
    /// on the first call. This avoids HashMap lookups in the hot path.
    #[cold]
    #[inline(always)]
    fn lazy_io<'a>(
        input_rx: &'a mut Option<Box<dyn crate::stage::Input<RawEegPacket>>>,
        output_tx: &'a mut Option<Box<dyn crate::stage::Output<VoltageEegPacket>>>,
        ctx: &'a mut StageContext<RawEegPacket, VoltageEegPacket>,
    ) -> (
        &'a mut dyn crate::stage::Input<RawEegPacket>,
        &'a mut dyn crate::stage::Output<VoltageEegPacket>,
    ) {
        if input_rx.is_none() {
            *input_rx = Some(
                ctx.inputs
                    .remove("in")
                    .unwrap_or_else(|| panic!("Input 'in' not found for to_voltage stage")),
            );
            *output_tx = Some(
                ctx.outputs
                    .remove("out")
                    .unwrap_or_else(|| panic!("Output 'out' not found for to_voltage stage")),
            );
        }
        (
            input_rx.as_mut().unwrap().as_mut(),
            output_tx.as_mut().unwrap().as_mut(),
        )
    }

    /// Efficiently processes all available packets in the input queue.
    async fn process_packets(&mut self, ctx: &mut StageContext<RawEegPacket, VoltageEegPacket>) -> Result<(), StageError> {
        let yield_threshold = self.yield_threshold;
        let vref = self.get_vref();
        let adc_bits = self.adc_bits;
        let max_value = (1 << (adc_bits - 1)) - 1; // For signed values
        
        let (input, output) = Self::lazy_io(&mut self.input_rx, &mut self.output_tx, ctx);
        let mut processed_count = 0;

        // Loop to drain the input queue.
        loop {
            let pkt = match input.try_recv()? {
                Some(p) => p,
                // The queue is empty, so we're done for now.
                None => return Ok(()),
            };

            // Convert raw samples to voltages
            let voltage_samples: Vec<f32> = pkt.samples.samples
                .iter()
                .map(|&raw| (raw as f32 / max_value as f32) * vref)
                .collect();

            // Create a new Packet with VoltageEegPacket type
            let output_pkt = Packet::new(pkt.header, VoltageEegPacket { samples: voltage_samples }, std::sync::Weak::<std::sync::Mutex<crate::data::MemoryPool<VoltageEegPacket>>>::new());

            // Send the packet downstream, handling back-pressure by awaiting.
            if let Err(e) = output.send(output_pkt).await {
                // The downstream stage has shut down.
                error!("Failed to send packet: {}", e);
                return Err(StageError::SendError(format!("Failed to send packet: {}", e)));
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
            "vref" => {
                let new_vref = val
                    .as_f64()
                    .map(|v| v as f32)
                    .ok_or_else(|| StageError::BadParam(key.into()))?;
                self.set_vref(new_vref);
                trace!("Updating vref to {}", new_vref);
            }
            "adc_bits" => {
                let new_adc_bits = val
                    .as_u64()
                    .map(|v| v as u8)
                    .ok_or_else(|| StageError::BadParam(key.into()))?;
                self.adc_bits = new_adc_bits;
                trace!("Updating adc_bits to {}", new_adc_bits);
            }
            _ => return Err(StageError::BadParam(key.into())),
        }
        Ok(())
    }
}

/// Parameters for configuring a `ToVoltageStage`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToVoltageStageParams {
    /// Reference voltage for ADC conversion
    #[serde(default = "default_vref")]
    vref: f32,
    /// ADC resolution in bits
    #[serde(default = "default_adc_bits")]
    adc_bits: u8,
    /// The number of packets to process before yielding to the scheduler.
    /// Set to 0 to never yield.
    #[serde(default = "default_yield_threshold")]
    yield_threshold: u32,
}

fn default_vref() -> f32 {
    4.5
}

fn default_adc_bits() -> u8 {
    24
}

fn default_yield_threshold() -> u32 {
    64
}

pub struct ToVoltageStageFactory;

impl ToVoltageStageFactory {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DataPlaneStageFactory<RawEegPacket, VoltageEegPacket> for ToVoltageStageFactory {
    /// Creates a new `ToVoltageStage` instance from JSON parameters.
    async fn create_stage(
        &self,
        params: &StageParams,
    ) -> PipelineResult<Box<dyn DataPlaneStage<RawEegPacket, VoltageEegPacket>>> {
        let params_value = serde_json::to_value(params)?;
        let params: ToVoltageStageParams = serde_json::from_value(params_value)?;
        let stage = ToVoltageStage::new(params.vref, params.adc_bits, params.yield_threshold);
        Ok(Box::new(stage))
    }

    fn stage_type(&self) -> &'static str {
        "to_voltage"
    }

    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::to_value(schema_for!(ToVoltageStageParams)).unwrap_or_default()
    }
}

#[async_trait]
impl DataPlaneStageErased for ToVoltageStage {
    async fn run_erased(&mut self, ctx: &mut dyn ErasedStageContext) -> Result<(), StageError> {
        let ctx = ctx.as_any_mut()
            .downcast_mut::<StageContext<RawEegPacket, VoltageEegPacket>>()
            .ok_or_else(|| StageError::InvalidContext("Expected StageContext<RawEegPacket, VoltageEegPacket>".to_string()))?;
        self.run(ctx).await
    }
}

#[async_trait]
impl ErasedDataPlaneStageFactory for ToVoltageStageFactory {
    async fn create_erased_stage(&self, params: &StageParams) -> PipelineResult<Box<dyn DataPlaneStageErased>> {
        let params_value = serde_json::to_value(params)?;
        let params: ToVoltageStageParams = serde_json::from_value(params_value)?;
        let stage = ToVoltageStage::new(params.vref, params.adc_bits, params.yield_threshold);
        Ok(Box::new(stage) as Box<dyn DataPlaneStageErased>)
    }

    fn stage_type(&self) -> &'static str {
        DataPlaneStageFactory::stage_type(self)
    }

    fn parameter_schema(&self) -> serde_json::Value {
        DataPlaneStageFactory::parameter_schema(self)
    }
}

// Automatically register this stage factory with the pipeline runtime.
inventory::submit! {
    StaticStageRegistrar {
        factory_fn: || Box::new(ToVoltageStageFactory::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        data::{Packet, RawEegPacket, VoltageEegPacket},
        stage::{ControlMsg, Input, Output, StageContext},
    };
    use serde_json::json;
    use std::collections::HashMap;
    use tokio::sync::mpsc;

    // MockInput now needs to support try_recv
    struct MockInput {
        rx: mpsc::UnboundedReceiver<Packet<RawEegPacket>>,
    }
    #[async_trait]
    impl Input<RawEegPacket> for MockInput {
        async fn recv(&mut self) -> Result<Option<Packet<RawEegPacket>>, StageError> {
            Ok(self.rx.recv().await)
        }
        fn try_recv(&mut self) -> Result<Option<Packet<RawEegPacket>>, StageError> {
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
        async fn send(&mut self, packet: Packet<VoltageEegPacket>) -> Result<(), TrySendError<Packet<VoltageEegPacket>>> {
            self.tx.send(packet).map_err(|e| TrySendError::Closed(e.0))
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
        ToVoltageStage,
        StageContext<RawEegPacket, VoltageEegPacket>,
        mpsc::UnboundedSender<ControlMsg>,
        mpsc::UnboundedSender<Packet<RawEegPacket>>,
        mpsc::UnboundedReceiver<Packet<VoltageEegPacket>>,
    ) {
        let stage = ToVoltageStage::new(4.5, 24, 64);

        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        let (out_tx, out_rx) = mpsc::unbounded_channel();

        let mut inputs: HashMap<String, Box<dyn Input<RawEegPacket>>> = HashMap::new();
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
    async fn test_voltage_conversion() {
        let (mut stage, mut ctx, _control_tx, in_tx, mut out_rx) = setup_test_rig();

        // Create test RawEegPacket
        let raw_samples = vec![8388607, 0, -8388607]; // Max, zero, min for 24-bit
        let pkt = Packet::new_for_test(RawEegPacket { samples: raw_samples.clone() });
        in_tx.send(pkt).unwrap();

        stage.run(&mut ctx).await.unwrap();

        let converted_pkt = out_rx.recv().await.unwrap();
        
        // Check that voltages were converted
        assert!((converted_pkt.samples.samples[0] - 4.5).abs() < 0.001); // Max value -> vref
        assert!(converted_pkt.samples.samples[1].abs() < 0.001); // Zero -> zero
        assert!((converted_pkt.samples.samples[2] + 4.5).abs() < 0.001); // Min value -> -vref
    }

    #[tokio::test]
    async fn test_update_params() {
        let (mut stage, mut ctx, control_tx, _in_tx, _out_rx) = setup_test_rig();

        // Send UpdateParam control message for vref
        let new_vref = 3.3;
        let msg = ControlMsg::UpdateParam("vref".to_string(), json!(new_vref));
        control_tx.send(msg).unwrap();

        // Run stage to process control message
        stage.run(&mut ctx).await.unwrap();
        assert!((stage.get_vref() - new_vref).abs() < 0.001);

        // Send UpdateParam control message for adc_bits
        let new_adc_bits = 16;
        let msg = ControlMsg::UpdateParam("adc_bits".to_string(), json!(new_adc_bits));
        control_tx.send(msg).unwrap();

        // Run stage to process control message
        stage.run(&mut ctx).await.unwrap();
        assert_eq!(stage.adc_bits, new_adc_bits);
    }

    #[tokio::test]
    async fn test_to_voltage_stage_factory_creation() {
        let factory = ToVoltageStageFactory::new();
        let mut params = HashMap::new();
        params.insert("vref".to_string(), json!(3.3));
        params.insert("adc_bits".to_string(), json!(16));

        let _stage = factory.create_stage(&params).await.unwrap();
        // If we get here without panicking, the stage was created successfully
    }

    #[tokio::test]
    async fn test_to_voltage_stage_factory_validation() {
        let factory = ToVoltageStageFactory::new();
        
        // Test invalid vref
        let mut params = HashMap::new();
        params.insert("vref".to_string(), json!(-1.0));
        assert!(factory.create_stage(&params).await.is_err());

        // Test invalid adc_bits
        params.clear();
        params.insert("adc_bits".to_string(), json!(7));
        assert!(factory.create_stage(&params).await.is_err());
    }

    #[test]
    fn test_parameter_schema() {
        let factory = ToVoltageStageFactory::new();
        let schema = <ToVoltageStageFactory as DataPlaneStageFactory<RawEegPacket, VoltageEegPacket>>::parameter_schema(&factory);
        assert!(schema.is_object());
        assert!(schema["properties"]["vref"].is_object());
        assert!(schema["properties"]["adc_bits"].is_object());
    }
}