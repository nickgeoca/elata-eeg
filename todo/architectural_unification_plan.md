# Plan: Unify Plugin Architecture and Resolve Build Failure

**Date:** 2025-06-25

## Status: Completed

**This plan was successfully executed on 2025-06-25. The architectural unification is complete.** The build is now stable, the cyclic dependency is resolved, and the project correctly uses a single, event-driven plugin architecture as intended. All obsolete code has been removed, and the `brain_waves_fft` plugin is fully integrated.

---

## 1. Executive Summary

This document outlines a comprehensive plan to resolve a critical `cargo build` failure and refactor the entire project to a single, unified, event-driven plugin architecture. The build is currently failing due to a combination of missing workspace dependencies and a deeper architectural flaw involving cyclic dependencies between the `adc_daemon` and its plugins.

The root cause is a conflict between an obsolete, duplicated plugin architecture within `crates/device` and the correct, modern architecture defined in `crates/eeg_types`. This plan will eliminate the flawed implementation, align the project with the superior event-driven model, and ensure long-term stability and maintainability.

## 2. Context and Supporting Documents

This plan is informed by a thorough analysis of the following documents, which revealed the extent of the architectural issues:

*   **[`todo/architectural_fix_plan.md`](todo/architectural_fix_plan.md):** This document first identified the cyclic dependency (`adc_daemon` -> `basic_voltage_filter_plugin` -> `adc_daemon`) and correctly diagnosed the cause as a duplicated plugin architecture.
*   **[`todo/architectural_plugin_fix.md`](todo/architectural_plugin_fix.md):** This document provided a deeper analysis, contrasting the flawed external-process architecture with the superior in-process, event-driven model. It confirmed that the correct path forward is to fully commit to the event-driven architecture.
*   **Build Logs:** The `cargo build` error message explicitly pointed to a workspace dependency inheritance issue, confirming that `anyhow` was missing from the root `Cargo.toml`'s `[workspace.dependencies]` section.

## 3. Consolidated Implementation Plan

### Phase 1: Fix Workspace Dependencies & Build Error

This phase addresses the immediate build failure.

1.  **Update Root `Cargo.toml`**:
    *   **File**: [`Cargo.toml`](Cargo.toml)
    *   **Action**: Add `anyhow`, `async-trait`, and `tracing` to the `[workspace.dependencies]` section. This will resolve the inheritance error that is currently breaking the build.
    *   **Proposed Change**:
        ```diff
        --- a/Cargo.toml
        +++ b/Cargo.toml
        @@ -12,3 +12,6 @@
         serde = { version = "1.0", features = ["derive"] }
         serde_json = "1.0"
         log = "0.4"
        +anyhow = "1.0"
        +async-trait = "0.1"
        +tracing = "0.1"
        ```

### Phase 2: Core Architectural Refactoring

This phase will eliminate the obsolete architecture and resolve the underlying cyclic dependency.

1.  **Delete Obsolete Code from `crates/device`**:
    *   **Action**: Delete the following files, which are superseded by `eeg_types`:
        *   `crates/device/src/event.rs`
        *   `crates/device/src/plugin.rs`
        *   `crates/device/src/plugin_manager.rs`

2.  **Refactor `adc_daemon` to Use `eeg_types`**:
    *   **Files**: `crates/device/src/main.rs`, `crates/device/src/lib.rs`
    *   **Action**: Remove `mod` declarations for the deleted files and update all `use` statements to import `EegPlugin`, `SensorEvent`, `EventBus`, etc., directly and exclusively from the `eeg_types` crate.

3.  **Update `EventBus` Implementation**:
    *   **File**: `crates/device/src/event_bus.rs`
    *   **Action**: Ensure the `EventBus` struct explicitly implements the `eeg_types::plugin::EventBus` trait.

4.  **Fix Plugin Dependencies**:
    *   **File**: `plugins/basic_voltage_filter/Cargo.toml`
    *   **Action**: Remove the direct dependency on `adc_daemon` to break the cycle.
    *   **File**: `plugins/basic_voltage_filter/src/lib.rs`
    *   **Action**: Update the plugin to import all necessary types from `eeg_types`.

### Phase 3: Finalize and Integrate the `brain_waves_fft` Plugin

The `brain_waves_fft` plugin will serve as the official, high-performance implementation for FFT processing.

1.  **Verify Plugin Integration**:
    *   **File**: `crates/device/src/main.rs`
    *   **Action**: Confirm that the `BrainWavesFftPlugin` is correctly instantiated and registered with the `PluginSupervisor`.

2.  **Verify WebSocket Forwarding**:
    *   **File**: `crates/device/src/server.rs`
    *   **Action**: Review the WebSocket handling logic to ensure that `FftPacket` events are correctly subscribed to and forwarded to the Kiosk UI.

3.  **Verify Kiosk Data Handling**:
    *   **File**: [`kiosk/src/components/EegDataHandler.tsx`](kiosk/src/components/EegDataHandler.tsx)
    *   **Action**: Confirm that the frontend correctly parses `FftPacket` events from the WebSocket, specifically handling messages where `message.type === 'Fft'`.

## 4. Expected Outcome

*   **Build Success**: The `cargo build` command will complete without errors.
*   **Architectural Integrity**: The cyclic dependency will be eliminated, and the entire application will use a single, consistent, and decoupled plugin architecture.
*   **Functionality**: The `brain_waves_fft` plugin will correctly process EEG data, and the results will be visualized in the Kiosk UI, confirming the end-to-end data flow is working as intended.

This plan provides a clear, actionable path to resolving the current issues and aligning the project with its intended architecture.