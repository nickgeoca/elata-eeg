use std::{
    collections::HashMap,
    fs,
    sync::{Arc, Mutex},
};

use adc_daemon::plugin_supervisor::PluginSupervisor;
use api::{AppState, PipelineHandle};
use clap::{Arg, Command};
use pipeline::config::SystemConfig;
use pipeline::control::PipelineEvent;
use pipeline::executor::Executor;
use sensors::{
    mock_eeg::driver::MockDriver,
    types::{AdcConfig, AdcDriver, DriverError},
};
use boards::elata_v2::driver::ElataV2Driver;
use pipeline::graph::PipelineGraph;
use pipeline::registry::StageRegistry;
use tokio::sync::broadcast;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod server;

#[tokio::main]
async fn main() -> Result<(), DriverError> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "adc_daemon=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("EEG Daemon starting...");

    // --- Argument Parsing ---
    let matches = Command::new("eeg_daemon")
        .about("EEG data acquisition daemon")
        .arg(
            Arg::new("mock")
                .long("mock")
                .action(clap::ArgAction::SetTrue)
                .help("Use mock EEG data instead of real hardware"),
        )
        .get_matches();

    // --- Centralized State ---
    let (sse_tx, _) = broadcast::channel(100);
    let (event_tx, event_rx) = flume::bounded(100);

    let app_state = AppState {
        pipelines: Arc::new(Mutex::new(HashMap::new())),
        sse_tx: sse_tx.clone(),
        pipeline_handle: Arc::new(Mutex::new(None)),
        source_meta_cache: Arc::new(Mutex::new(None)),
    };

    // --- Plugin Supervisor ---
    let _supervisor = PluginSupervisor::new();

    // --- Default Pipeline Startup ---
    let config_path = "pipelines/default.yaml";
    let config_str = fs::read_to_string(config_path).map_err(|e| DriverError::IoError(e.to_string()))?;
    let initial_config: SystemConfig = serde_yaml::from_str(&config_str).map_err(|e| DriverError::ConfigurationError(e.to_string()))?;

    let mut registry = StageRegistry::new();
    pipeline::stages::register_builtin_stages(&mut registry);

    // --- Driver Initialization ---
    let use_mock = matches.get_flag("mock");
    let driver: Option<Arc<Mutex<Box<dyn AdcDriver>>>> = if use_mock {
        tracing::info!("Using mock EEG driver");
        let adc_config = AdcConfig::default(); // Use default for mock
        Some(Arc::new(Mutex::new(Box::new(MockDriver::new(adc_config).unwrap()))))
    } else {
        tracing::info!("Using ElataV2 hardware driver");
        // The AdcConfig should be sourced from a dedicated hardware configuration file
        // or have a sensible default. For now, we'll use a default config.
        let adc_config = AdcConfig {
            chips: vec![
                sensors::types::ChipConfig {
                    spi_bus: 0,
                    cs_pin: 7,
                    channels: vec![2],
                },
                sensors::types::ChipConfig {
                    spi_bus: 0,
                    cs_pin: 8,
                    channels: vec![1],
                },
            ],
            ..AdcConfig::default()
        };
        let mut driver_instance = ElataV2Driver::new(adc_config)?;
        driver_instance.initialize()?;
        Some(Arc::new(Mutex::new(Box::new(driver_instance))))
    };


    let graph =
        PipelineGraph::build(&initial_config, &registry, event_tx.clone(), None, &driver).unwrap();

    let (executor, input_tx, fatal_error_rx) = Executor::new(graph);
    tracing::info!("Default pipeline executor started.");

    // Store the handle to the running pipeline
    *app_state.pipeline_handle.lock().unwrap() = Some(PipelineHandle {
        id: "default".to_string(),
        executor: Some(executor),
        input_tx,
    });

    // Spawn a task to listen for fatal errors from the default pipeline
    let pipeline_handle_clone = app_state.pipeline_handle.clone();
    let sse_tx_clone = app_state.sse_tx.clone();
    tokio::spawn(async move {
        if let Ok(panic_payload) = fatal_error_rx.recv_async().await {
            tracing::error!("Fatal pipeline error detected in default pipeline. Shutting down.");

            let error_msg = if let Some(s) = panic_payload.downcast_ref::<&'static str>() {
                s.to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic payload".to_string()
            };

            if let Some(mut handle) = pipeline_handle_clone.lock().unwrap().take() {
                if let Some(executor) = handle.executor.take() {
                    executor.stop();
                }
            }

            let event = PipelineEvent::PipelineFailed {
                error: error_msg,
            };
            if let Ok(event_json) = serde_json::to_string(&event) {
                if sse_tx_clone.send(event_json).is_err() {
                    tracing::warn!("Failed to send pipeline failure SSE event.");
                }
            }
        }
    });

    // --- Event Forwarding Task ---
    // Forwards events from the pipeline to the SSE broadcast channel
    let event_forwarding_state = app_state.clone();
    tokio::spawn(async move {
        while let Ok(event) = event_rx.recv_async().await {
            // If this is the source ready event, cache its metadata
            if let PipelineEvent::SourceReady { meta } = &event {
                tracing::debug!("Caching SourceReady event metadata");
                let mut cache = event_forwarding_state.source_meta_cache.lock().unwrap();
                *cache = Some(meta.clone());
            }

            if let Ok(event_json) = serde_json::to_string(&event) {
                if sse_tx.send(event_json).is_err() {
                    tracing::warn!("Failed to send SSE event: no active subscribers");
                }
            } else {
                tracing::error!("Failed to serialize pipeline event");
            }
        }
        tracing::info!("Event forwarding task finished.");
    });

    // --- Server Thread ---
    let server_handle = tokio::spawn(server::run(app_state.clone()));

    // --- Graceful Shutdown ---
    tokio::signal::ctrl_c().await.map_err(|e| DriverError::IoError(e.to_string()))?;
    tracing::info!("Shutdown signal received. Stopping services...");

    if let Some(mut handle) = app_state.pipeline_handle.lock().unwrap().take() {
        if let Some(executor) = handle.executor.take() {
            executor.stop();
        }
    }

    server_handle.abort();
    tracing::info!("EEG Daemon stopped gracefully.");

    Ok(())
}

