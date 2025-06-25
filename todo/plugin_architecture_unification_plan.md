# Plugin Architecture Unification Plan

## 1. The Problem

The current system has three conflicting plugin architectures, leading to compilation errors and architectural ambiguity:

1.  **Built-in Modules:** Compiled directly into the `device` crate (e.g., `brain_waves`).
2.  **External Crate Dependencies:** Linked at compile time via `Cargo.toml` (e.g., `csv_recorder_plugin`, `basic_voltage_filter_plugin`).
3.  **Dynamic Executables:** A new, incomplete system in `plugin_manager.rs` that loads plugins from `plugin.toml` files at runtime.

The `basic_voltage_filter` is caught between models #1 and #2, causing the immediate build failure.

## 2. The Solution: A Unified Architecture

We will formally adopt the **External Crate Dependency** model (#2) as the standard for all plugins. This provides the best balance of modularity, performance, and ease of development for the current project stage. It directly enables the desired `git clone -> recompile -> ready-to-go` workflow.

This architecture is built for performance on multi-core systems like the Raspberry Pi 5 by leveraging true parallelism and a zero-copy event bus.

### High-Level Architecture Diagram

```mermaid
graph TD
    subgraph "Device Crate (Running on Pi 5)"
        subgraph "Core 1"
            A[Acquisition Task] -- RawEeg Event --> B((Event Bus));
        end
        subgraph "Core 2"
            B -- RawEeg Event --> C[Voltage Filter Plugin Task];
            C -- FilteredEeg Event --> B;
        end
        subgraph "Core 3"
            B -- RawEeg Event --> D[CSV Recorder Plugin Task];
        end
        subgraph "Core 4"
            B -- RawEeg & FilteredEeg Events --> E[WebSocket Server Task];
        end
        F[Plugin Supervisor] -- Manages Lifecycle --> C;
        F -- Manages Lifecycle --> D;
    end

    subgraph "External"
        E -- EEG Data Stream --> G(Kiosk GUI);
    end

    style `Device Crate (Running on Pi 5)` fill:#f9f,stroke:#333,stroke-width:2px
    style External fill:#ccf,stroke:#333,stroke-width:2px
```

## 3. Implementation Plan

### Phase 1: Architectural Alignment & Bug Fix

1.  **Standardize the Plugin Model:** Officially adopt the "External Crate Dependency" model. Plugins will be separate crates in the `/plugins` directory and included in `crates/device/Cargo.toml`.
2.  **Correct Module Declaration:** Fix the compile error by modifying `crates/device/src/plugins/mod.rs` to correctly `use` the external `basic_voltage_filter_plugin` crate instead of declaring it as a local `mod`.
3.  **Create Plugin Supervisor:** Create a new module, `crates/device/src/plugin_supervisor.rs`. This will be responsible for:
    *   Holding a collection of all statically compiled plugins (`Box<dyn EegPlugin>`).
    *   Spawning and managing the lifecycle of an async task for each plugin.
    *   Connecting each plugin to the main event bus.

### Phase 2: Integrating Plugins into the Data Flow

1.  **Update Event Bus:** Modify `crates/device/src/event_bus.rs` to allow the `PluginSupervisor` to register plugins as listeners.
2.  **Update Main Logic:** Modify `crates/device/src/main.rs` to initialize the `PluginSupervisor` and start the plugins.
3.  **Ensure Data Flow:** The `basic_voltage_filter` plugin will listen for `RawEeg` events, process them, and publish new `FilteredEeg` events back onto the bus.

### Phase 3: Connecting to the GUI

1.  **Update WebSocket Server:** Modify `crates/device/src/server.rs` to listen for the new `FilteredEeg` events and broadcast them over the `/eeg` WebSocket.
2.  **Verify Frontend Handling:** Ensure the Kiosk frontend (`kiosk/src/components/EegRenderer.tsx`) can correctly parse and display this new data type.

## 4. Follow-up Task

*   Create a `plugins/README.md` file to document the new standardized plugin architecture, its performance benefits, and how to create and add new plugins.