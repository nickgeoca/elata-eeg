//! Built-in pipeline stages

use crate::registry::StageRegistry;

pub mod acquire;
pub mod csv_sink;
pub mod filter;
pub mod test_stage;
pub mod to_voltage;
pub mod websocket_sink;

pub use acquire::AcquireFactory;
pub use csv_sink::CsvSinkFactory;
pub use filter::FilterFactory;
pub use test_stage::StatefulTestStage;
pub use to_voltage::ToVoltageFactory;
pub use websocket_sink::WebsocketSinkFactory;

/// Registers all built-in stages with the provided registry.
pub fn register_builtin_stages(registry: &mut StageRegistry<f32, f32>) {
    registry.register("acquire", AcquireFactory::default());
    registry.register("to_voltage", ToVoltageFactory::default());
    registry.register("filter", FilterFactory::default());
    registry.register("csv_sink", CsvSinkFactory::default());
    registry.register(
        "websocket_sink",
        WebsocketSinkFactory::default(),
    );
}