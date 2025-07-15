use adc_daemon::server;
use eeg_sensor::raw::mock_eeg::MockDriver;
use eeg_sensor::AdcDriver;
use eeg_types::{BridgeMsg, Packet, WsControlCommand};
use futures_util::SinkExt;
use pipeline::config::{StageConfig, SystemConfig};
use pipeline::control::{ControlCommand, PipelineEvent};
use pipeline::error::StageError;
use pipeline::registry::{StageFactory, StageRegistry};
use pipeline::stage::Stage;
use pipeline::stages::test_stage::StatefulTestStage;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc::{self, Receiver, Sender}};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use async_trait::async_trait;

struct StatefulTestStageFactory;

#[async_trait]
impl StageFactory<Value, Value> for StatefulTestStageFactory {
    async fn create(
        &self,
        config: &StageConfig,
    ) -> Result<Box<dyn Stage<Value, Value>>, StageError> {
        Ok(Box::new(StatefulTestStage::new(&config.name)))
    }
}

/// Sets up a complete, running daemon instance for testing.
///
/// This function constructs and runs all the components of the device daemon:
/// - A mock sensor driver thread
/// - A pipeline thread with a stateful test stage
/// - A WebSocket server
/// - The main event loop to orchestrate everything
///
/// It returns handles and channels necessary to interact with and inspect the daemon.
async fn setup_test_daemon() -> (
    tokio::task::JoinHandle<()>,
    Receiver<PipelineEvent>,
    mpsc::Sender<ControlCommand>,
    Arc<AtomicBool>,
    String, // WebSocket address
) {
    let (control_tx, control_rx) = mpsc::channel(32);
    let (event_tx, event_rx) = mpsc::channel(32);
    let (bridge_tx, bridge_rx) = std_mpsc::channel::<BridgeMsg>();
    let bridge_rx = Arc::new(Mutex::new(bridge_rx));
    let (pipeline_data_tx, pipeline_data_rx) = mpsc::channel::<Packet<Value>>(32);
    let (error_tx, _) = broadcast::channel(32);

    // A minimal config that includes our test stage
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
    let _pipeline_handle = {
        let mut registry = StageRegistry::<Value, Value>::new();
        registry.register("stateful_test_stage", StatefulTestStageFactory);
        let registry = Arc::new(registry);
        tokio::spawn(async move {
            let result = pipeline::runtime::run(
                test_config,
                registry,
                pipeline_data_rx,
                control_rx, // Directly use the mpsc receiver
                event_tx,
            )
            .await;
            if let Err(e) = result {
                // The panic is expected if the receiver disconnects, which it will when the test finishes.
                if !e.to_string().contains("disconnected") {
                    panic!("Pipeline runtime failed: {}", e);
                }
            }
        })
    };

    // --- Sensor Thread ---
    let stop_flag = Arc::new(AtomicBool::new(false));
    let _sensor_thread_handle = {
        let mut config: eeg_sensor::AdcConfig = Default::default();
        config.batch_size = config.channels.len();
        let mut driver = MockDriver::new(config).unwrap();
        let stop_flag_clone = stop_flag.clone();
        thread::spawn(move || {
            let _ = driver.acquire(bridge_tx, &stop_flag_clone);
        })
    };

    // --- Server ---
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let ws_routes = server::setup_websocket_routes(control_tx.clone(), error_tx.clone());
    let server_handle = tokio::spawn(warp::serve(ws_routes).run_incoming(TcpListenerStream::new(listener)));

    // --- Main Event Loop ---
    let main_loop_handle = {
        let stop_flag_clone = stop_flag.clone();
        tokio::spawn(async move {
            let mut packet_to_send: Option<Packet<Value>> = None;
            loop {
                if stop_flag_clone.load(Ordering::Relaxed) {
                    break;
                }

                if let Some(packet) = &packet_to_send {
                    // State 1: We have a packet to send.
                    tokio::select! {
                        res = pipeline_data_tx.send(packet.clone()) => {
                            if res.is_ok() {
                                packet_to_send = None; // Clear after successful send.
                            } else {
                                break; // Pipeline receiver dropped.
                            }
                        },
                        _ = tokio::time::sleep(Duration::from_millis(10)) => {
                            // Timeout, continue loop to re-check stop_flag
                        }
                    }
                } else {
                    // State 2: We need to receive a packet.
                    tokio::select! {
                        res = tokio::task::spawn_blocking({
                            let bridge_rx = Arc::clone(&bridge_rx);
                            move || bridge_rx.lock().unwrap().try_recv()
                        }) => {
                            match res {
                                Ok(Ok(BridgeMsg::Data(packet))) => {
                                    packet_to_send = Some(Packet {
                                        header: packet.header,
                                        samples: packet.samples.into_iter().map(|s| json!(s)).collect(),
                                    });
                                },
                                Ok(Ok(BridgeMsg::Error(_))) => { /* Ignore sensor errors */ },
                                Ok(Err(std_mpsc::TryRecvError::Empty)) => {
                                    // No data, yield before trying again.
                                    tokio::time::sleep(Duration::from_millis(1)).await;
                                },
                                Ok(Err(std_mpsc::TryRecvError::Disconnected)) | Err(_) => {
                                    break; // Sensor thread died.
                                }
                            }
                        },
                        _ = tokio::time::sleep(Duration::from_millis(10)) => {
                             // Timeout, continue loop to re-check stop_flag
                        }
                    }
                }
            }
            server_handle.abort();
        })
    };

    (main_loop_handle, event_rx, control_tx, stop_flag, addr)
}

#[tokio::test]
async fn test_full_stack_command_and_shutdown() {
    let (main_loop_handle, mut event_rx, control_tx, stop_flag, addr) = setup_test_daemon().await;

    // 1. Connect WebSocket client
    let (mut ws_client, _) = connect_async(format!("ws://{}/control", addr))
        .await
        .expect("Failed to connect");

    // 2. Send command to change state
    let cmd = WsControlCommand::SetTestState { value: 42 };
    let cmd_json = serde_json::to_string(&cmd).unwrap();
    ws_client
        .send(Message::Text(cmd_json))
        .await
        .unwrap();

    // Add a small delay to allow the pipeline to process the command
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 3. Verify the pipeline emits the correct event
    let event = tokio::task::spawn_blocking(move || {
        event_rx.recv_timeout(Duration::from_secs(2))
    })
    .await
    .expect("Event receive task panicked")
    .expect("Timeout waiting for event");

    assert_eq!(event, PipelineEvent::TestStateChanged(42));

    // 4. Initiate graceful shutdown
    control_tx.send(ControlCommand::Shutdown).await.unwrap();

    // 5. Signal all threads to stop
    stop_flag.store(true, Ordering::Relaxed);

    // 6. Wait for the main loop to finish
    main_loop_handle
        .await
        .expect("Main loop task panicked");

    // Test passed if it reaches here without panicking
}