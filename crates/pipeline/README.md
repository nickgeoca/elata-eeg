# Pipeline Graph Architecture

This crate implements a dataflow graph (DAG) architecture for EEG data processing, replacing the event-bus-based plugin system with explicit pipeline stages and data flow contracts.

## Overview

The pipeline system provides:

- **Graph-Based Pipelines**: Pipelines are modeled as directed acyclic graphs (DAGs), allowing stages to be shared, fanned out, and referenced by multiple endpoints.
- **Explicit Data Contracts**: Each sink (CSV, WebSocket, etc.) specifies the exact data schema/fields it expects, ensuring clarity and interoperability.
- **Runtime Introspection & Control**: The system exposes pipeline configs, stage parameters, and runtime state via API for transparency and UI integration.
- **Stage-Level Locking**: Locking and mutability are managed at the pipeline stage instance level—each stage is unique, even if it performs the same task for different outputs.
- **Versioning & Error Handling**: Pipeline configs are versioned for reproducibility, and error handling strategies are defined for robust operation.

## Architecture

### Core Components

1. **PipelineStage Trait**: Core trait that all pipeline stages must implement
2. **StageRegistry**: Registry for stage factories that can create stage instances
3. **PipelineGraph**: Represents the dataflow structure with stages and edges
4. **PipelineRuntime**: Executes pipeline graphs with proper lifecycle management
5. **PipelineConfig**: JSON-serializable configuration for defining pipelines

### Built-in Stages

- **acquire**: Data acquisition from EEG sensors
- **to_voltage**: Convert raw ADC values to voltages
- **filter**: Digital filtering (lowpass, highpass, notch)
- **websocket_sink**: Stream data to WebSocket clients
- **csv_sink**: Record data to CSV files

## Example Pipeline

```json
{
  "version": "1.0",
  "metadata": {
    "name": "EEG Processing Pipeline",
    "description": "Example pipeline for EEG data acquisition, filtering, and output"
  },
  "stages": [
    {
      "name": "acquire1",
      "type": "acquire",
      "params": {"sps": 500, "gain": 24, "channels": 8}
    },
    {
      "name": "to_voltage1",
      "type": "to_voltage",
      "params": {"vref": 4.5, "adc_bits": 24},
      "inputs": ["acquire1"]
    },
    {
      "name": "filter1",
      "type": "filter",
      "params": {"lowpass": 40.0, "highpass": 0.5},
      "inputs": ["to_voltage1"]
    },
    {
      "name": "display_ws",
      "type": "websocket_sink",
      "params": {
        "endpoint": "ws://filtered_data",
        "fields": ["timestamp", "filtered_channels"],
        "format": "json"
      },
      "inputs": ["filter1"]
    },
    {
      "name": "csv_filtered",
      "type": "csv_sink",
      "params": {
        "path": "filtered.csv",
        "fields": ["timestamp", "filtered_channels"]
      },
      "inputs": ["filter1"]
    }
  ]
}
```

This creates a pipeline where:
1. Data is acquired from sensors
2. Raw ADC values are converted to voltages
3. Digital filtering is applied
4. Filtered data is both streamed via WebSocket AND recorded to CSV

## Usage

```rust
use std::sync::Arc;
use pipeline::{
    PipelineConfig, PipelineRuntime, StageRegistry,
    register_builtin_stages,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create stage registry and register built-in stages
    let mut registry = StageRegistry::new();
    register_builtin_stages(&mut registry);

    // Load pipeline configuration
    let config = PipelineConfig::from_json(config_json)?;
    config.validate()?;

    // Create and start runtime
    let mut runtime = PipelineRuntime::new(Arc::new(registry));
    runtime.load_pipeline(&config).await?;
    runtime.start().await?;

    // Pipeline is now running...

    runtime.stop().await?;
    Ok(())
}
```

## Running the Example

```bash
cargo run --example basic_pipeline
```

## Key Benefits

1. **Transparency**: Every stage and parameter is explicit and versioned with each session
2. **Reproducibility**: All settings (acquisition, filtering, endpoints) are saved, enabling exact reproduction of experiments
3. **Extensibility**: New endpoints (e.g., CSV, WebSocket, OSC) and processing stages can be added without major refactoring
4. **Efficiency**: Shared stages (e.g., filtering) are computed once, with outputs fanned out to multiple endpoints

## Migration from Plugin System

The new pipeline system replaces the event-bus-based plugin architecture with explicit dataflow graphs. Key differences:

- **Before**: Plugins communicated via events on a shared bus
- **After**: Stages are connected via explicit channels in a DAG
- **Before**: Implicit data flow and dependencies
- **After**: Explicit data contracts and dependency management
- **Before**: Runtime plugin discovery and loading
- **After**: Compile-time stage registration with runtime configuration

