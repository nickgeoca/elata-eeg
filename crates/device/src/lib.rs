// Re-export modules for library use
pub mod config;
pub mod server;
pub mod driver_handler;
pub mod connection_manager;
pub mod pid_manager;
pub mod plugin_manager;
pub mod elata_emu_v1;

// Event-driven architecture modules
pub mod event;
pub mod event_bus;
pub mod plugin;