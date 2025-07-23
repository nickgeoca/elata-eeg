mod api;
mod server;

use adc_daemon::plugin_supervisor::PluginSupervisor;
use clap::{Arg, Command};
use pipeline::config::SystemConfig;
use pipeline::control::PipelineEvent;
use pipeline::executor::Executor;
use pipeline::graph::PipelineGraph;
use pipeline::registry::StageRegistry;
use std::fs;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "adc_daemon=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("EEG Daemon starting...");

    // Parse command line arguments
    let _matches = Command::new("eeg_daemon")
        .about("EEG data acquisition daemon")
        .arg(
            Arg::new("mock")
                .long("mock")
                .action(clap::ArgAction::SetTrue)
                .help("Use mock EEG data instead of real hardware"),
        )
        .get_matches();

    // --- Channel Setup ---
    let (event_tx, event_rx) = flume::bounded(10);

    // --- Server Thread ---
    let server_handle = tokio::spawn(async {
        if let Err(e) = server::run().await {
            tracing::error!("Server error: {}", e);
        }
    });

    // --- Configuration ---
    let config_path = "crates/daemon/e2e_test_pipeline.yaml";
    let config_str = fs::read_to_string(config_path)?;
    let initial_config: SystemConfig = serde_yaml::from_str(&config_str)?;

    // --- Plugin Supervisor ---
    let _supervisor = PluginSupervisor::new();

    // --- Pipeline Setup ---
    let mut registry = StageRegistry::new();
    pipeline::stages::register_builtin_stages(&mut registry);

    let graph =
        PipelineGraph::build(&initial_config, &registry, event_tx.clone(), None).unwrap();

    let (executor, _input_tx) = Executor::new(graph);
    tracing::info!("Pipeline executor started.");

    // --- Main Event Loop ---
    tracing::info!("EEG Daemon is running. Press Ctrl+C to exit.");
    let _ = event_rx.recv();

    // --- Graceful Shutdown ---
    tracing::info!("Shutdown signal received. Stopping executor...");
    executor.stop();

    // Shutdown the server
    server_handle.abort();

    tracing::info!("EEG Daemon stopped gracefully.");
    Ok(())
}

