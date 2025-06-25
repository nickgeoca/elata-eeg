# Plan: Full Refactor to a Unified Event-Driven Plugin Architecture

**Date:** 2025-06-24

## Status: Completed & Superseded

**This plan was executed and its goals were incorporated into the comprehensive `architectural_unification_plan.md` on 2025-06-25.** The flawed external-process architecture has been fully removed and replaced by the unified event-driven model. This document is now archived for historical context.

---

## 1. Problem Summary

A thorough investigation revealed that the root cause of the failing `eeg-circular-graph` and `brain-waves-display` plugins is a major architectural conflict within the `device` daemon.

*   **The Standard:** The documented and correct architecture (`plugins/README.md`, `PluginSupervisor`) is a high-performance, **in-process, event-driven system**. Plugins are compiled-in Rust crates that communicate via a zero-copy `EventBus`.
*   **The Flaw:** The `brain-waves-display` plugin was built against an obsolete, **external-process architecture**. It runs as a standalone web server. The `PluginManager` that was supposed to run it was an incomplete stub that didn't actually run the process or forward any data.

This conflict means the raw data stream from the sensor was being sent to a black hole. The `PluginManager` would log that it was "sending" data, but the data was immediately dropped. No data ever reached any plugin or the frontend WebSocket, causing the UI to be blank.

## 2. Architectural Decision: Event-Driven vs. External Process

We have confirmed that the **in-process, event-driven architecture is vastly superior** for this application.

| Feature | Event-Driven (In-Process) | External Process (WebSocket/HTTP) |
| :--- | :--- | :--- |
| **Performance** | **Excellent.** Nanosecond latency. Zero-copy data sharing via `Arc<T>`. | **Poor.** Millisecond latency. High CPU/memory overhead from serialization and network stack. |
| **Simplicity** | **High.** Type safety guaranteed by the Rust compiler. Unified build process. | **Low.** High runtime complexity managing ports, processes, data contracts, and network errors. |
| **Resources** | **Efficient.** Single process, low memory footprint. Ideal for embedded. | **Inefficient.** Each plugin is a separate process, consuming significant resources. |
| **Conclusion** | **Correct choice.** Performant, robust, and maintainable. | **Flawed choice.** Brittle, slow, and overly complex for this use case. |

The path forward is to commit to the superior event-driven model and eliminate all remnants of the flawed external-process architecture.

### 2.5. Advanced Implementation Details (User-Suggested Improvements)

To ensure the highest level of performance and robustness, the `EventBus` and event types will be implemented with the following principles:

1.  **Bounded, Backpressure-Enabled Event Bus:**
    *   The `EventBus` will be built on `tokio::sync::broadcast::channel(N)`.
    *   The channel capacity `N` will be sized to hold approximately 25ms of data, ensuring that a slow plugin cannot cause system-wide memory bloat or latency spikes. Slow consumers will drop packets, which is the desired behavior.

2.  **Zero-Copy Event Design:**
    *   Events carrying large data payloads (e.g., `RawEeg`, `FilteredEeg`) will be defined as newtypes wrapping slices with a lifetime parameter (e.g., `struct RawEeg<'a>(&'a [i32])`).
    *   This design makes it impossible for a plugin to accidentally clone the data, enforcing zero-copy reads across the entire system at compile time.

3.  **Future Optimization: Memory Pooling:**
    *   The initial implementation will allocate buffers as needed.
    *   If performance profiling indicates that memory allocation is a bottleneck, a memory pool (e.g., `slab` or a custom bump allocator) will be introduced to reuse data buffers, further reducing latency and overhead.

## 3. The Refactoring Plan

The following steps will be taken to unify the architecture and fix the data pipeline.

### Step 1: Delete Obsolete and Flawed Code

To create a clean slate, the following components, which are based on the flawed architecture, will be deleted:

1.  **Directory:** `plugins/brain-waves-display/`
2.  **File:** `crates/device/src/plugin_manager.rs`

### Step 2: Create a New, Correct `brain-waves` Plugin

A new plugin will be created from scratch that adheres to the standard defined in `plugins/README.md`.

1.  **Create New File:** `crates/device/src/plugins/brain_waves.rs`
2.  **Implement `BrainWavesPlugin` Struct:** This struct will contain the state needed for FFT analysis (e.g., data buffers, FFT planner).
3.  **Implement `EegPlugin` Trait:**
    *   `name()`: Returns `"brain_waves"`.
    *   `event_filter()`: Returns `vec![EventFilter::FilteredEeg]`. The FFT should run on the clean, filtered data, not the raw ADC output.
    *   `run()`: This method will contain the core logic:
        1.  Receive `SensorEvent::FilteredEeg` from the event bus.
        2.  Append the `voltage_samples` to an internal buffer for each channel.
        3.  When a buffer contains enough data (e.g., 512 samples), perform the FFT calculation using `rustfft`.
        4.  Construct a new `SensorEvent::FftPacket` containing the power spectral density (PSD) for each channel.
        5.  Broadcast the `FftPacket` back onto the `EventBus`.

### Step 3: Integrate the New Plugin

The new plugin will be registered with the `PluginSupervisor` to be managed as part of the main daemon process.

1.  **Modify `crates/device/src/plugins/mod.rs`**:
    *   Add `pub mod brain_waves;`
    *   Add `pub use brain_waves::BrainWavesPlugin;`
2.  **Modify `crates/device/src/plugin_supervisor.rs`**:
    *   In `register_plugins()`, add the `BrainWavesPlugin` to the list of plugins to be started.

### Step 4: Verify WebSocket Forwarding

The final step is to ensure the main WebSocket server, which communicates with the Kiosk UI, correctly handles the new `FftPacket`.

1.  **Investigate `crates/device/src/server.rs`**: Review the WebSocket handling logic.
2.  **Ensure `FftPacket` Forwarding**: Confirm that the server subscribes to `FftPacket` events from the `EventBus` and forwards them to connected frontend clients that have requested them.

This plan will result in a robust, performant, and maintainable system that correctly processes and visualizes EEG data.