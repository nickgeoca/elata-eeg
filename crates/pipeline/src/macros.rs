//! Utility macros for the pipeline crate.

/// A macro to simplify the handling of control messages in a stage's run loop.
///
/// This macro processes all pending messages from the control receiver,
/// handling common messages like `Pause` and `Resume` automatically. It
/// delegates `UpdateParam` messages to the provided stage's `update_param`
/// method.
#[macro_export]
macro_rules! ctrl_loop {
    ($self:ident, $ctx:ident) => {
        while let Ok(msg) = $ctx.control_rx.try_recv() {
            match msg {
                $crate::stage::ControlMsg::Pause => {
                    tracing::trace!("Stage paused");
                    $self.enabled.store(false, std::sync::atomic::Ordering::Release);
                }
                $crate::stage::ControlMsg::Resume => {
                    tracing::trace!("Stage resumed");
                    $self.enabled.store(true, std::sync::atomic::Ordering::Release);
                }
                $crate::stage::ControlMsg::UpdateParam(key, val) => {
                    if let Err(e) = $self.update_param(&key, val) {
                        tracing::error!("Failed to update parameter: {}", e);
                    }
                }
                _ => {
                    tracing::warn!("Received unknown control message: {:?}", msg);
                }
            }
        }
    };
}

/// A declarative macro for defining a pipeline stage.
///
/// This macro generates all the boilerplate required for a stage, including:
/// - The main stage struct.
/// - A parameter struct with `serde` and `schemars` support.
/// - `DataPlaneStage` and `DataPlaneStageErased` trait implementations.
/// - A factory for creating the stage from configuration.
/// - `inventory::submit!` registration for auto-discovery.
///
/// # Example
///
/// ```ignore
/// stage_def! {
///     name: MyStage,
///     inputs: InputPacketType,
///     outputs: OutputPacketType,
///     params: {
///         my_param: f32 = 1.0,
///     },
///     fields: {
///         my_state: AtomicU32,
///     },
///     init: |params| {
///         Self { my_state: AtomicU32::new(0) }
///     },
///     process: |self, pkt, ctx| -> Result<Packet<OutputPacketType>, StageError> {
///         // ... processing logic ...
///         Ok(output_pkt)
///     },
///     update_param: |self, key, val| {
///         // ... custom param update logic ...
///         Ok(())
///     }
/// }
/// ```
#[macro_export]
macro_rules! stage_def {
    (
        name: $name:ident,
        inputs: $input_ty:ty,
        outputs: $output_ty:ty,
        params: {
            $(
                $(#[$param_meta:meta])*
                $param_name:ident: $param_ty:ty = $param_default:expr
            ),* $(,)?
        },
        fields: {
            $(
                $(#[$field_meta:meta])*
                $field_name:ident: $field_ty:ty
            ),* $(,)?
        },
        init: |$init_params:ident| $init_body:block,
        process: |$process_self:ident, $process_pkt:ident: Packet<$process_input_ty:ty>, $process_ctx:ident: &mut StageContext<_, _>| -> Result<$process_output_ty:ty, $process_err_ty:ty> $process_body:block,
        update_param: |$update_self:ident, $update_key:ident: &str, $update_val:ident: Value| $update_body:block
    ) => {
        // --- Parameter Struct Definition ---
        #[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct [<$name Params>] {
            $(
                $(#[$param_meta])*
                #[serde(default = $crate::macros::stringify_path!([<$name _default_ $param_name>]))]
                pub $param_name: $param_ty,
            )*
        }

        $(
            fn [<$name _default_ $param_name>]() -> $param_ty {
                $param_default
            }
        )*

        // --- Stage Struct Definition ---
        pub struct $name {
            // Parameters are stored directly in the struct
            $(
                pub $param_name: $param_ty,
            )*
            // Custom state fields
            $(
                $(#[$field_meta])*
                pub $field_name: $field_ty,
            )*
            // Common stage state
            enabled: std::sync::atomic::AtomicBool,
            input_rx: Option<Box<dyn $crate::stage::Input<$input_ty>>>,
            output_tx: Option<Box<dyn $crate::stage::Output<$output_ty>>>,
        }

        // --- Main Stage Logic Implementation ---
        impl $name {
            pub fn new($($param_name: $param_ty,)*) -> Self {
                let params = [<$name Params>] { $($param_name,)* };
                let mut stage = Self {
                    $($param_name: params.$param_name,)*
                    // This is a bit of a hack to get around init. We create dummy fields,
                    // then call the user's init block to create the real ones.
                    $($field_name: {
                        // Use `MaybeUninit` to avoid creating a real instance of potentially non-default types.
                        // This is safe because we immediately overwrite it in the next step.
                        unsafe { std::mem::MaybeUninit::uninit().assume_init() }
                    },)*
                    enabled: std::sync::atomic::AtomicBool::new(true),
                    input_rx: None,
                    output_tx: None,
                };

                // The user's init logic provides the actual fields
                let initialized_fields = {
                    let $init_params = &params;
                    let temp_self = $init_body;
                    $(
                        stage.$field_name = temp_self.$field_name;
                    )*
                };

                stage
            }

            #[cold]
            #[inline(always)]
            fn lazy_io<'a>(
                input_rx: &'a mut Option<Box<dyn $crate::stage::Input<$input_ty>>>,
                output_tx: &'a mut Option<Box<dyn $crate::stage::Output<$output_ty>>>,
                ctx: &'a mut $crate::stage::StageContext<$input_ty, $output_ty>,
            ) -> (
                &'a mut dyn $crate::stage::Input<$input_ty>,
                &'a mut dyn $crate::stage::Output<$output_ty>,
            ) {
                if input_rx.is_none() {
                    *input_rx = Some(
                        ctx.inputs
                            .remove("in")
                            .unwrap_or_else(|| panic!("Input 'in' not found for stage")),
                    );
                    *output_tx = Some(
                        ctx.outputs
                            .remove("out")
                            .unwrap_or_else(|| panic!("Output 'out' not found for stage")),
                    );
                }
                (
                    input_rx.as_mut().unwrap().as_mut(),
                    output_tx.as_mut().unwrap().as_mut(),
                )
            }

            async fn process_packets(&mut self, ctx: &mut $crate::stage::StageContext<$input_ty, $output_ty>) -> Result<(), $crate::error::StageError> {
                let (input, output) = Self::lazy_io(&mut self.input_rx, &mut self.output_tx, ctx);
                let mut processed_count = 0;

                loop {
                    if !self.enabled.load(std::sync::atomic::Ordering::Acquire) {
                        tokio::task::yield_now().await;
                        continue;
                    }

                    let pkt = match input.try_recv()? {
                        Some(p) => p,
                        None => return Ok(()),
                    };

                    // --- User's Processing Logic ---
                    let result_pkt = {
                        let $process_self = self;
                        let $process_pkt = pkt;
                        let $process_ctx = ctx;
                        $process_body
                    }?;
                    // --- End User's Logic ---

                    if let Err(e) = output.send(result_pkt).await {
                        tracing::error!("Downstream stage channel closed: {}", e);
                        return Err($crate::error::StageError::SendError(format!("Failed to send packet: {}", e)));
                    }

                    processed_count += 1;
                    if self.yield_threshold > 0 && processed_count >= self.yield_threshold {
                        processed_count = 0;
                        tokio::task::yield_now().await;
                    }
                }
            }

            fn update_param(&mut self, key: &str, val: serde_json::Value) -> Result<(), $crate::error::StageError> {
                let $update_self = self;
                let $update_key = key;
                let $update_val = val;
                $update_body
            }
        }

        // --- Trait Implementations ---
        #[async_trait::async_trait]
        impl $crate::stage::DataPlaneStage<$input_ty, $output_ty> for $name {
            async fn run(&mut self, ctx: &mut $crate::stage::StageContext<$input_ty, $output_ty>) -> Result<(), $crate::error::StageError> {
                $crate::ctrl_loop!(self, ctx);
                self.process_packets(ctx).await
            }
        }

        #[async_trait::async_trait]
        impl $crate::stage::DataPlaneStageErased for $name {
            async fn run_erased(&mut self, ctx: &mut dyn $crate::stage::ErasedStageContext) -> Result<(), $crate::error::StageError> {
                let ctx = ctx.as_any_mut()
                    .downcast_mut::<$crate::stage::StageContext<$input_ty, $output_ty>>()
                    .ok_or_else(|| $crate::error::StageError::InvalidContext(format!("Expected StageContext<{}, {}>", stringify!($input_ty), stringify!($output_ty))))?;
                self.run(ctx).await
            }
        }

        // --- Factory Definition ---
        pub struct [<$name Factory>];
        impl [<$name Factory>] {
            pub fn new() -> Self { Self }
        }

        #[async_trait::async_trait]
        impl $crate::stage::DataPlaneStageFactory<$input_ty, $output_ty> for [<$name Factory>] {
            async fn create_stage(
                &self,
                params: &$crate::stage::StageParams,
            ) -> $crate::error::PipelineResult<Box<dyn $crate::stage::DataPlaneStage<$input_ty, $output_ty>>> {
                let params_value = serde_json::to_value(params)?;
                let params: [<$name Params>] = serde_json::from_value(params_value)?;
                let stage = $name::new($(params.$param_name,)*);
                Ok(Box::new(stage))
            }

            fn stage_type(&self) -> &'static str {
                stringify!([<$name:lower>])
            }

            fn parameter_schema(&self) -> serde_json::Value {
                serde_json::to_value(schemars::schema_for!([<$name Params>])).unwrap_or_default()
            }
        }

        #[async_trait::async_trait]
        impl $crate::stage::ErasedDataPlaneStageFactory for [<$name Factory>] {
            async fn create_erased_stage(&self, params: &$crate::stage::StageParams) -> $crate::error::PipelineResult<Box<dyn $crate::stage::DataPlaneStageErased>> {
                let params_value = serde_json::to_value(params)?;
                let params: [<$name Params>] = serde_json::from_value(params_value)?;
                let stage = $name::new($(params.$param_name,)*);
                Ok(Box::new(stage) as Box<dyn $crate::stage::DataPlaneStageErased>)
            }

            fn stage_type(&self) -> &'static str {
                $crate::stage::DataPlaneStageFactory::stage_type(self)
            }

            fn parameter_schema(&self) -> serde_json::Value {
                $crate::stage::DataPlaneStageFactory::parameter_schema(self)
            }
        }

        // --- Static Registration ---
        inventory::submit! {
            $crate::stage::StaticStageRegistrar {
                factory_fn: || Box::new([<$name Factory>]::new()),
            }
        }
    };
}

// Helper macro to stringify a path, used for default value functions.
#[macro_export]
macro_rules! stringify_path {
    ($path:path) => {
        stringify!($path)
    };
}