## Future Work

- Hot-swapping of pipeline configurations
- Visual pipeline editor
- Advanced error recovery strategies
- Performance optimizations for high-throughput scenarios


# Misc
### What your current code already does

| Feature                                        | Status in the code you showed                                                                                                                                                                                       |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Fan-out / TEE**                              | **Built-in.** Every stage publishes with a `tokio::sync::broadcast::Sender`.  Each downstream edge simply calls `sender.subscribe()` to get its own `Receiver`.  That *is* the TEE. No extra stage required.        |
| **Multiple downstream consumers per stage**    | **Works today.**  You can point two, three, or twenty child stages at the same parent—each gets its own subscription and sees every packet.                                                                         |
| **Multiple independent sensors (roots)**       | **Also supported.**  A “sensor” is just another source stage with its own broadcast channel.  Nothing stops you from instantiating `ads1299_board_1`, `ads1299_board_2`, etc., inside the same `PipelineRuntime`.   |
| **Cross-sensor fusion**                        | **Not yet first-class.**  You’d need to write an explicit *fan-in* stage (e.g., `AlignAndZip`) that takes two receivers in its `inputs: [...]` array, waits for matching timestamps, and outputs a combined packet. |
| **Hot reconfig / prune / shared-prefix reuse** | **Planned, not implemented.**  The code you posted sets up the pieces (`shutdown_rx`, multi-input vector), but you still have to add ref-counting and the control-plane walker to make dynamic attach/detach safe.  |

---

### How to use it as a TEE right now

```toml
# pipeline.toml
[[stages]]
name  = "adc"
type  = "Ads1299Source"
params = { board = 0 }

[[stages]]
name   = "plotter"
type   = "GuiSink"
inputs = ["adc"]          # <-- subscribe to adc

[[stages]]
name   = "recorder"
type   = "CsvSink"
inputs = ["adc"]          # <-- another independent subscribe
```

At runtime:

```
adc ─► broadcast ─┬► plotter
                  └► recorder
```

Nothing extra to configure; the dispatcher gives each sink its own buffer.

---

### Adding a second sensor

```toml
[[stages]]
name  = "adc1"
type  = "Ads1299Source"
params = { board = 0 }

[[stages]]
name  = "adc2"
type  = "Ads1299Source"
params = { board = 1 }

# Separate plots / logs
[[stages]]  # GUI
name   = "plot1"
type   = "GuiSink"
inputs = ["adc1"]

[[stages]]  # CSV
name   = "rec1"
type   = "CsvSink"
inputs = ["adc1"]

[[stages]]  # GUI
name   = "plot2"
type   = "GuiSink"
inputs = ["adc2"]

[[stages]]  # CSV
name   = "rec2"
type   = "CsvSink"
inputs = ["adc2"]
```

That spawns two completely independent tee chains inside the same runtime.

---

### If you need a **combined** stream

1. **Create a fan-in stage**:

```rust
pub struct AlignAndZip { /* … */ }

#[async_trait]
impl Stage<Packet<f32>, Packet<Combined>> for AlignAndZip {
    async fn process(&mut self,
        packet: Packet<f32>,
        ctx: &mut StageContext,
        port: usize) -> Result<Option<Packet<Combined>>, StageError> {
        // store per-port packet until you have one from each;
        // then emit fused packet
    }
}
```

2. Wire it in the config:

```toml
[[stages]]
name   = "zip"
type   = "AlignAndZip"
inputs = ["adc1", "adc2"]

[[stages]]
name   = "plot_zip"
type   = "GuiSink"
inputs = ["zip"]
```

Now you have both per-sensor sinks *and* a fused stream, all still with one broadcast per edge.

---

### Bottom line

* **TEE-friendliness:** already there—every stage’s broadcast sender *is* a tee.
* **Multi-sensor:** just declare more source stages; they coexist fine.
* **Next step (only if needed):** write small fan-in/zip stages for cross-sensor DSP, or move on to the control-plane/ref-count work if you truly need live graph mutation.

So yes—your current architecture is well-suited to simple fan-out and multiple sensors without adding a single line of boilerplate.

# Two Architectures
| Decision                              | Payoff                                                                                                                                                                                                          | Cost                                               |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| **Static, config-driven graph**       | • Zero runtime headaches: start, run, stop.<br>• Easier to test—build once, exhaustively verify paths.<br>• Simple tee fan-out already lets you display *and* record simultaneously.                            | • Config changes require a restart (milliseconds). |
| **Arc-meta, self-describing packets** | • Every stage stays stateless (no global sensor table).<br>• Multi-sensor always correct—no “channel-1 belongs to which board?” bugs.<br>• Future proof: a dynamic runtime just passes the same packets around. | • 8–16 bytes extra pointer per packet (tiny)       |
