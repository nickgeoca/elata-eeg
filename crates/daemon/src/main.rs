use adc_daemon::plugin_supervisor::PluginSupervisor;
use boards::create_driver;
use sensors::{AdcConfig, DriverType};
use eeg_types::BridgeMsg;
use sensors::types::ChipConfig;
use pipeline::config::SystemConfig;
use pipeline::control::{PipelineEvent, ControlCommand};
use pipeline::graph::PipelineGraph;
use pipeline::registry::StageRegistry;
use pipeline::runtime::{run as pipeline_run, RuntimeMsg};
use pipeline::stage::StageContext;
use crossbeam_channel::{bounded, select};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    env_logger::init();
    tracing::info!("EEG Daemon starting...");

    // --- Channel Setup ---
    // For WebSocket -> Control Plane
    let (_control_tx, _control_rx) = bounded::<ControlCommand>(10);
    // For Pipeline -> Main Loop (Events)
    let (event_tx, event_rx) = bounded(100);
    // For Sensor Thread -> Main Loop (Data/Errors)
    let (bridge_tx, bridge_rx) = bounded(100);
    // For Main Loop -> Pipeline (Data)
    let (pipeline_data_tx, pipeline_data_rx) = bounded(100);

    // --- Configuration ---
    let initial_config: SystemConfig = SystemConfig {
        version: "1.0".to_string(),
        metadata: Default::default(),
        stages: vec![], // In a real app, load from file
    };

    // --- Plugin Supervisor ---
    let _supervisor = PluginSupervisor::new();
    // In a real app, you would load plugins dynamically
    // supervisor.add_plugin(Box::new(MyPlugin::new()));

    // --- Pipeline Thread ---
    let pipeline_handle = {
        let mut registry = StageRegistry::new();
        pipeline::stages::register_builtin_stages(&mut registry);
        
        let event_tx_clone = event_tx.clone();
        let context = StageContext {
            event_tx: event_tx_clone,
        };
        let graph = PipelineGraph::build(&initial_config, &registry, context).unwrap();

        thread::spawn(move || {
            tracing::info!("Pipeline task started.");
            let result = pipeline_run(pipeline_data_rx, event_tx, graph);
            tracing::info!("Pipeline task finished with result: {:?}", result);
        })
    };

    // --- Sensor Thread ---
    let stop_flag = Arc::new(AtomicBool::new(false));
    let sensor_thread_handle = {
        // TODO: Load this from a config file
        let adc_config = AdcConfig {
            board_driver: DriverType::ElataV2,
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
                    drdy_pin: 25,
                },
            ],
            ..Default::default()
        };
        let mut driver = create_driver(adc_config).unwrap();
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
    let mut shutdown_initiated = false;
    loop {
        select! {
            recv(bridge_rx) -> bridge_msg => {
                match bridge_msg {
                    Ok(BridgeMsg::Data(packet)) => {
                        if pipeline_data_tx.send(RuntimeMsg::Data(packet)).is_err() {
                            tracing::error!("Failed to send data to pipeline; channel closed.");
                            break; // Exit if pipeline is gone
                        }
                    }
                    Ok(BridgeMsg::Error(e)) => {
                        tracing::error!("Sensor error: {}", e);
                    }
                    Err(_) => {
                        tracing::info!("Bridge channel disconnected.");
                        break;
                    }
                }
            },
            recv(event_rx) -> event => {
                match event {
                    Ok(PipelineEvent::ShutdownAck) => {
                        tracing::info!("Pipeline acknowledged shutdown. Stopping sensor thread.");
                        // 2. Tell sensor thread to stop
                        stop_flag.store(true, Ordering::Relaxed);
                        break; // Exit main loop
                    }
                    Ok(PipelineEvent::TestStateChanged(val)) => {
                        tracing::info!("Test state changed to: {}", val);
                    }
                    Ok(_) => {
                        tracing::debug!("Received unhandled pipeline event");
                    }
                    Err(_) => {
                        tracing::info!("Event channel disconnected.");
                        break;
                    }
                }
            },
            // TODO: Implement a proper shutdown signal handler
            // For now, we just break the loop on any channel disconnect
            default(Duration::from_secs(1)) => {
                // This timeout prevents the loop from spinning if all channels are empty
                // but not disconnected. In a real scenario, you might have a heartbeat
                // or other periodic tasks here.
                if shutdown_initiated {
                    break;
                }
            }
        }
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
