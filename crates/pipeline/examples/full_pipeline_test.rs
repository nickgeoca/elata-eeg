//! Full pipeline test demonstrating actual data flow

use std::sync::Arc;
use std::time::Duration;
use pipeline::{
    PipelineConfig, PipelineRuntime, StageRegistry,
    register_builtin_stages,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    println!("Full Pipeline Data Flow Test");
    println!("============================");

    // Create stage registry and register built-in stages
    let mut registry = StageRegistry::new();
    register_builtin_stages(&mut registry);

    // Load pipeline configuration
    let config_json = include_str!("../../../examples/pipeline-config.json");
    let config = PipelineConfig::from_json(config_json)?;

    println!("Loaded pipeline: {}", config.metadata.name);
    println!("Pipeline stages: {}", config.stages.len());

    // Validate the configuration
    config.validate()?;
    println!("Pipeline configuration is valid");

    // Create runtime
    let mut runtime = PipelineRuntime::new(Arc::new(registry));

    // Load the pipeline
    runtime.load_pipeline(&config).await?;
    println!("Pipeline loaded successfully");

    // Start the pipeline
    println!("Starting pipeline...");
    match runtime.start().await {
        Ok(()) => println!("Pipeline started!"),
        Err(e) => {
            println!("Failed to start pipeline: {}", e);
            return Err(e.into());
        }
    }

    // Let it run for a few seconds to process some data
    println!("Running pipeline for 3 seconds...");
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Stop the pipeline
    println!("Stopping pipeline...");
    runtime.stop().await?;
    println!("Pipeline stopped");

    // Show final metrics
    let metrics = runtime.metrics().await;
    println!("Final metrics:");
    println!("  Items processed: {}", metrics.items_processed);
    println!("  Errors: {}", metrics.error_count);
    println!("  Uptime: {}ms", metrics.uptime_ms);

    println!("Full pipeline test completed successfully!");

    Ok(())
}