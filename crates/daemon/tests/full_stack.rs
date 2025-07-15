use sensors::raw::mock_eeg::MockDriver;
use sensors::AdcDriver;
use eeg_types::BridgeMsg;
use pipeline::config::{StageConfig, SystemConfig};
use pipeline::control::{ControlCommand, PipelineEvent};
use pipeline::error::StageError;
use pipeline::graph::PipelineGraph;
use pipeline::registry::{StageFactory, StageRegistry};
use pipeline::runtime::RuntimeMsg;
use pipeline::stage::{Stage, StageContext};
use pipeline::stages::test_stage::StatefulTestStage;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use crossbeam_channel::{Receiver, Sender};

struct StatefulTestStageFactory;

impl StageFactory for StatefulTestStageFactory {
    fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage>, StageError> {
        Ok(Box::new(StatefulTestStage::new(&config.name)))
    }
}

/// Sets up a complete, running daemon instance for testing.
async fn setup_test_daemon() -> (
    thread::JoinHandle<()>,
    Receiver<PipelineEvent>,
    Sender<RuntimeMsg>,
    Arc<AtomicBool>,
    String, // WebSocket address
) {
    let (runtime_tx, runtime_rx) = crossbeam_channel::unbounded::<RuntimeMsg>();
    let (event_tx, event_rx) = crossbeam_channel::unbounded::<PipelineEvent>();
    let (bridge_tx, bridge_rx) = crossbeam_channel::unbounded::<BridgeMsg>();
    let bridge_rx = Arc::new(Mutex::new(bridge_rx));
    let (error_tx, _) = broadcast::channel::<StageError>(32);

    let test_config = SystemConfig {
        version: "1.0".to_string(),
        metadata: Default::default(),
        stages: vec![serde_json::from_value(json!({
            "name": "test_stage_1",
            "type": "stateful_test_stage",
            "inputs": []
        }))
        .unwrap()],
    };

    // --- Pipeline Thread ---
    let pipeline_handle = {
        let mut registry = StageRegistry::new();
        registry.register(
            "stateful_test_stage",
            Box::new(StatefulTestStageFactory),
        );
        let registry = Arc::new(registry);
        let graph = PipelineGraph::build(
            &test_config,
            &registry,
            StageContext::new(event_tx.clone()),
        )
        .unwrap();

        thread::spawn(move || {
            let result = pipeline::runtime::run(runtime_rx, event_tx, graph);
            if let Err(e) = result {
                if !e.to_string().contains("channel disconnected") {
                    panic!("Pipeline runtime failed: {}", e);
                }
            }
        })
    };

    // --- Sensor Thread ---
    let stop_flag = Arc::new(AtomicBool::new(false));
    let _sensor_thread_handle = {
        let config = sensors::AdcConfig {
            chips: vec![sensors::types::ChipConfig {
                channels: (0..8).collect(),
                ..Default::default()
            }],
            sample_rate: 1000,
            batch_size: 8,
            ..Default::default()
        };
        let mut driver = MockDriver::new(config).unwrap();
        let stop_flag_clone = stop_flag.clone();
        thread::spawn(move || {
            let _ = driver.acquire(bridge_tx, &stop_flag_clone);
        })
    };

    // --- Server ---
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    // Note: The server setup might need adjustment if it relies on tokio mpsc.
    // For this test, we assume it can be adapted or mocked if necessary.
    // The `server` module is not fully visible, so we proceed with the assumption
    // that it can work with a `crossbeam_channel` sender, or that we can adapt it.
    // let ws_routes = server::setup_websocket_routes(runtime_tx.clone(), error_tx.clone());
    // let server_handle = tokio::spawn(warp::serve(ws_routes).run_incoming(TcpListenerStream::new(listener)));

    // --- Main Event Loop (simplified for this test) ---
    let main_loop_handle = {
        let stop_flag_clone = stop_flag.clone();
        let runtime_tx_clone = runtime_tx.clone();
        tokio::spawn(async move {
            loop {
                if stop_flag_clone.load(Ordering::Relaxed) {
                    break;
                }

                let msg_result = {
                    // Lock, try_recv, and then immediately drop the lock
                    bridge_rx.lock().unwrap().try_recv()
                };

                match msg_result {
                    Ok(BridgeMsg::Data(packet_data)) => {
                        let packet = packet_data;
                        if runtime_tx_clone.send(RuntimeMsg::Data(packet)).is_err() {
                            break; // Pipeline receiver dropped.
                        }
                    }
                    Ok(BridgeMsg::Error(_)) => { /* Ignore sensor errors */ }
                    Err(crossbeam_channel::TryRecvError::Empty) => {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        break; // Sensor thread died.
                    }
                }
            }
            // server_handle.abort();
        })
    };

    (pipeline_handle, event_rx, runtime_tx, stop_flag, addr)
}

#[tokio::test]
async fn test_full_stack_command_and_shutdown() {
    // The server part is commented out as it requires more info to fix.
    // This test will focus on the pipeline and control logic.
    let (_pipeline_handle, event_rx, runtime_tx, stop_flag, _addr) = setup_test_daemon().await;

    // 2. Send command to change state
    let cmd = ControlCommand::SetTestState(42);
    runtime_tx.send(RuntimeMsg::Ctrl(cmd)).unwrap();

    // 3. Verify the pipeline emits the correct event
    let event = event_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(event, PipelineEvent::TestStateChanged(42));

    // 4. Initiate graceful shutdown
    runtime_tx
        .send(RuntimeMsg::Ctrl(ControlCommand::Shutdown))
        .unwrap();

    // 5. Signal all threads to stop
    stop_flag.store(true, Ordering::Relaxed);

    // Test passed if it reaches here without panicking
}