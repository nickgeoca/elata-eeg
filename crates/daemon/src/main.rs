use adc_daemon::plugin_supervisor::PluginSupervisor;
use boards::create_driver;
use clap::{Arg, Command};
use eeg_types::BridgeMsg;
use flume::Selector;
use pipeline::config::SystemConfig;
use pipeline::control::{ControlCommand, PipelineEvent};
use pipeline::executor::Executor;
use pipeline::graph::PipelineGraph;
use pipeline::registry::StageRegistry;
use sensors::types::ChipConfig;
use sensors::{AdcConfig, DriverType};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
    let (_control_tx, _control_rx) = flume::bounded::<ControlCommand>(10);
    // For Pipeline -> Main Loop (Events)
    let (event_tx, event_rx) = flume::bounded(10);
    // For Sensor Thread -> Main Loop (Data/Errors)
    let (bridge_tx, bridge_rx) = flume::bounded::<BridgeMsg>(10);

    // --- Configuration ---
    let config_path = "crates/daemon/e2e_test_pipeline.yaml";
    let config_str = fs::read_to_string(config_path)?;
    let initial_config: SystemConfig = serde_yaml::from_str(&config_str)?;

    // --- Plugin Supervisor ---
    let _supervisor = PluginSupervisor::new();
    // In a real app, you would load plugins dynamically
    // supervisor.add_plugin(Box::new(MyPlugin::new()));

    // --- Pipeline Thread ---
    let (pipeline_input_tx, pipeline_handle) = {
        let mut registry = StageRegistry::new();
        pipeline::stages::register_builtin_stages(&mut registry);

        let graph =
            PipelineGraph::build(&initial_config, &registry, event_tx.clone(), None).unwrap();

        let (executor, input_tx) = Executor::new(graph);

        let handle = thread::spawn(move || {
            tracing::info!("Pipeline executor started.");
            // The executor's internal state will run until stop() is called.
            // For now, we just let the thread run. The stop() method will be called
            // during graceful shutdown.
            thread::park(); // Park the thread until unparked during shutdown
            executor.stop();
            tracing::info!("Pipeline executor stopped.");
        });

        (input_tx, handle)
    };

    // --- Sensor Thread ---
    let stop_flag = Arc::new(AtomicBool::new(false));
    let sensor_thread_handle = {
        // TODO: Load this from a config file
        // TODO: Load this from a config file
        let mut adc_config = AdcConfig {
            board_driver: DriverType::ElataV1,
            chips: vec![
                ChipConfig {
                    channels: (0..8).collect(),
                    gain: 24.0,
                    spi_bus: 0,
                    cs_pin: 0,
                    drdy_pin: 25,
                },
                ChipConfig {
                    channels: (0..8).collect(),
                    gain: 24.0,
                    spi_bus: 0,
                    cs_pin: 1,
                    drdy_pin: 26, // Use a different DRDY pin for the second chip
                },
            ],
            ..Default::default()
        };
        if matches.get_flag("mock") {
            adc_config.board_driver = DriverType::MockEeg;
        }
        let mut driver = create_driver(adc_config)
            .expect("Failed to create driver - check board driver type is supported");
        driver.initialize().unwrap();
        let stop_flag_clone = stop_flag.clone();
        thread::spawn(move || {
            tracing::info!("Sensor thread started.");
            if let Err(e) = driver.acquire(bridge_tx, &stop_flag_clone) {
                tracing::error!("Sensor thread exited with error: {}", e);
            }
            tracing::info!("Sensor thread finished.");
        })
    };


    // --- Main Event Loop ---
    loop {
        Selector::new()
            .recv(&bridge_rx, |bridge_msg| {
                match bridge_msg {
                    Ok(BridgeMsg::Data(packet)) => {
                        if pipeline_input_tx.send(Arc::new(packet.into())).is_err() {
                            tracing::error!("Failed to send data to pipeline; channel closed.");
                        }
                    }
                    Ok(BridgeMsg::Error(e)) => {
                        tracing::error!("Sensor error: {}", e);
                    }
                    Err(_) => {
                        tracing::info!("Bridge channel disconnected.");
                    }
                }
            })
            .recv(&event_rx, |event| {
                match event {
                    Ok(PipelineEvent::ShutdownAck) => {
                        tracing::info!("Pipeline acknowledged shutdown. Stopping sensor thread.");
                        // 2. Tell sensor thread to stop
                        stop_flag.store(true, Ordering::Relaxed);
                    }
                    Ok(PipelineEvent::TestStateChanged(val)) => {
                        tracing::info!("Test state changed to: {}", val);
                    }
                    Err(_) => {
                        tracing::info!("Event channel disconnected.");
                    }
                }
            })
            .wait_timeout(std::time::Duration::from_secs(1)).ok();
    }

    // --- Graceful Shutdown ---
    tracing::info!("Waiting for sensor thread to join...");
    if let Err(e) = sensor_thread_handle.join() {
        tracing::error!("Sensor thread panicked: {:?}", e);
    }

    tracing::info!("Waiting for pipeline thread to join...");
    if let Err(e) = pipeline_handle.join() {
        tracing::error!("Pipeline thread panicked: {:?}", e);
    }

    tracing::info!("EEG Daemon stopped gracefully.");
    Ok(())
}
