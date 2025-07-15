//! Integration tests for the pipeline.

use crate::config::SystemConfig;
use crate::stages::register_builtin_stages;
use crate::registry::StageRegistry;
use std::sync::Arc;

use crate::control::{ControlCommand, PipelineEvent};
use crate::runtime::RuntimeMsg;
use crossbeam_channel;
use std::thread;

#[test]
fn test_full_static_pipeline() {
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
    let (tx, rx) = crossbeam_channel::unbounded::<RuntimeMsg>();
    let (event_tx, event_rx) = crossbeam_channel::unbounded::<PipelineEvent>();

    // 4. Build the graph
    let graph = crate::graph::PipelineGraph::build(
        &config,
        &registry,
        crate::stage::StageContext::new(event_tx.clone()),
    )
    .unwrap();

    // 5. Run the pipeline in a separate task
    let pipeline_handle = thread::spawn(move || crate::runtime::run(rx, event_tx, graph));

    // 6. Send a shutdown command
    tx.send(RuntimeMsg::Ctrl(ControlCommand::Shutdown))
        .unwrap();

    // 7. Wait for the pipeline to shut down
    let result = pipeline_handle.join().unwrap();
    assert!(result.is_ok());

    // 8. Check for the shutdown acknowledgement
    let event = event_rx.recv().unwrap();
    assert!(matches!(event, PipelineEvent::ShutdownAck));
}
