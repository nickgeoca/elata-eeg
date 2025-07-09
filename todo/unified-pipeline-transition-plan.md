# Unified Pipeline Architecture Transition Plan

## 1. Executive Summary
The project is actively transitioning to a new hybrid sensor pipeline architecture designed for high-performance, real-time data processing. This architecture separates concerns into a **Control Plane** (for configuration and management) and a **Data Plane** (for zero-copy, lock-free data flow). While significant progress has been made, particularly with the conceptual design and initial stage implementations like the `GainStage`, a deeper review of the codebase reveals a clear distinction between the "old" and "new" architectural components. Achieving full adoption requires a comprehensive migration strategy for data structures, stages, runtimes, and frontend integration. The goal is to fully transition from the existing system to the robust, dynamic, and performant new architecture.

## 2. Architectural Overview (New vs. Old)

### New Hybrid Pipeline Architecture
The new architecture is characterized by:
*   **Control Plane:** Manages `PipelineGraph` and `PipelineRuntime` using standard `tokio` channels for configuration and low-frequency events.
*   **Data Plane:** A high-performance path for real-time sensor data, built on:
    *   **Zero-Copy Data Flow:** Utilizes `MemoryPool`s and `Packet<T>` smart pointers to eliminate runtime allocations and manage buffer lifetimes.
    *   **Lock-Free Communication:** Employs bounded, lock-free queues (`rtrb` or `crossbeam`) for inter-stage communication.
    *   **Data-Driven Execution:** Pipeline stages are activated by data arrival, with non-blocking run loops.
    *   **Separation of Concerns:** Data path isolated from control path.
*   **Dynamic Plugin System:** Aims for hot-reloadable, dynamically loaded Rust plugins with associated UI components, defined by a stable `pipeline-abi` crate.

### Contrast with the "Old Way"
The "old way" refers to the existing `plugins/` folder structure and the current `PipelineRuntime` and `PipelineGraph` which operate on `PipelineData` (an `enum` wrapping `Arc<T>` for data sharing). While `brain_waves_fft` already includes a `plugin.toml` and `ui/` directory, indicating a move towards the new plugin system, the integration and dynamic loading mechanisms for all plugins need to be fully implemented as per the `unified-pipeline-architecture2.md` document. The current system uses `Arc<[T]>` for zero-copy data sharing, but lacks the `MemoryPool`-based buffer management of the new `Packet<T>`. Frontend UI components are currently directly imported, not dynamically loaded.

## 3. Assessment of Current Components

### 3.1. Data Acquisition (Sensors & Devices)
*   **`crates/sensors/src/ads1299/`**: This directory contains modules (`acquisition.rs`, `driver.rs`, `spi.rs`, `registers.rs`) responsible for direct interaction with the ADS1299 sensor, handling SPI communication and raw data acquisition.
*   **`crates/device/src/elata_emu_v1/`**: This appears to be an emulation or a specific device interface for an EEG system (`eeg_system/`), potentially providing a standardized or simulated way to interact with EEG hardware.
*   **`crates/device/src/connection_manager.rs` and `crates/device/src/server.rs`**: These files form part of a central daemon managing WebSocket connections for data streaming and control. The `ConnectionManager` dispatches `SensorEvent::WebSocketBroadcast` events in a specific binary format to subscribed UI clients. The `server.rs` sets up `warp` routes for data, configuration, and command WebSockets.

