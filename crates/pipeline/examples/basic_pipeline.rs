//! Basic pipeline example demonstrating the new architecture

use std::sync::Arc;
use pipeline::{
    PipelineConfig, PipelineRuntime, StageRegistry,
    register_builtin_stages,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging (commented out for now)
    // tracing_subscriber::fmt::init();

    println!("Pipeline Graph Architecture Example");
    println!("===================================");

    // Create stage registry and register built-in stages
    let mut registry = StageRegistry::new();
    register_builtin_stages(&mut registry);

    println!("Registered stage types: {:?}", registry.stage_types());

    // Load pipeline configuration
    let config_json = include_str!("../../../examples/pipeline-config.json");
    let config = PipelineConfig::from_json(config_json)?;

    println!("Loaded pipeline: {}", config.metadata.name);
    println!("Pipeline stages: {}", config.stages.len());

    // Validate the configuration
    config.validate()?;
    println!("Pipeline configuration is valid");

    // Get topological order
    let topo_order = config.topological_order()?;
    println!("Execution order: {:?}", 
             topo_order.iter().map(|s| &s.name).collect::<Vec<_>>());

    // Create runtime
    let mut runtime = PipelineRuntime::new(Arc::new(registry));

    // Load the pipeline
    runtime.load_pipeline(&config).await?;
    println!("Pipeline loaded successfully");

    // Note: We don't actually start the pipeline in this example
    // because the stages would try to process real data
    println!("Pipeline ready to start (not starting in this example)");

    // Show pipeline graph information
    if let Some(graph) = runtime.graph().await {
        let graph_guard = graph.read().await;
        let stats = graph_guard.stats();
        println!("Graph stats: {:?}", stats);
    }

    println!("Example completed successfully!");

    Ok(())
}