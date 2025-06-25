# Plan: Resolve Cyclic Dependency by Consolidating Plugin Architecture

**Date:** 2025-06-25

## Status: Completed & Superseded

**This plan was executed and its goals were incorporated into the comprehensive `architectural_unification_plan.md` on 2025-06-25.** The cyclic dependency has been resolved, and the obsolete plugin architecture has been removed. This document is now archived for historical context.

---

## 1. Problem

The `cargo build` command fails with a cyclic dependency error:
`adc_daemon` -> `basic_voltage_filter_plugin` -> `adc_daemon`

A thorough investigation revealed that the cause is a duplicated and outdated plugin architecture defined within the `crates/device` (`adc_daemon`) crate. The correct, modern, and decoupled architecture is already defined in the `crates/eeg_types` crate but is not being used consistently.

## 2. Goal

The goal is to eliminate the obsolete code, resolve the cyclic dependency, and align the entire project with the superior architecture defined in `eeg_types`. This will make the system more robust, maintainable, and fix the build error.

## 3. Implementation Steps

### Step 1: Delete Obsolete Code from `crates/device`

The `event.rs` and `plugin.rs` files in `crates/device/src` are outdated and conflict with the correct definitions in `eeg_types`. They will be deleted.

- **Action:** Delete file `crates/device/src/event.rs`.
- **Action:** Delete file `crates/device/src/plugin.rs`.

### Step 2: Refactor `adc_daemon` to Adhere to the `eeg_types` Contract

The `adc_daemon` crate will be updated to correctly use and implement the traits and types from `eeg_types`.

1.  **Update `crates/device/src/main.rs` and `crates/device/src/lib.rs`**:
    *   Remove any `mod event;` and `mod plugin;` declarations.
    *   Update all `use` statements to import `EegPlugin`, `SensorEvent`, `EventBus`, etc., directly from `eeg_types`.

2.  **Make `EventBus` Implementation Official**:
    *   In `crates/device/src/event_bus.rs`, modify the `EventBus` struct to explicitly implement the `eeg_types::plugin::EventBus` trait.

3.  **Update `PluginSupervisor`**:
    *   In `crates/device/src/plugin_supervisor.rs`, ensure it uses the `EegPlugin` trait from `eeg_types` for all plugin management.

### Step 3: Fix Plugin Dependencies

The `basic_voltage_filter_plugin` will be updated to remove its invalid dependency on the `adc_daemon`.

1.  **Modify `plugins/basic_voltage_filter/Cargo.toml`**:
    *   **Remove** the line: `device = { path = "../../crates/device", package = "adc_daemon" }`.
    *   Ensure the dependency on `eeg_types` remains.

2.  **Update Plugin Source Code**:
    *   Modify `plugins/basic_voltage_filter/src/lib.rs` to import all necessary traits and types (`EegPlugin`, `SensorEvent`, etc.) from `eeg_types`.

## 4. Expected Outcome

- The cyclic dependency will be resolved.
- `cargo build` will complete successfully.
- The entire application will use a single, consistent, and decoupled plugin architecture, improving long-term stability and maintainability.