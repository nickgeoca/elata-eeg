use adc_daemon::server;
use eeg_sensor::raw::mock_eeg::MockDriver;
use eeg_sensor::AdcDriver;
use eeg_types::{BridgeMsg, Packet};
use pipeline::config::SystemConfig;
use pipeline::control::ControlCommand;
use pipeline::control::PipelineEvent;
use pipeline::registry::StageRegistry;
use pipeline::runtime::run as pipeline_run;
use std::sync::mpsc as std_mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    env_logger::init();
    tracing::info!("EEG Daemon starting...");

    // --- Channel Setup ---
    // For WebSocket -> Control Plane
    let (control_tx, control_rx) = mpsc::channel(32);
    // For Pipeline -> Main Loop (Events)
    let (event_tx, mut event_rx) = mpsc::channel(128);
    // For Sensor Thread -> Main Loop (Data/Errors)
    let (bridge_tx, bridge_rx) = std_mpsc::channel();
    let bridge_rx = Arc::new(Mutex::new(bridge_rx));
    // For Main Loop -> Pipeline (Data)
    let (pipeline_data_tx, pipeline_data_rx) = mpsc::channel::<Packet<f32>>(1024);
    // For broadcasting errors to WebSocket clients
    let (error_tx, _) = broadcast::channel(32);

    // --- Configuration ---
    let initial_config: SystemConfig = SystemConfig {
        version: "1.0".to_string(),
        metadata: Default::default(),
        stages: vec![], // In a real app, load from file
    };

    // --- Pipeline Thread ---
    let pipeline_handle = {
        let mut registry = StageRegistry::<f32, f32>::new();
        pipeline::stages::register_builtin_stages(&mut registry);
        let registry = Arc::new(registry);
        let event_tx_clone = event_tx.clone();

        tokio::spawn(async move {
            tracing::info!("Pipeline task started.");
            let result = pipeline_run(
                initial_config.clone(),
                registry,
                pipeline_data_rx,
                control_rx,
                event_tx_clone,
            )
            .await;
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

    // --- Server ---
    let ws_routes = server::setup_websocket_routes(control_tx.clone(), error_tx.clone());
    let mut server_handle: Option<JoinHandle<()>> = Some(tokio::spawn(warp::serve(ws_routes).run(([0, 0, 0, 0], 8080))));
    tracing::info!("WebSocket server listening on ws://0.0.0.0:8080");

    // --- Main Event Loop ---
    let mut shutdown_initiated = false;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c(), if !shutdown_initiated => {
                tracing::info!("Ctrl-C received, initiating shutdown.");
                // 1. Tell pipeline to shut down
                if control_tx.send(ControlCommand::Shutdown).await.is_err() {
                    tracing::error!("Failed to send Shutdown command to pipeline.");
                }
                shutdown_initiated = true;
            },

            // Handle messages from the sensor bridge
            bridge_msg_task_result = tokio::task::spawn_blocking({
                let bridge_rx = Arc::clone(&bridge_rx);
                move || bridge_rx.lock().unwrap().recv()
            }) => {
                match bridge_msg_task_result {
                    Ok(Ok(BridgeMsg::Data(packet))) => {
                        let f32_packet = Packet {
                            header: packet.header,
                            samples: packet.samples.into_iter().map(|s| s as f32).collect(),
                        };
                        if pipeline_data_tx.send(f32_packet).await.is_err() {
                            tracing::error!("Failed to send data to pipeline; channel closed.");
                            break; // Exit if pipeline is gone
                        }
                    }
                    Ok(Ok(BridgeMsg::Error(e))) => {
                        tracing::error!("Sensor error: {}", e);
                        if error_tx.send(e.clone()).is_err() {
                            tracing::warn!("Failed to broadcast sensor error to clients.");
                        }
                    }
                    Ok(Err(_)) => { // RecvError
                        tracing::warn!("Bridge channel disconnected. Main loop will exit.");
                        break;
                    }
                    Err(_) => { // JoinError
                        tracing::error!("Sensor bridge task panicked!");
                        break;
                    }
                }
            },

            // Handle events from the pipeline
            Some(event) = event_rx.recv() => {
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
                    event => {
                        tracing::debug!("Received unhandled pipeline event: {:?}", event);
                    }
                }
            },

            // Handle server completion
            result = async { server_handle.as_mut().unwrap().await }, if server_handle.is_some() => {
                tracing::warn!("Server task completed unexpectedly: {:?}", result);
                server_handle.take(); // Prevent polling a completed handle
                break;
            }
        }
    }

    // --- Graceful Shutdown ---
    tracing::info!("Waiting for sensor thread to join...");
    if let Err(e) = sensor_thread_handle.join() {
        tracing::error!("Sensor thread panicked: {:?}", e);
    }

    tracing::info!("Waiting for pipeline thread to join...");
    if let Err(e) = pipeline_handle.await {
        tracing::error!("Pipeline task panicked: {:?}", e);
    }

    tracing::info!("EEG Daemon stopped gracefully.");
    Ok(())
}
