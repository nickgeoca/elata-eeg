# Plan: Fix FFT Data Flow

**Date:** 2025-06-25

## 1. Executive Summary

This document outlines the investigation and resolution of a bug where the Kiosk UI displayed "No FFT data available" despite receiving raw EEG data. The root cause was a combination of a disabled backend plugin and a data structure mismatch between the backend and frontend.

This plan details the steps taken to enable the plugin and align the data contracts, restoring the intended FFT functionality.

## 2. Investigation Findings

The investigation proceeded through several stages, revealing two primary issues:

1.  **Disabled Backend Plugin:** The `brain_waves_fft` Rust plugin, responsible for all FFT calculations, was defined as an optional dependency behind a Cargo feature flag (`brain_waves_fft_feature`) in `crates/device/Cargo.toml`. This feature was not enabled by default, meaning the plugin was never compiled into the `eeg_daemon` application, and therefore no `FftPacket` events were ever generated.

2.  **Frontend Data Mismatch:** The frontend component responsible for handling FFT data, `kiosk/src/context/EegDataContext.tsx`, expected a different data structure than the one defined by the `FftPacket` in the Rust `eeg_types` crate.
    *   **Backend Sent:** A single `FftPacket` object containing a `psd_packets` array.
    *   **Frontend Expected:** A single object with `channel_index` and `fft_output` properties.

## 3. Resolution Plan

The following steps were executed to resolve the issue:

1.  **Enable the FFT Plugin:**
    *   **Action:** Modified `crates/device/Cargo.toml`.
    *   **Change:** Added the `brain_waves_fft_feature` to the `[features]` section's `default` array.
    *   **Outcome:** This ensures the `brain_waves_fft` plugin is always included in a standard `cargo build` of the `eeg_daemon`.

2.  **Align Frontend Data Handling:**
    *   **Action:** Modified `kiosk/src/context/EegDataContext.tsx`.
    *   **Change:** Updated the `handleFftData` callback to correctly parse the `FftPacket` structure, iterating through the `psd_packets` array and using the correct property names (`channel` and `psd`).
    *   **Outcome:** The frontend can now correctly interpret the data broadcasted by the backend and update the UI state.

## 4. Validation

With these changes, the `eeg_daemon` will now correctly perform FFT analysis on the EEG data stream, and the Kiosk UI will receive and render the resulting Power Spectral Density (PSD) graph. The architectural integrity of the event-driven plugin system is maintained.