use adc_daemon::plugin_supervisor::PluginSupervisor;
use clap::{Arg, Command};
use pipeline::config::SystemConfig;
use pipeline::control::PipelineEvent;
use pipeline::executor::Executor;
use pipeline::graph::PipelineGraph;
use pipeline::registry::StageRegistry;
use std::fs;
use std::thread;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Parse command line arguments
    let matches = Command::new("eeg_daemon")
        .about("EEG data acquisition daemon")
        .arg(Arg::new("mock")
            .long("mock")
            .action(clap::ArgAction::SetTrue)
            .help("Use mock EEG data instead of real hardware"))
        .get_matches();

    // Initialize logging
    env_logger::init();
    tracing::info!("EEG Daemon starting...");

    // --- Channel Setup ---
    // For WebSocket -> Control Plane
    // For Pipeline -> Main Loop (Events)
    let (event_tx, event_rx) = flume::bounded(10);
    // --- Configuration ---
    let config_path = "crates/daemon/e2e_test_pipeline.yaml";
    let config_str = fs::read_to_string(config_path)?;
    let initial_config: SystemConfig = serde_yaml::from_str(&config_str)?;

    // --- Plugin Supervisor ---
    let _supervisor = PluginSupervisor::new();
    // In a real app, you would load plugins dynamically
    // supervisor.add_plugin(Box::new(MyPlugin::new()));

    // --- Pipeline Thread ---
    // --- Pipeline Setup ---
    let mut registry = StageRegistry::new();
    pipeline::stages::register_builtin_stages(&mut registry);

    let graph =
        PipelineGraph::build(&initial_config, &registry, event_tx.clone(), None).unwrap();

    let (executor, _input_tx) = Executor::new(graph);
    tracing::info!("Pipeline executor started.");

    // --- Main Event Loop ---
    // The main loop now waits for a shutdown signal (like Ctrl+C)
    // or for the pipeline to terminate on its own.
    tracing::info!("EEG Daemon is running. Press Ctrl+C to exit.");

    // This will block until the pipeline's event sender is dropped or the program is interrupted.
    let _ = event_rx.recv();

    // --- Graceful Shutdown ---
    tracing::info!("Shutdown signal received. Stopping executor...");
    executor.stop();

    tracing::info!("EEG Daemon stopped gracefully.");
    Ok(())
}

