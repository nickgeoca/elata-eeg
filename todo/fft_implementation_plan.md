# Plan: Implement FFT Power Spectral Density Analysis

**Date:** 2025-06-25

## 1. Goal

To process `FilteredEeg` data streams, perform Fast Fourier Transform (FFT) analysis, calculate the Power Spectral Density (PSD) in units of µV²/Hz, and broadcast the results as `FftPacket` events.

## 2. Proposed Architecture & Data Flow

The plugin's internal logic will follow a standard digital signal processing (DSP) pipeline. Each channel's data will be processed independently.

```mermaid
graph TD
    A[EventBus: Receives SensorEvent::FilteredEeg] --> B{BrainWavesPlugin};
    B --> C[Append Samples to Channel Buffers];
    C --> D{Buffer Full?};
    D -- No --> C;
    D -- Yes --> E[Apply Hann Window Function];
    E --> F[Perform FFT];
    F --> G[Calculate Power Spectral Density (µV²/Hz)];
    G --> H[Construct FftPacket with PSD data];
    H --> I[EventBus: Publish SensorEvent::Fft];
```

## 3. Detailed Implementation Steps

### Step 1: Add Dependencies

The following crates will be added to `crates/device/Cargo.toml` for numerical computation and signal processing:

*   **`rustfft`**: The primary library for performing the Fast Fourier Transform.
*   **`num-complex`**: Provides the complex number type required by `rustfft`.
*   **`hann-window`**: A small crate to apply a Hann windowing function to the signal, which is crucial for reducing spectral leakage.

### Step 2: Refactor `BrainWavesPlugin` for State Management

The plugin must be refactored to manage state (data buffers, FFT planner).

1.  **Remove `Default`:** Remove the `Default` trait from the `BrainWavesPlugin` struct.
2.  **Update Struct Fields:** Add fields to the struct to manage state, including buffers for each channel, an FFT planner, and configuration constants (`fft_size`, `sample_rate`).
3.  **Create `new()` Constructor:** Implement a `BrainWavesPlugin::new()` function to initialize the state.
4.  **Update Plugin Supervisor:** Modify `crates/device/src/plugin_supervisor.rs` to use `BrainWavesPlugin::new()` for instantiation.

### Step 3: Implement Core DSP Logic in `run()`

The main `run()` method will be updated to execute the DSP pipeline.

1.  **Data Buffering:** As `FilteredEeg` events arrive, append the samples to the correct channel's buffer.
2.  **Windowing & FFT:** When a buffer contains enough samples for an FFT:
    *   Copy the buffered data into a temporary buffer.
    *   Apply the Hann windowing function.
    *   Execute the FFT using the planner.
3.  **Power Calculation:** Convert the complex FFT output into a Power Spectral Density (PSD). The result will be in units of µV²/Hz.
4.  **Output:** Construct the `FftPacket` containing the raw PSD data for each channel and publish it to the event bus.
5.  **Buffer Management (Overlapping Windows):** Implement overlapping windows to create a smoother data stream. After an FFT, retain a portion of the old data (e.g., 50%) before appending new samples.

## 4. Validation

The primary validation will be observing the `eeg-circular-graph` in the Kiosk UI correctly rendering the live PSD data, confirming the end-to-end data flow is working as expected.