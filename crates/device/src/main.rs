use adc_daemon::plugin_supervisor::PluginSupervisor;
use eeg_sensor::raw::mock_eeg::MockDriver;
use eeg_sensor::AdcDriver;
use eeg_types::BridgeMsg;
use pipeline::data::Packet;
use pipeline::config::SystemConfig;
use pipeline::control::ControlCommand;
use pipeline::control::PipelineEvent;
use pipeline::graph::PipelineGraph;
use pipeline::registry::StageRegistry;
use pipeline::runtime::{run as pipeline_run, RuntimeMsg};
use pipeline::stage::StageContext;
use std::sync::mpsc as std_mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    env_logger::init();
    tracing::info!("EEG Daemon starting...");

    // --- Channel Setup ---
    // For WebSocket -> Control Plane
    let (control_tx, control_rx) = std_mpsc::channel();
    // For Pipeline -> Main Loop (Events)
    let (event_tx, event_rx) = std_mpsc::channel();
    // For Sensor Thread -> Main Loop (Data/Errors)
    let (bridge_tx, bridge_rx) = std_mpsc::channel();
    let bridge_rx = Arc::new(Mutex::new(bridge_rx));
    // For Main Loop -> Pipeline (Data)
    let (pipeline_data_tx, pipeline_data_rx) = std_mpsc::channel();

    // --- Configuration ---
    let initial_config: SystemConfig = SystemConfig {
        version: "1.0".to_string(),
        metadata: Default::default(),
        stages: vec![], // In a real app, load from file
    };

    // --- Plugin Supervisor ---
    let mut supervisor = PluginSupervisor::new();
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
        let mut driver = MockDriver::new(Default::default()).unwrap();
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
        // Handle messages from the sensor bridge
        if let Ok(bridge_msg) = bridge_rx.lock().unwrap().try_recv() {
            match bridge_msg {
                BridgeMsg::Data(packet) => {
                    let f32_packet = Packet {
                        header: packet.header,
                        samples: packet.samples.into_iter().map(|s| s as f32).collect(),
                    };
                    if pipeline_data_tx
                        .send(RuntimeMsg::Data(Box::new(f32_packet)))
                        .is_err()
                    {
                        tracing::error!("Failed to send data to pipeline; channel closed.");
                        break; // Exit if pipeline is gone
                    }
                }
                BridgeMsg::Error(e) => {
                    tracing::error!("Sensor error: {}", e);
                }
            }
        }

        // Handle events from the pipeline
        if let Ok(event) = event_rx.try_recv() {
            match event {
                PipelineEvent::ShutdownAck => {
                    tracing::info!("Pipeline acknowledged shutdown. Stopping sensor thread.");
                    // 2. Tell sensor thread to stop
                    stop_flag.store(true, Ordering::Relaxed);
                    break; // Exit main loop
                }
                PipelineEvent::TestStateChanged(val) => {
                    tracing::info!("Test state changed to: {}", val);
                }
                _ => {
                    tracing::debug!("Received unhandled pipeline event");
                }
            }
        }

        if shutdown_initiated {
            break;
        }

        // Check for shutdown signal
        if let Ok(_) = std_mpsc::channel::<()>().1.try_recv() {
            tracing::info!("Ctrl-C received, initiating shutdown.");
            // 1. Tell pipeline to shut down
            if control_tx.send(ControlCommand::Shutdown).is_err() {
                tracing::error!("Failed to send Shutdown command to pipeline.");
            }
            shutdown_initiated = true;
        }

        thread::sleep(Duration::from_millis(10));
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
