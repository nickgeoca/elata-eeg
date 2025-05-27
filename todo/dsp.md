# DSP Architecture: "Applets" Model

This document outlines the refined "Applets" model for implementing Digital Signal Processing (DSP) capabilities within the EEG system's daemon. This approach favors higher-level, purpose-driven processing chains over a single, client-configured pipeline.

## Core Concept: DSP Applets

Instead of a single, generic DSP pipeline configured by the client, the daemon will host multiple "DSP Applets". Each applet is a self-contained Rust module responsible for a specific data processing task and provides a ready-to-use data stream.

**Examples of Applets:**

*   `gui_voltage_applet.rs`: Provides a data stream optimized for real-time GUI voltage display (e.g., includes a 4Hz high-pass filter).
*   `fft_spectrum_applet.rs`: Provides a stream of FFT data, potentially with its own specific pre-processing.
*   `bci_feedback_applet.rs`: Implements logic for a specific Brain-Computer Interface paradigm, possibly using FFT data or other processed signals.
*   `raw_data_applet.rs`: Provides access to nearly raw data with minimal essential processing (e.g., only calibration).

## Advantages of the Applets Model:

1.  **Clearer Abstraction:** Clients request a specific "data product" (e.g., "GUI-optimized voltage") rather than managing a complex chain of DSP stages.
2.  **Simplified Client Logic:** The Kiosk UI (or other clients) connects to a dedicated WebSocket endpoint for each applet, reducing client-side configuration complexity.
3.  **Encapsulation:** Each applet encapsulates its specific DSP logic and necessary components. Internal details (like specific filter parameters for the `gui_voltage_applet`) are hidden unless explicitly exposed as configurable.
4.  **Discoverability:** The daemon can provide a mechanism for clients to discover available applets and their functionalities.

## Proposed Daemon Structure:

*   **`daemon/src/applets/`**:
    *   This directory will contain the individual applet modules (e.g., `gui_voltage_applet.rs`, `fft_spectrum_applet.rs`).
    *   Each applet module will define its processing logic and manage its data stream.
*   **`daemon/src/dsp_components/`** (or `daemon/src/processing_blocks/`):
    *   This directory will house reusable DSP building blocks that applets can import and use.
    *   Examples: `filters.rs` (various filter implementations), `fft_processor.rs`, `windowing_functions.rs`, `signal_statistics.rs`.
    *   This promotes code reuse and consistency across different applets.

## WebSocket Routing:

*   Each active applet will expose its data stream via a distinct WebSocket endpoint.
*   The daemon's server logic will manage these routes.
*   **Example Routes:**
    *   `ws://<daemon_address>/ws/applet/gui_voltage`
    *   `ws://<daemon_address>/ws/applet/fft_spectrum`
    *   `ws://<daemon_address>/ws/applet/bci_feedback`

## Configuration of Applets:

*   While applets provide a higher-level abstraction, they can still be **configurable**.
*   The Kiosk UI (or other clients) could use the existing `/config` WebSocket endpoint (or a new dedicated endpoint for applet configuration) to:
    1.  **Discover Applets:** Get a list of available applets and a brief description of their purpose.
    2.  **Get Applet Parameters:** For a selected applet, retrieve its list of configurable parameters (e.g., cutoff frequency for an internal filter, FFT window size).
    3.  **Set Applet Parameters:** Send commands to update these parameters for a specific applet instance.
*   The daemon would be responsible for applying these configurations to the respective applet.

## User Customization:

*   **Initial Phase:** Users select from pre-defined applets and configure their exposed parameters via the Kiosk UI.
*   **Advanced / Long-Term:**
    *   Consider allowing users to define "meta-applets" or "custom applets" by chaining existing `dsp_components` through a configuration file (e.g., JSON/YAML). This is a step towards more flexible user-defined pipelines without requiring users to write Rust code directly.
    *   The idea of users contributing their own Rust code for new applets or `dsp_components` remains a powerful but complex long-term possibility, requiring careful consideration of security, compilation, and API stability.

## Noted Ambiguities & Further Considerations:

*   **Applet Discovery Mechanism:** Define the specific protocol for clients to discover available applets and their configurable parameters (e.g., a new message type on `/config` or a dedicated REST/WebSocket endpoint).
*   **Parameter Scope:** Determine if applet parameters are global for that applet type or instance-specific if multiple clients can use the same applet type with different settings. (Likely instance-specific if resources allow, or global with clear indication).
*   **Resource Management:** If multiple applets are active, or multiple clients connect to applets, the daemon must efficiently manage processing resources and data flow to avoid redundancy and maintain performance.
*   **Inter-Applet Communication/Dependency:** Consider if applets might need to consume data from other applets (e.g., `bci_feedback_applet` consuming data from `fft_spectrum_applet`). This adds complexity but could be powerful. Initially, applets might primarily consume raw/minimally processed data.
*   **Dynamic Loading/Unloading of Applets:** For resource efficiency, should applets be loaded/started only when a client requests them and unloaded/stopped when no longer in use?
*   **Error Handling & State Management:** Robust error handling within applets and clear communication of applet state/errors to clients.
*   **Persistence of Applet Configurations:** How user-defined configurations for applets are saved and reloaded.