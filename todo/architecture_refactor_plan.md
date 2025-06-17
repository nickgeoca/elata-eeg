# Architecture Refactor Implementation Plan (v0.6)

This document outlines the concrete steps required to refactor the codebase to align with the simplified, single-plugin architecture defined in `todo/new_directories.md`.

**Guiding Principles:**
- **Single active plugin:** The system manages one plugin at a time.
- **Plugin-owned DSP:** All DSP logic resides within the plugin's backend.
- **System-wide configuration:** Settings like sample rate and channel count are defined in `config.json`, not by the plugin.
- **Simplified data flow:** The core system passes raw `AdcData` to the plugin without processing.

---

## Phase 1: Decouple and Simplify the `driver` Crate

**Goal:** Make the driver crate (`sensor`) a pure, hardware-focused library with no DSP logic.

1.  **Delete Obsolete DSP Module:**
    -   [ ] **DELETE** the entire directory: `driver/src/dsp/`
    -   **Reason:** This contains the centralized `DspCoordinator` and `SignalProcessor` which are being replaced by the plugin-owned DSP model.

2.  **Refactor `EegSystem` (`driver/src/eeg_system/mod.rs`):**
    -   [ ] **Modify `EegSystem::new` and `EegSystem::start`:**
        -   Remove all code and comments related to `SignalProcessor`.
        -   The `EegSystem` should no longer be aware of any DSP concepts.
    -   [ ] **Modify the processing task:**
        -   The output channel type should be changed from `mpsc::Sender<ProcessedData>` to `mpsc::Sender<AdcData>`.
        -   The `process_data_batch` helper function should be removed.
        -   The main loop inside `tokio::spawn` should now directly send the `AdcData` received from the `DriverEvent::Data(data_batch)` to the output channel.
    -   [ ] **Remove `ProcessedData` struct dependency:**
        -   The `use super::ProcessedData;` line should be removed. This struct is no longer relevant to the driver.

3.  **Rename Crate `driver` -> `sensor`:**
    -   [ ] **Rename Directory:** Rename the `driver/` directory to `sensor/`.
    -   [ ] **Update `daemon/Cargo.toml`:** Change the dependency from `eeg_driver` to `eeg_sensor`.
        ```toml
        # Before
        eeg_driver = { path = "../driver" }
        # After
        eeg_sensor = { path = "../sensor" }
        ```
    -   [ ] **Update `daemon/src/main.rs`:** Change `use eeg_driver::{...}` to `use eeg_sensor::{...}`.

---

## Phase 2: Overhaul the `daemon` Crate

**Goal:** Transform the daemon into a lean orchestrator that manages the sensor and a single active plugin.

1.  **Remove Centralized DSP and Connection Management:**
    -   [ ] **Modify `daemon/src/main.rs`:**
        -   **DELETE** `use eeg_driver::dsp::coordinator::DspCoordinator;`.
        -   **DELETE** `use connection_manager::ConnectionManager;`.
        -   **DELETE** all code that initializes or uses `DspCoordinator` and `ConnectionManager` (lines 107-114, 140, 193, etc.).
        -   **DELETE** the `tx_filtered_eeg_data` broadcast channel. There is only one data stream now.

2.  **Implement Plugin Management Logic:**
    -   [ ] **Create new module `daemon/src/plugin_manager.rs`:**
        -   This module will be responsible for:
            -   Scanning the `plugins/` directory.
            -   Parsing `plugin.toml` files.
            -   Loading the active plugin (either spawning its backend process or loading its `.dylib`).
            -   Providing a simple interface like `plugin_manager.send_data(&data)`.
    -   [ ] **Integrate `PluginManager` into `daemon/src/main.rs`:**
        -   Initialize the `PluginManager` on startup.
        -   In the main data loop, instead of broadcasting to WebSockets, get the `AdcData` from the `EegSystem` and pass it to the `PluginManager`.

3.  **Simplify the Main Loop:**
    -   [ ] **Refactor `daemon/src/main.rs`'s `main` function:**
        -   The complex `tokio::select!` loop for handling reconfiguration can be simplified.
        -   Reconfiguration will now be a simpler process:
            1. Receive command from Kiosk.
            2. Call `eeg_system.reconfigure(new_config)`.
            3. Inform the active plugin of the new configuration (e.g., new channel count).

4.  **Update Configuration Handling:**
    -   [ ] **Modify `daemon/src/config.rs` and `daemon/src/main.rs`:**
        -   Ensure that `sample_rate` and `channels` are loaded from `config.json` and passed into the `AdcConfig`.
        -   The Kiosk will send commands to update these values, which the daemon will persist back to `config.json` and use to reconfigure the `EegSystem`.

---

## Phase 3: Final Cleanup

1.  **Verify `Cargo.toml` files:**
    -   [ ] Ensure all workspace dependencies are correct after the `driver` -> `sensor` rename.
2.  **Update `README.md`:**
    -   [ ] Briefly document the new, simplified architecture.