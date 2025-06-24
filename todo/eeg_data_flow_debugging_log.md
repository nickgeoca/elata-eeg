# EEG Data Flow Debugging Log

This document tracks the investigation and resolution of the EEG data flow and visualization issues reported on 2025-06-23.

## Initial Report

*   **Problem 1:** Slow data rate. The backend sends data at ~31Hz, but the frontend updates at ~2Hz.
*   **Problem 2:** The EEG graph shows a flat line, even when data is being received.
*   **Problem 3:** Console logs are overly verbose and unhelpful.

## Debugging Attempts

### Attempt 1 (Incorrect Diagnosis)

*   **Hypothesis:** The `useEffect` hook in `EegDataHandler.tsx` was re-triggering due to an unstable `config` object reference, causing constant WebSocket reconnections. The flat line was due to an incorrect vertical scaling factor in `EegMonitor.tsx`.
*   **Action:**
    1.  Modified `EegDataHandler.tsx` to use a `useMemo` hook to create a stable key for the `useEffect` dependency array.
    2.  Modified `EegMonitor.tsx` to increase the `scaleY` property of the WebGL lines.
*   **Result:** **Failed.** The issue persisted, indicating the root cause was deeper than the `EegDataHandler` component. The "connection interrupted" log confirmed the WebSocket was still being torn down.

### Attempt 2 (Correct Diagnosis & Fix)

*   **Hypothesis:** The true root cause was a stale closure in `EegConfig.tsx`. The `connectWebSocket` function, wrapped in a `useCallback` with an incomplete dependency array, was capturing a stale `config` value (`null`). This caused `areConfigsEqual` to always return `false`, which in turn caused `setConfig` to be called on every message, creating a new `config` object and triggering the downstream cascade of re-renders and WebSocket closures.
*   **Action:**
    1.  Refactored `EegConfig.tsx` to remove the problematic `useCallback` and simplify the logic within the main `useEffect` hook. This ensures the `onmessage` handler always has access to the latest `config` state.
    2.  Corrected the logic in the `onmessage` handler to properly handle initial config vs. subsequent updates, fixing TypeScript errors introduced in the process.
*   **Result:** **Success.** Stabilizing the `config` object in the provider stopped the unnecessary re-renders, which allowed the WebSocket connection to remain stable. This fixed the data rate issue, and with the data flowing correctly, the scaling fix from Attempt 1 could now work as intended, resolving the flat-line graph.

### Attempt 3 (Current Investigation - 2025-06-23)

*   **Correction:** The previous investigation incorrectly focused on the `MockDriver`. The issue is occurring with the `ads1299` hardware driver. The symptoms remain the same: the data rate is ~2Hz instead of the expected ~31.25Hz, and the graph is a flat line.

*   **New Hypothesis:** The problem lies within the `ads1299` driver implementation. Possible causes include:
    *   Incorrect SPI communication setup (clock speed, mode).
    *   Errors in the data acquisition loop, such as excessive delays or blocking operations.
    *   Improper handling of the `DRDY` (Data Ready) signal from the ADS1299 chip.
    *   The chip is not being configured correctly to start data conversion.
    *   The data being read is either all zeros or not being correctly interpreted.

*   **Next Steps:**
    1.  Thoroughly review the `ads1299` driver source code in `crates/sensors/src/ads1299/`.
    2.  Trace the data flow from the hardware interrupt/polling to the point where data is sent to the main application.
    3.  Add targeted logging to the driver to observe SPI commands, register values, and the timing of the data acquisition loop.