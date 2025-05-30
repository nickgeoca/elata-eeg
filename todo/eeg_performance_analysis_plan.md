# EEG System Performance Analysis Plan

This document outlines a plan to analyze the performance of the EEG system, from data acquisition through various processing stages, including applet-specific DSP like FFT.

## I. Information Gathering & Context Refinement

1.  **Confirm Applet DSP Execution Location**:
    *   Search in `daemon/` source code (e.g., `daemon/src/main.rs`, `daemon/src/server.rs`) for how applet manifests (like `applets/brain_waves/manifest.json`) are loaded and how `dsp` components (e.g., "brain_waves_fft" specified in the manifest) are invoked. This includes how their WebSocket endpoints (e.g., `dspWebSocketPath`) are set up. This will confirm if FFT or other applet-specific DSP runs within the `daemon` process on the Raspberry Pi.
    *   Examine the source code of applet DSP libraries (e.g., `dsp/brain_waves_fft/src/lib.rs`) to understand their inputs (likely `ProcessedData` or similar from the `driver`), outputs (e.g., FFT results), and computational characteristics.
2.  **Review Kiosk UI for Applet Data Consumption**:
    *   Briefly examine Kiosk UI components that render applet data (e.g., `applets/brain_waves/ui/AppletFftRenderer.tsx`) to understand how they consume data from applet-specific WebSockets (e.g., `/applet/brain_waves/data`) and what data format/structure they expect.
    *   Check the primary data handler in Kiosk (e.g., `kiosk/src/components/EegDataHandler.tsx`) to understand how it handles the main `/eeg` data stream from the `daemon`.
3.  **Identify Key Configuration Parameters Affecting Performance**:
    *   From `driver` (`AdcConfig`): Sample rate, number of active channels, IIR filter settings (types, order, cutoff frequencies).
    *   From `daemon` (`DaemonConfig`): CSV recording settings (e.g., `max_recording_length_minutes`, output path), WebSocket batching strategies (if any).
    *   From applet DSP libraries (e.g., `dsp/brain_waves_fft/src/lib.rs`): Parameters for algorithms like FFT (window size, overlap, specific FFT algorithm used).

## II. Performance Analysis & Bottleneck Identification

1.  **Driver Performance (Acquisition & IIR Filtering on RPi5)**:
    *   **Focus**: CPU usage on Raspberry Pi 5, particularly related to the `SignalProcessor` (`driver/src/dsp/filters.rs`) applying IIR filters.
    *   **Metrics**:
        *   Latency from hardware DRDY signal to `ProcessedData` being available to the `daemon`.
        *   CPU cycles or processing time per sample per channel for IIR filtering.
        *   SPI communication throughput and latency.
    *   **Concerns**: Impact of high sample rates and/or many active channels on RPi5 CPU capacity. Potential for `rppal` (GPIO/SPI library) overhead.
2.  **Daemon Performance (Core Tasks on RPi5)**:
    *   **CSV Recording**:
        *   **Focus**: Disk I/O throughput and latency on the RPi5's storage (likely SD card).
        *   **Metrics**: Time taken to write data batches to CSV, impact of recording on other `daemon` tasks and overall system responsiveness. CPU overhead of CSV formatting.
        *   **Concerns**: SD card write speed limitations, especially with high data rates.
    *   **`/eeg` WebSocket Streaming (Filtered Data)**:
        *   **Focus**: Network throughput from RPi5, CPU overhead for data transformation (`ProcessedData` to `EegBatchData`) and binary serialization (`server::create_eeg_binary_packet`).
        *   **Metrics**: End-to-end latency for this stream to the Kiosk client. Maximum data rate sustainable over WebSocket.
        *   **Concerns**: Network interface limitations on RPi5, WebSocket protocol overhead.
3.  **Daemon Performance (Applet-Specific DSP on RPi5 - e.g., FFT)**:
    *   **Focus**: **CPU usage on RPi5 for applet DSP calculations** (e.g., by the `brain_waves_fft` library if run server-side). Memory usage for these calculations.
    *   **Metrics**: Time to compute applet DSP (e.g., FFT) per data window/batch. Impact on `daemon` responsiveness, other data streams (`/eeg`, CSV), and overall RPi5 CPU/memory utilization.
    *   **Concerns**: This is a **potential major performance hotspot** if complex DSP like FFT runs on the RPi5 for multiple channels/high rates.