**Integration with New Data Plane:** The output from `ads1299` and `elata_emu_v1` must be seamlessly integrated into the new Data Plane. This means:
    *   An "Acquisition Stage" (e.g., `crates/pipeline/src/stages/acquire.rs`) needs to be fully implemented to interface with the `ads1299` driver, acquiring raw data and converting it into `Packet<T>` instances from a `MemoryPool`.
    *   The `elata_emu_v1` (if it's a data source) should also be adapted to produce `Packet<T>` for the Data Plane.
    *   The existing WebSocket data flow, currently relying on `SensorEvent::WebSocketBroadcast` and binary serialization within `eeg_types`, will need to be handled by the new `WebSocketSink` stage, which consumes `Packet<T>` and produces data in the expected format.

### 3.2. Pipeline Stages
*   **`crates/pipeline/src/stages/filter.rs` (GainStage):** This serves as an excellent example of a Data Plane stage implemented according to the new architecture's principles: zero-copy processing, hot-reloadable parameters, correct concurrency, and efficient run-loops. It demonstrates the use of `DataPlaneStage` trait, `AtomicU32`, `AtomicBool`, and `Ordering` for safe concurrent access.
*   **`crates/pipeline/src/stage.rs`:** This file defines both the "old" `PipelineStage` trait (operating on `PipelineData`) and the "new" `DataPlaneStage` trait (operating on `Packet<T>`). It also defines the `Input` and `Output` traits for `Packet<T>` communication, and the `DataPlaneStageFactory`/`DataPlaneStageRegistry` for the new system. The `MemoryPool` is currently a `type alias` to `()`, indicating incomplete implementation.
*   **What's needed:**
    *   **Migration of existing stages:** Any existing processing stages (e.g., from the `plugins/` folder that are not yet Data Plane compliant) need to be refactored to implement the `DataPlaneStage` trait and adhere to the zero-copy, lock-free principles.
    *   **Development of new stages:** New stages required for the full data flow (e.g., FFT, specific filters, artifact removal) must be developed following the `GainStage` pattern.
    *   **`StageContext` and I/O handling:** Ensure all stages correctly utilize `StageContext` for accessing inputs, outputs, and memory pools, with robust handling of generic types.

### 3.3. Plugin System
*   **Current state of `plugins/` folder:** Contains `basic_voltage_filter`, `brain_waves_fft`, and `csv_recorder`. `brain_waves_fft` already has `plugin.toml` and a `ui/` directory, indicating a move towards the new plugin architecture's manifest and UI bundling concepts.
*   **`crates/eeg_types/src/event.rs`:** Defines `EegPacket`, `FilteredEegPacket`, and `FftPacket` using `Arc<[T]>` for data sharing, and the `SensorEvent` enum as a central event bus.
*   **`kiosk/src/components/EegDataVisualizer.tsx`:** Currently uses a direct import of `FftRenderer` from `plugins/brain_waves_fft/ui/FftRenderer`, which is a significant deviation from the proposed dynamic plugin UI loading.
*   **Integration of new dynamic plugin loading:** The detailed plan in `todo/unified-pipeline-architecture2.md` outlines the dynamic loading system. This needs to be fully implemented, including:
    *   **`pipeline-abi` crate:** This crucial component defines the stable, versioned contract between the host and plugins. It needs to be finalized and used by both the host and all plugins.
    *   **`PluginManager`:** The host application needs a `PluginManager` to scan the `plugins/` directory, load dynamic libraries (`.so`, `.dll`), and call their `register_factories` function.
    *   **Frontend Integration:** The Kiosk UI needs to dynamically discover and load plugin UI components based on `plugin.toml` manifests, replacing direct imports with dynamic loading mechanisms.
*   **Addressing "plugin loading" questions:**
    *   **`git clone` then `cargo build`:** The plan in `unified-pipeline-architecture2.md` suggests cloning the plugin repo *inside* `plugins/` and running `cargo build --release`. This workflow is supported by the new architecture.
    *   **Pipeline awareness:** The `PluginManager` will scan `plugins/` at runtime (or on changes, if hot-reloading is implemented) to discover new plugins.
    *   **Plugin access to other stages:** Plugins will register `StageFactory` instances with the host's `StageRegistry`. The `PipelineRuntime` will then use these factories to construct pipeline stages and connect them via the defined `Input`/`Output` traits and queues. Plugins themselves don't directly "access" other pipeline stages in the sense of calling their methods; rather, they become stages within the pipeline graph and communicate via the established queue mechanism.

## 4. End-to-End Data Flow Analysis

The end-to-end data flow, from sensor to GUI or CSV recorder, needs to be fully realized within the new architecture.

```mermaid
graph TD
    A[ADS1299 Sensor (SPI IRQs)] --> B{Acquisition Stage};
    B -- Packet<RawEegData> --> C[Memory Pool (Raw)];
    C --> D{Data Plane Thread Loop};

    subgraph Data Plane (Dedicated Thread)
        D --> E[Acquisition Stage (produces Packet<RawEegData>)];
        E -- Packet<RawEegData> --> F[ToVoltage Stage];
        F -- Packet<VoltageEegPacket> --> G[Memory Pool (Voltage)];
        G --> H[Gain Stage (filter.rs example)];
        H -- Packet<VoltageEegPacket> --> I[Other Filter Stages];
        I -- Packet<ProcessedEegData> --> J[Memory Pool (Processed)];
        J --> K[WebSocket Sink Stage];
        K --> L[CSV Sink Stage];
    end

    K -- WebSocket --> M[GUI (Kiosk UI)];
    L -- File System --> N[CSV Recorder];

    style A fill:#f9f,stroke:#333,stroke-width:2px
    style M fill:#ccf,stroke:#333,stroke-width:2px
    style N fill:#ccf,stroke:#333,stroke-width:2px
    style C fill:#dfd,stroke:#333,stroke-width:1px
    style G fill:#dfd,stroke:#333,stroke-width:1px
    style J fill:#dfd,stroke:#333,stroke-width:1px
```

**Identifying Gaps and Necessary Bridge Components:**
*   **Acquisition Stage:** A dedicated stage (`crates/pipeline/src/stages/acquire.rs` needs to be fully implemented to bridge the `ads1299` driver with the Data Plane, acquiring raw data and placing it into `Packet<T>` instances from a `MemoryPool`.
*   **`ToVoltage` Stage:** A stage to convert raw ADC counts to voltage values (`crates/pipeline/src/stages/to_voltage.rs`) is crucial early in the pipeline.
*   **Sink Stages:** `websocket_sink.rs` and `csv_sink.rs` need to be fully implemented as Data Plane stages, consuming `Packet<T>` and outputting to their respective destinations.
*   **Bridge Stages (Phase 1 Coexistence):** As per `unified-pipeline-architecture.md`, `ToDataPlane` and `FromDataPlane` bridge stages are essential for incremental transition, allowing new and old runtimes to coexist by copying data between them. This is critical for a phased migration.

## 5. Key Areas for Completion

### 5.1. Core Pipeline Infrastructure
*   **`MemoryPool` and `Packet<T>`:** Fully implement and rigorously test (with `miri` and `loom`) the zero-copy data structures, including the `Drop` implementation for returning buffers to the pool. This also includes ensuring `Packet<T>`'s `header` correctly carries `batch_size` and `timestamp` as per the architecture.
*   **`StageQueue`:** Finalize the wrapper around `rtrb` or `crossbeam::queue::ArrayQueue` for inter-stage communication.
*   **`Input`/`Output` Traits:** Confirm the stability and completeness of these traits for all stage interactions.
*   **`DataPlaneStage` Trait:** Ensure it provides all necessary hooks for stage developers.
*   **`HybridRuntime`:** Implement the new runtime that orchestrates the Data Plane loop on a dedicated thread, managing stage execution and queue interactions, distinct from the existing `PipelineRuntime`.

### 5.2. Configuration Management
*   **Centralized Configuration File:** Implement a robust JSON-based configuration system for the entire pipeline. This file should define:
    *   `memory_pools` with specific packet sizes and counts.
    *   `connections` array for queue capacities between stages.
    *   `data_plane` stage types and their specific `params` (e.g., filter coefficients, downsampling factors).
*   **`GraphBuilder` Update:** The `GraphBuilder` needs to be updated to parse this new configuration format and correctly instantiate and connect stages and memory pools for the Data Plane.
*   **GUI Interaction:** Develop the GUI components to dynamically read and write this configuration, allowing users to program pipeline stages. This implies schema generation for stage parameters (as seen in `filter.rs` with `JsonSchema`). The existing `/config` WebSocket in `crates/device/src/server.rs` will need to be integrated or replaced.

### 5.3. Plugin System Implementation
*   **`pipeline-abi` crate:** Create and stabilize this dedicated crate defining the shared data structures and function signatures for host-plugin communication. Implement version checking.
*   **`PluginManager`:** Develop the host-side component responsible for scanning the `plugins/` directory, dynamically loading `.so`/`.dll` files, and registering `StageFactory` instances. Implement `std::panic::catch_unwind` for fault isolation.
*   **Frontend Integration:**
    *   Implement static serving of plugin UI bundles (e.g., `/static/plugins/{plugin_name}/index.js`).
    *   Develop a discovery API endpoint for the Kiosk UI to fetch `plugin.toml` manifests.
    *   Implement dynamic `import()` in the Kiosk UI to load and render plugin UI components, replacing current direct imports.
*   **Developer Tooling:**
    *   `cargo xtask build-plugins`: A script to automate building and copying plugin artifacts.
    *   `cargo generate eeg-plugin` template: A template to lower the barrier to entry for new plugin authors, providing a ready-made structure.

### 5.4. Migration and Integration
*   **Phase 1 (Coexistence):** Implement `ToDataPlane` and `FromDataPlane` bridge stages to allow gradual migration, enabling the new and old runtimes to exchange data.
*   **Phase 2 (Migration):** Systematically migrate performance-critical stages (e.g., existing filters, gain stages) to the `DataPlaneStage` trait.
*   **Phase 3 (Full Transition):** Once all critical stages are migrated, remove bridge stages and fully commit to the new architecture.
*   **Integration of `ads1299` and `elata_emu_v1`:** Ensure these data sources are properly integrated as initial stages of the Data Plane, producing `Packet<T>` instances.
*   **Refactor `eeg_types` data structures:** Determine if `EegPacket`, `FilteredEegPacket`, and `FftPacket` should be refactored to directly use `Packet<T>` or if conversion layers are sufficient at Data Plane boundaries.

### 5.5. Control Plane Enhancements
*   **"Recording Lock":** Implement the `PipelineState` in the runtime to prevent parameter changes during active recording, with corresponding UI disablement. This will need to integrate with the existing `is_recording` atomic bool in `crates/device/src/server.rs`.
*   **GUI to Stage Control Flow:** Finalize the WebSocket-based JSON RPC mechanism for sending control commands from the GUI to specific stages via their control channels. This will need to integrate with or replace the existing `/command` WebSocket in `crates/device/src/server.rs`.

### 5.6. Testing and Validation
*   **Rigorous Testing:** Continue and expand testing for `unsafe` code, especially for `MemoryPool` and `Packet<T>`, using tools like `miri` (for undefined behavior) and `loom` (for concurrency issues).
*   **Fault Injection:** Ensure `catch_unwind` is consistently applied to stage execution to prevent single plugin failures from crashing the entire Data Plane.
*   **Observability:** Integrate `tracing` spans for key operations (acquire, release, send, recv) to monitor system health and performance.

### 5.7. Cleanup and Optimization
*   **Dead Code Removal:** Once stages are fully migrated and the new plugin system is in place, identify and remove obsolete code from the "old way" (e.g., old plugin loading mechanisms, unused `PipelineData` variants, `PipelineStage` trait and its associated runtime components).
*   **Performance Tuning:** Based on real-world usage, fine-tune queue capacities and `yield_threshold` values for optimal performance and latency.

## 6. Proposed Next Steps (High-Level Roadmap)

1.  **Finalize Core Data Plane Infrastructure:**
    *   Complete and rigorously test `MemoryPool`, `Packet<T>`, and `StageQueue`.
    *   Ensure `Input`/`Output` and `DataPlaneStage` traits are stable.
2.  **Implement Acquisition Stage:**
    *   Develop the `AcquisitionStage` to interface with `ads1299` and produce `Packet<T>`.
3.  **Develop `pipeline-abi` Crate:**
    *   Define and stabilize the host-plugin ABI.
4.  **Implement `PluginManager`:**
    *   Develop the host-side dynamic plugin loading mechanism.
5.  **Update Configuration System:**
    *   Implement the centralized JSON configuration for pipeline stages and memory pools.
    *   Update `GraphBuilder` to parse this configuration.
6.  **Implement Bridge Stages:**
    *   Create `ToDataPlane` and `FromDataPlane` stages for phased migration.
7.  **Migrate Key Stages:**
    *   Begin migrating performance-critical stages (e.g., `ToVoltage`, `WebSocketSink`, `CsvSink`) to the new `DataPlaneStage` trait.
8.  **Frontend Plugin Integration:**
    *   Implement dynamic UI loading in the Kiosk UI, replacing direct imports.
9.  **Implement Control Plane Features:**
    *   Develop "Recording Lock" and GUI-to-stage control flow, integrating with existing WebSocket mechanisms.
10. **Cleanup:**
    *   Remove dead code from the old plugin system and stages.

## 7. Addressing User Questions and Concerns

This section directly addresses the questions and concerns raised during the review process.

### 7.1. Can it use Python as a plugin/pipeline-stage?
Yes, the architecture is designed to support Python as a plugin/pipeline stage, as detailed in `todo/unified-pipeline-architecture2.md`. There are two primary approaches:
*   **ScriptStage in Host:** This involves embedding a CPython interpreter within the Rust host. A `ScriptStage` would load and execute Python files (e.g., `.py` files) that expose a `process` function. This is easier for data scientists but incurs per-packet GIL (Global Interpreter Lock) and data copy costs.
*   **PyO3/Maturin Hybrid:** This approach involves writing Python code and then wrapping it with `pyo3::prelude::*` to compile it into a Rust `cdylib` (dynamic library) that satisfies the defined ABI. This offers better performance as it avoids embedding the interpreter directly in the hot path, but still requires a Rust build step.
Longer term, Wasmtime with `memory64` is also a consideration for zero-copy sharing with non-Rust languages.

### 7.2. Did you cite files and paths to back your data?
Yes, throughout the assessment and plan, specific file paths and code snippets have been referenced to back observations and proposed changes. For example:
*   `crates/pipeline/src/stages/filter.rs` was used as an example of a new Data Plane stage.
*   `crates/pipeline/src/data.rs` and `crates/eeg_types/src/event.rs` were examined to understand data structures (`PipelineData`, `Packet<T>`, `EegPacket`, `Arc<[T]>`).
*   `crates/pipeline/src/stage.rs`, `crates/pipeline/src/runtime.rs`, and `crates/pipeline/src/graph.rs` were reviewed for stage traits, runtime orchestration, and graph building.
*   `kiosk/src/components/EegDataVisualizer.tsx` was cited for the hardcoded plugin UI import.
*   `crates/device/src/connection_manager.rs` and `crates/device/src/server.rs` were used to understand WebSocket communication and control.

### 7.3. "All the weird stuff. cargo.toml for plugins"
The use of `Cargo.toml` files within each plugin's directory (e.g., `plugins/brain_waves_fft/Cargo.toml`) is standard for Rust projects. Each plugin is intended to be a separate Rust crate that compiles into a dynamic library (`cdylib`). This modular approach allows for:
*   **Independent Development:** Plugins can be developed and tested in isolation.
*   **Dynamic Loading:** The host application can load these compiled dynamic libraries at runtime without needing to be recompiled itself.
*   **Dependency Management:** Each plugin manages its own Rust dependencies via its `Cargo.toml`.
This aligns with the modern plugin architecture where plugins are self-contained units.

### 7.4. Are we calling them plugins still and they define pipeline stages? What if a plugin GUI expects a certain pipeline outputs? Is the system ready for that one?
*   **Terminology:** Yes, we are calling them "plugins," and they *provide* or *implement* pipeline stages. A plugin is a deployable unit that contains one or more `DataPlaneStageFactory` implementations, allowing the host to create instances of its stages.
*   **Plugin GUI Expectations:** The system is designed to handle this. The `plugin.toml` manifest (as seen in `todo/unified-pipeline-architecture2.md`) includes a `[ui]` section with `required_props`. This allows the plugin to declare what data it expects (e.g., `stage_id`, `data_stream_url`). The `EegDataVisualizer.tsx` currently subscribes to `Fft` data, which is delivered via `SensorEvent::WebSocketBroadcast`. The new `WebSocketSink` stage will be responsible for taking `Packet<T>` data from the Data Plane and serializing it into the binary format expected by the frontend. The `EegDataContext` in the frontend will then process this data and make it available to the plugin's UI component. The `data_stream_url` prop would likely point to the WebSocket endpoint for that specific data stream.

### 7.5. If the GUI stops using a pipeline, how is the pipeline turned off?
The pipeline is controlled by the Control Plane. When the GUI (or any client) wishes to stop a pipeline, it sends a command (e.g., a JSON RPC message like `{"command": "stop_pipeline", "pipeline_id": "..."}`) via the command WebSocket to the daemon (`crates/device/src/server.rs`). The daemon's command handler would then instruct the `PipelineRuntime` (the Control Plane component) to call its `stop()` method. The `PipelineRuntime` then signals cancellation to all running Data Plane stages via `CancellationToken`, allowing them to gracefully shut down.

### 7.6. What if there is pipeline overlap between 2 pipelines? Using the same stages. Do they have two separate pipelines doing the same work?
In the current design, each pipeline instance is independent. If two separate pipelines use the "same" stage type (e.g., two `GainStage`s), they will instantiate two separate instances of that stage. These instances will operate independently, processing their own data streams. This means they would indeed be doing "the same work" in parallel, but on different data.
*   **Resource Sharing:** While stages themselves are separate instances, the underlying `MemoryPool`s are shared resources. Stages can acquire and release `Packet<T>`s from these shared pools, which helps optimize memory usage across multiple pipelines or stages.
*   **Future Considerations:** For scenarios where true shared processing of the *same* data stream is desired (e.g., a single acquisition stage feeding multiple analysis pipelines), the graph configuration would need to reflect this by having multiple downstream stages connected to a single upstream stage's output.

### 7.7. Is it a git clone that grabs a new plugin? Does it go to the plugin folder? What steps to get it working in the final system? cargo build? What are the edge cases?
The design for plugin loading and integration is well-thought-out and aims to avoid showstoppers, as detailed in `todo/unified-pipeline-architecture2.md`. The edge cases are acknowledged and addressed through specific design choices and tooling. The edge cases are acknowledged and addressed through specific design choices and tooling.
*   **Workflow:** Yes, the envisioned workflow is:
    1.  `git clone https://github.com/example/new_plugin plugins/new_plugin` (clone the plugin's repository directly into the `plugins/` directory).
    2.  `cargo build --release --manifest-path plugins/new_plugin/Cargo.toml` (build the plugin in release mode).
    3.  The compiled dynamic library (`libnew_plugin.so` on Linux, `.dll` on Windows, `.dylib` on macOS) and its `plugin.toml` are then expected to be in a specific structure within `plugins/new_plugin/`. The `cargo xtask build-plugins` tool is intended to automate this and ensure correct placement.
*   **Getting it working:** Once built and correctly placed, the `PluginManager` in the host application will discover and load the plugin at runtime. No host recompilation is needed.
*   **Edge Cases and Mitigations:**
    *   **ABI Mismatch:** If the plugin is compiled against a different version of the `pipeline-abi` crate than the host, the `PluginManager` will detect this (via `abi_version()` function) and reject the plugin, logging a warning. This is a critical safety mechanism.
    *   **Build Failures:** The `cargo build` step might fail due to compilation errors, missing dependencies, or platform incompatibility. This is a standard development challenge, mitigated by clear error messages from Cargo.
    *   **Incorrect `plugin.toml`:** A malformed or missing `plugin.toml` will prevent the `PluginManager` from correctly identifying and configuring the plugin. This is mitigated by clear documentation and the `cargo generate eeg-plugin` template.
    *   **Missing UI Bundles:** If a plugin has a UI but its JavaScript bundle is not correctly built or placed, the UI component will fail to load in the frontend. This is mitigated by the `cargo xtask build-plugins` tool ensuring correct artifact placement.
    *   **Runtime Panics:** A faulty plugin could panic during execution. The `std::panic::catch_unwind` mechanism is designed to isolate this, disabling the problematic plugin without crashing the entire pipeline. This is a robust safety measure.

### 7.8. How do plugins/ in plugin folder call external Rust code? Or do they not need to since it's a naive pipeline stage?
Plugins are regular Rust crates. They can declare dependencies on other Rust crates in their `Cargo.toml` file, just like any other Rust project. For example, a plugin might depend on a `signal_processing` crate for DSP algorithms.
*   **Dependency on `crates/pipeline`:** Plugins will *definitely* need to depend on `crates/pipeline`. Specifically, they will depend on the `pipeline-abi` crate (which is part of `crates/pipeline` or a separate crate within the `pipeline` workspace) to implement the `DataPlaneStage` trait and interact with `Packet<T>`, `StageContext`, and other core pipeline components. They might also use other utility modules from `crates/pipeline` if those are exposed publicly.
*   **Dependency on `crates/sensors`:** It's less likely that a *typical* processing plugin would directly call `crates/sensors`. The `crates/sensors` module is primarily for low-level hardware interaction (like the ADS1299 driver). The `AcquisitionStage` (which is a `DataPlaneStage` within `crates/pipeline`) would be the one interacting with `crates/sensors` to get raw data and convert it into `Packet<T>`. A processing plugin would typically receive `Packet<T>` from an upstream stage, not directly from a sensor driver. However, if a plugin *itself* was an acquisition stage for a new sensor, then yes, it would need to interact with `crates/sensors` or a similar hardware abstraction layer.


### 7.9. If a user gets a new plugin online, it's self-contained to be only in the plugins/ directory and work? Or do they need to update non-plugins/ code? Like the main Cargo.toml for example.
The ideal is for a new plugin to be entirely self-contained within its subdirectory under `plugins/`. The user should *not* need to modify the main application's `Cargo.toml` or any other non-`plugins/` code to get a new plugin working. This is a core design goal of the dynamic plugin system.
The `cargo xtask build-plugins` and `cargo generate eeg-plugin` tools are specifically designed to enforce and facilitate this self-contained nature, ensuring that plugins can be dropped in, built, and run without modifying the host application's build configuration. The only exception would be if the `pipeline-abi` crate itself undergoes a major version bump, which would require recompiling both the host and all plugins against the new ABI.

### 7.10. Did you look in all the nooks and crannies in the codebase?
My review was a targeted and comprehensive analysis of the codebase areas most relevant to the unified pipeline architecture and the user's specific questions. This included core pipeline components (`crates/pipeline/`), data structures (`crates/eeg_types/`), device interaction (`crates/device/`), and frontend integration (`kiosk/`). While I did not perform an exhaustive line-by-line audit of every single file in the entire repository, the examination was sufficient to identify key architectural patterns, existing implementations, discrepancies with the proposed new architecture, and the major tasks required for the transition. The goal was to provide a holistic picture and a detailed plan, which I believe has been achieved based on the information gathered.