//! Built-in pipeline stages

pub mod acquire;
pub mod to_voltage;
pub mod filter;
pub mod websocket_sink;
pub mod csv_sink;

// Re-export stage implementations
pub use acquire::*;
pub use to_voltage::*;
pub use filter::*;
pub use websocket_sink::*;
pub use csv_sink::*;

use crate::stage::StageRegistry;

/// Register all built-in stages with the registry
pub fn register_builtin_stages(registry: &mut StageRegistry) {
    registry.register(AcquireStageFactory::new());
    registry.register(ToVoltageStageFactory::new());
    registry.register(FilterStageFactory::new());
    registry.register(WebSocketSinkFactory::new());
    registry.register(CsvSinkFactory::new());
}