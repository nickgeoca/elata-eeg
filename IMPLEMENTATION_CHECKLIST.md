# Unified Pipeline Architecture: Implementation Checklist

This checklist tracks the implementation progress for the new hybrid pipeline architecture, based on the roadmaps in the planning documents.

## Phase 0: Core Infrastructure & Foundations

- [ ] **1. Implement Core Data Structures (`MemoryPool`, `Packet<T>`)**
  - [ ] Define `Packet<T>` smart pointer with `PacketHeader`.
  - [ ] Implement `Packet`'s `Drop` trait for auto-release.
  - [ ] Implement `MemoryPool` with `crossbeam::ArrayQueue` for lock-free acquire/release.
  - [ ] Add `acquire()` (async) and `try_acquire()` (non-blocking) methods.
- [ ] **2. Rigorously Test Core Data Structures**
  - [ ] Create unit tests for `MemoryPool` and `Packet`.
  - [ ] Integrate `miri` testing to check for undefined behavior in `unsafe` blocks.
  - [ ] Integrate `loom` testing to verify concurrency and thread-safety.
- [ ] **3. Stabilize Core Traits**
  - [ ] Finalize the `Input<T>` and `Output<T>` traits for inter-stage communication.
  - [ ] Finalize the `DataPlaneStage` and `DataPlaneStageFactory` traits.
- [ ] **4. Implement `StageQueue`**
  - [ ] Create a wrapper around the chosen queue backend (`rtrb` or `crossbeam`) that implements the `Input`/`Output` traits.

## Phase 1: Initial Vertical Slice

- [ ] **1. Implement `pipeline-abi` Crate**
  - [ ] Create the dedicated, versioned `pipeline-abi` crate.
  - [ ] Define `AbiVersion` struct and `register_factories` function signature.
- [ ] **2. Implement `PluginManager`**
  - [ ] Create the `PluginManager` in the host application.
  - [ ] Implement scanning of `plugins/` directory.
  - [ ] Implement dynamic library loading with `libloading`.
  - [ ] Implement ABI version checking.
  - [ ] Wrap all plugin calls in `std::panic::catch_unwind`.
- [ ] **3. Implement Acquisition & Sink Stages**
  - [ ] Implement `AcquisitionStage` to bridge the `ads1299` driver to the Data Plane.
  - [ ] Implement `ToVoltageStage` to convert raw data.
  - [ ] Implement `WebSocketSink` as a `DataPlaneStage`.
  - [ ] Implement `CsvSink` as a `DataPlaneStage`.
- [ ] **4. Implement Bridge Stages for Transition**
  - [ ] Create `ToDataPlane` bridge stage.
  - [ ] Create `FromDataPlane` bridge stage.

## Phase 2: Configuration & Control

- [ ] **1. Update Configuration System**
  - [ ] Finalize the JSON schema for defining memory pools, stages, and connections.
  - [ ] Update `GraphBuilder` to parse the new schema and construct the Data Plane graph.
- [ ] **2. Implement Control Plane Features**
  - [ ] Implement the "Recording Lock" (`PipelineState`) in the `PipelineRuntime`.
  - [ ] Implement the WebSocket JSON RPC command router for GUI control.

## Phase 3: Full Migration & UI Integration

- [ ] **1. Migrate Existing Stages**
  - [ ] Migrate `basic_voltage_filter` to the `DataPlaneStage` trait.
  - [ ] Migrate `brain_waves_fft` to the `DataPlaneStage` trait.
- [ ] **2. Frontend Plugin Integration**
  - [ ] Implement static serving of plugin UI bundles.
  - [ ] Implement the `/api/plugins` discovery endpoint.
  - [ ] Refactor Kiosk UI to use dynamic `import()` for plugin components, removing direct imports.
- [ ] **3. Developer Tooling**
  - [ ] Create `cargo xtask build-plugins` script.
  - [ ] Create `cargo generate eeg-plugin` template.

## Phase 4: Cleanup & Optimization

- [ ] **1. Remove Legacy Code**
  - [ ] Remove bridge stages (`ToDataPlane`, `FromDataPlane`).
  - [ ] Remove old `PipelineStage` trait and associated runtime logic.
  - [ ] Remove old plugin loading mechanisms.
- [ ] **2. Performance Tuning**
  - [ ] Add `tracing` metrics for queue lengths and pool usage.
  - [ ] Benchmark and tune default queue capacities and `yield_threshold` values.