# DSP Pipeline Design Notes

This document outlines ideas and considerations for implementing a configurable Digital Signal Processing (DSP) pipeline, primarily for the EEG system.

## Core Recommendations:

1.  **Pipeline Location:**
    *   The primary, configurable DSP pipeline should reside within the **daemon**.
    *   This centralizes logic, promotes code reuse, and simplifies integration of new hardware drivers.
    *   Drivers can still handle very hardware-specific initial processing if necessary.

2.  **Modular Design:**
    *   The pipeline should be composed of **distinct, configurable stages** or modules (e.g., high-pass filter, notch filter, FFT pre-processing, custom algorithms).
    *   This allows different applications or data consumers (e.g., GUI voltage view vs. FFT analysis) to use data processed optimally for their specific needs.
    *   Addresses the concern that a single filter configuration (e.g., a 4Hz high-pass for GUI) might not be suitable for all use cases (e.g., FFT).

3.  **GUI-Based Configuration:**
    *   The Kiosk UI should be able to **configure the DSP pipeline** running in the daemon.
    *   This includes discovering available DSP stages, enabling/disabling them, and setting parameters for each stage.
    *   Communication would likely occur via the existing `/config` WebSocket endpoint.

4.  **Flexible Data Streaming:**
    *   The daemon should support **flexible ways to stream processed data**:
        *   It could offer multiple, distinct WebSocket endpoints for differently processed data.
        *   Alternatively, it could allow the main `/eeg` data stream's processing to be dynamically configured by the client.

5.  **User Customization Strategy:**
    *   **Initial Phase:**
        *   Focus on allowing users to configure pipelines composed of **pre-built DSP modules** (implemented in Rust within the daemon).
        *   Configuration could be driven by the Kiosk UI or potentially loaded from a configuration file (e.g., JSON/YAML) by the daemon.
    *   **Long-Term Vision (Advanced):**
        *   Explore mechanisms for advanced users to **contribute their own custom DSP modules** (e.g., as separate Rust crates or scripts loaded by the daemon). This requires careful consideration of compilation, security, API stability, and sandboxing.

## Noted Ambiguities & Further Considerations:

*   **User-Contributed Modules:** The exact implementation details for allowing user-contributed DSP modules (compilation, security model, module API, versioning, sandboxing) are complex and require significant design effort for a robust long-term solution.
*   **DSP Stage Discovery:** The specific protocol or mechanism for how the Kiosk UI (or other clients) would dynamically discover the available DSP stages and their configurable parameters from the daemon needs to be defined.
*   **Error Handling & Feedback:** Detailed error handling for invalid DSP configurations and clear feedback mechanisms to the user (via the Kiosk UI) are crucial.
*   **Resource Management:** If multiple clients can request differently processed streams, the daemon needs to manage resources efficiently to avoid redundant computations or excessive load.
*   **Real-time Guarantees:** The impact of a flexible, multi-stage DSP pipeline on real-time processing guarantees needs to be assessed, especially if complex user-defined modules are allowed.
*   **Persistence of Configuration:** How user-defined DSP pipeline configurations are saved and reloaded (e.g., per user, per session, system-wide defaults) needs to be decided.