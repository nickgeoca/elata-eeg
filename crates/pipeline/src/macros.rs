//! Utility macros for the pipeline crate.

/// A declarative macro for defining a pipeline stage.
///
/// This macro generates the boilerplate required for a stage, including:
/// - The main stage struct.
/// - A parameter struct with `serde` and `schemars` support.
/// - An implementation of the `Stage` trait.
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
///         my_state: u32,
///     },
///     init: |params| {
///         Self { my_state: 0 }
///     },
///     process: |self, pkt, ctx| -> Result<Option<Packet<OutputPacketType>>, StageError> {
///         // ... processing logic ...
///         Ok(Some(output_pkt))
///     },
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
        process: |$process_self:ident, $process_pkt:ident, $process_ctx:ident| $process_body:block
    ) => {
        // --- Parameter Struct Definition ---
        #[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
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
        }

        // --- Main Stage Logic Implementation ---
        impl $name {
            pub fn new(params: [<$name Params>]) -> Self {
                let user_init_fields = {
                    let $init_params = &params;
                    $init_body
                };
                Self {
                    $($param_name: params.$param_name.clone(),)*
                    $($field_name: user_init_fields.$field_name,)*
                }
            }
        }

        // --- Trait Implementations ---
        #[async_trait::async_trait]
        impl $crate::stage::Stage<$input_ty, $output_ty> for $name {
            async fn process(
                &mut self,
                packet: $crate::data::Packet<$input_ty>,
                ctx: &mut $crate::stage::StageContext,
            ) -> Result<Option<$crate::data::Packet<$output_ty>>, $crate::error::StageError> {
                let $process_self = self;
                let $process_pkt = packet;
                let $process_ctx = ctx;
                $process_body
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