4.  **Overall System Throughput & Latency**:
    *   **Metrics**:
        *   End-to-end latency: from analog signal at electrodes to data displayed in Kiosk UI (for both the "raw-ish" filtered stream and any applet-processed streams).
        *   Maximum sustainable sample rate and channel count before system overload (evidenced by increased latency, data gaps, errors, or RPi5 CPU saturation).
    *   **Concerns**: Interaction effects between different processing stages and resource contention (CPU, memory, disk I/O, network) on the RPi5.

## III. Visualization of Data Flow & Processing Stages

Create/Refine a Mermaid diagram to clearly illustrate the complete data flow, highlighting processing stages, data types, and interfaces between components (`driver`, `daemon`, applet DSP, Kiosk UI).

```mermaid
graph TD
    subgraph Raspberry Pi 5
        subgraph Driver Process (driver)
            A[EEG Hardware Interface (ADS1299 via SPI/DRDY)] --> B{Raw ADC Data};
            B --> C[Parse & Convert to Voltage];
            C --> D[DSP: IIR Filters (SignalProcessor)];
            D -- ProcessedData --> E(MPSC Channel to Daemon);
        end

        subgraph Daemon Process (daemon)
            E --> F{Received ProcessedData};
            F --> G[Optional: CSV Recorder];
            G -- CSV Files --> H[Disk Storage (SD Card)];

            F --> I[Transform to EegBatchData];
            I --> J[Broadcast Channel for /eeg];
            J --> K[WebSocket Server: /eeg (Binary Stream)];

            F --> L{Applet DSP Dispatcher (if applet active)};
            L -- ProcessedData --> M[Applet DSP: e.g., brain_waves_fft (FFT Calc)];
            M -- FFT Results --> N[Broadcast Channel for Applet Data];
            N --> O[WebSocket Server: /applet/brain_waves/data (Applet Stream)];
        end
    end

    subgraph Kiosk UI (Client Machine)
        K -- EEG Data Stream --> P[EegDataHandler & Renderer];
        O -- Applet FFT Data Stream --> Q[AppletFftRenderer];
    end

    style Driver Process fill:#lightblue,stroke:#333,stroke-width:2px
    style Daemon Process fill:#lightgreen,stroke:#333,stroke-width:2px
    style Kiosk UI fill:#lightyellow,stroke:#333,stroke-width:2px

    classDef bottleneck fill:#ffcccc,stroke:#cc0000,stroke-width:2px;
    class D,M,G bottleneck;
```

## IV. Potential Optimizations & Recommendations (Initial High-Level)

*   **Offloading Intensive DSP**: If applet DSP (like FFT) running on the RPi5 is a significant bottleneck:
    *   Consider client-side processing in the Kiosk UI (using JavaScript or WebAssembly compiled from Rust). This shifts CPU load to the client machine.
    *   Evaluate if a more powerful dedicated co-processor for DSP is feasible (though likely outside scope of software-only changes).
*   **Driver Optimizations**:
    *   Review `biquad` filter implementations for efficiency.
    *   Ensure optimal use of `tokio` for asynchronous operations.
    *   Profile SPI communication for any unexpected latencies.
*   **Daemon Optimizations**:
    *   **CSV Writing**: Ensure robust buffering and fully asynchronous writes to minimize impact on real-time tasks.
    *   **WebSocket Streaming**: Optimize serialization, consider compression for WebSocket data if bandwidth is a constraint (trade-off with CPU).
    *   **Task Management**: Efficiently manage concurrent tasks, especially if multiple applets with server-side DSP might run.
*   **Configuration Tuning**: Provide guidance on realistic sample rates, channel counts, and DSP complexity suitable for the RPi5's capabilities to manage user expectations and prevent system overload.
*   **Resource Monitoring**: Implement or recommend tools for monitoring RPi5 CPU, memory, disk I/O, and network usage to help identify bottlenecks during operation.

## V. Final Report Structure (Deliverable)

1.  **Executive Summary**: Overview of system performance, key findings, and top recommendations.
2.  **Detailed Data Flow and Processing Architecture**: Updated analysis and diagrams.
3.  **Performance Analysis by Component**:
    *   Driver (Acquisition, IIR Filtering).
    *   Daemon (Core tasks: CSV, `/eeg` WebSocket).
    *   Daemon (Applet-specific DSP, e.g., FFT).
4.  **Identified Bottlenecks and Resource Constraints**: Detailed discussion of limitations on the RPi5 (CPU, Disk I/O, Network, Memory).
5.  **Recommendations for Optimization**: Prioritized list of actionable recommendations.
6.  **Open Questions / Areas for Deeper Investigation**: Suggestions for further profiling, testing, or measurements needed for more precise analysis.