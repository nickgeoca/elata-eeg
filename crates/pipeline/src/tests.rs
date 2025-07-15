//! Integration tests for the pipeline.

use crate::config::SystemConfig;
use crate::stages::register_builtin_stages;
use crate::registry::StageRegistry;
use std::sync::Arc;

use crate::control::{ControlCommand, PipelineEvent};
use eeg_types::data::Packet;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_full_static_pipeline() {
    // 1. Set up the stage registry
    let mut registry = StageRegistry::new();
    register_builtin_stages(&mut registry);
    let registry = Arc::new(registry);

    // 2. Define a pipeline configuration
    let config_json = r#"
    {
        "version": "1.0",
        "stages": [
            {
                "name": "acquire1",
                "type": "acquire",
                "params": { "sps": 500 }
            },
            {
                "name": "to_voltage1",
                "type": "to_voltage",
                "inputs": ["acquire1"]
            },
            {
                "name": "filter1",
                "type": "filter",
                "params": { "lowpass": 40.0 },
                "inputs": ["to_voltage1"]
            }
        ]
    }
    "#;
    let config: SystemConfig = serde_json::from_str(config_json).unwrap();

    // 3. Create channels for the runtime
    let (data_tx, data_rx) = mpsc::channel::<Packet<f32>>(10);
    let (control_tx, control_rx) = mpsc::channel::<ControlCommand>(10);
    let (event_tx, mut event_rx) = mpsc::channel::<PipelineEvent>(10);

    // 4. Run the pipeline in a separate task
    let pipeline_handle = tokio::spawn(crate::runtime::run(
        config,
        registry,
        data_rx,
        control_rx,
        event_tx,
    ));

    // 5. TODO: Inject data and assert output
    //    - This will require a way to get data into the 'acquire' stage.
    //    - It will also require a sink stage that can be inspected.
    //    For now, just send a shutdown command and wait for the pipeline to exit.
    control_tx.send(ControlCommand::Shutdown).await.unwrap();

    // 6. Wait for the pipeline to shut down
    let result = pipeline_handle.await.unwrap();
    assert!(result.is_ok());

    // 7. Check for the shutdown acknowledgement
    let event = event_rx.recv().await;
    assert!(matches!(event, Some(PipelineEvent::ShutdownAck)));
}
