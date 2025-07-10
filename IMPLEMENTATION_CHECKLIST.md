# Unified Pipeline Architecture: Implementation Checklist (Pure-Rust Inventory)

This checklist tracks the implementation progress for the new **pure-Rust inventory-based** hybrid pipeline architecture. This plan prioritizes a clean, static plugin model over a dynamic ABI-driven approach.

## Phase 0: Core Infrastructure & Foundations

- [x] **1. Implement Core Data Structures (`MemoryPool`, `Packet<T>`)**
  - [x] Define `Packet<T>` smart pointer with `PacketHeader`.
  - [x] Implement `Packet`'s `Drop` trait for auto-release.
  - [x] Implement `MemoryPool` with `crossbeam::ArrayQueue` for lock-free acquire/release.
  - [x] Add `acquire()` (async) and `try_acquire()` (non-blocking) methods.
- [x] **2. Rigorously Test Core Data Structures**
  - [x] Create unit tests for `MemoryPool` and `Packet`.
  - [x] Integrate `miri` testing to check for undefined behavior in `unsafe` blocks.
  - [x] Integrate `loom` testing to verify concurrency and thread-safety.
- [x] **3. Implement `StageQueue`**
  - [x] Create a wrapper around the chosen queue backend (`rtrb` or `crossbeam`) that implements the `Input`/`Output` traits.

## Phase 1: Pure-Rust Inventory Plugin System ✅ COMPLETE

- [x] **1. Inventory-Based Stage Registration**
  - [x] Use `inventory::submit!` for compile-time stage registration (see [`filter.rs`](crates/pipeline/src/stages/filter.rs:277)).
  - [x] Define `DataPlaneStage` trait and `DataPlaneStageFactory` for stage creation.
  - [x] Implement `StaticStageRegistrar` for automatic stage discovery.
  - [x] All stages register themselves automatically at compile time.
- [x] **2. Stage Registry System**
  - [x] Stages are discovered via `inventory::iter` at runtime.
  - [x] No dynamic library loading required - pure Rust workspace approach.
  - [x] Type-safe stage creation with full compiler checking.
  - [x] Zero FFI/unsafe code for plugin system.
- [x] **3. Developer Experience**
  - [x] Simple workflow: add stage to workspace, implement traits, use `inventory::submit!`.
  - [x] Full IDE support and debugging capabilities.
  - [x] Rebuild host when adding new stages (acceptable trade-off for simplicity).

## Phase 2: Initial Vertical Slice (Inventory-Native)

- [x] **1. Implement Core Stages (Unified Approach)** *(✅ Complete)*
  - [x] Implement `AcquisitionStage` to bridge the `ads1299` driver to the Data Plane.
  - [x] Implement `ToVoltageStage` to convert raw data.
  - [x] Implement `WebSocketSink` as a `DataPlaneStage`.
  - [x] Implement `CsvSink` as a `DataPlaneStage`.
  - [x] Implement `FilterStage` as canonical example with `ctrl_loop!` macro.
  - [x] Fix major compilation issues (dependencies, trait methods, enum variants).
  - [x] Resolve borrowing conflicts in CSV sink and WebSocket sink stages.
  - [x] **COMPLETED:** Fixed final compilation errors in acquire stage (borrowing conflicts) and WebSocket sink (trait ambiguity in tests).
- [ ] **2. Update Configuration System**
  - [ ] Finalize the JSON schema for defining memory pools, stages (by string ID), and connections.
  - [ ] Update `GraphBuilder` to use the `PluginManager`'s `StageRegistry` to construct the Data Plane graph.
- [ ] **3. Implement Control Plane Features**
  - [ ] Implement the "Recording Lock" (`PipelineState`) in the `PipelineRuntime`.
  - [ ] Implement the WebSocket JSON RPC command router for GUI control.

## Phase 3: UI Integration & Full Adoption

- [ ] **1. Frontend Plugin Integration**
  - [ ] Define `plugin.toml` manifest standard.
  - [ ] Implement static serving of plugin UI bundles.
  - [ ] Implement the `/api/plugins` discovery endpoint to serve manifests.
  - [ ] Refactor Kiosk UI to use dynamic `import()` for plugin components based on manifest data.
- [ ] **2. Port Remaining Logic**
  - [ ] Migrate any remaining critical logic from `plugins/*` to new, ABI-compliant plugins.
- [ ] **3. Remove Legacy Code**
  - [ ] Remove old `PipelineStage` trait and associated runtime logic.
  - [ ] Remove old, unused plugin-loading code.
  - [ ] **Note:** The `ToDataPlane`/`FromDataPlane` bridge stages are no longer needed and will not be implemented.
- [ ] **4. Performance Tuning**
  - [ ] Add `tracing` metrics for queue lengths and pool usage.
  - [ ] Benchmark and tune default queue capacities.
