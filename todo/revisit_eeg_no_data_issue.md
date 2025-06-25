# Revisiting the "No Data" Issue in EEG Monitor

**Date:** 2025-06-24

## 1. Problem Summary

The Kiosk UI displays a "no data" status for the EEG graphs, despite WebSocket connections appearing to be established. Console logs indicate that the data WebSocket (`/ws/data`) is closing unexpectedly with code 1006 and then entering a reconnect loop. This prevents a stable flow of EEG data to the frontend.

## 2. Investigation and Actions Taken

Our investigation pointed to a cascading re-render issue originating from an unstable `config` object reference.

### Initial Diagnosis:
- **`EegConfig.tsx`:** The `EegConfigProvider` was creating a new `config` object on every message, even if the data was identical.
- **`EegDataContext.tsx`:** This new object reference was passed down, causing the `useEegDataHandler` hook to re-run.
- **`EegDataHandler.tsx`:** The hook's `useEffect` was triggered by the changing `config` prop, causing it to tear down and re-establish the data WebSocket connection continuously.

### Fixes Implemented:

1.  **Stabilized Configuration (`kiosk/src/components/EegConfig.tsx`):**
    *   **Action:** Modified the `onmessage` handler to perform a deep comparison of the incoming configuration with the existing one using the `areConfigsEqual` helper.
    *   **Change:** The `setConfig` state updater is now only called if the configuration has actually changed.
    *   **Goal:** To break the re-render loop and provide a stable `config` object reference to downstream components.

2.  **Corrected React Hook Violation (`kiosk/src/components/EegRenderer.tsx`):**
    *   **Action:** Initially, I added an early return to prevent rendering when container dimensions were invalid. This violated the Rules of Hooks.
    *   **Change:** I reverted this change. The existing logic within the `useEffect` hooks already correctly handles the case where dimensions are not ready, preventing the WebGL plot from initializing prematurely without violating hook rules.

## 3. Current Status

Despite the fixes, the "no data" issue persists. This indicates that while the configuration stability was a valid issue, it may not have been the sole cause, or my fix was incomplete.

### Next Steps & Hypotheses:

1.  **Verify the Fix:** The first step for tomorrow is to re-verify that the change in `EegConfig.tsx` is behaving as expected. We should add logging to confirm that `setConfig` is no longer being called unnecessarily.
2.  **Backend WebSocket Endpoint:** The issue might be on the server side. The `/ws/data` endpoint could be closing the connection for reasons unrelated to the frontend's behavior. We need to investigate the backend daemon's logs (`crates/device/src/server.rs`) to see why it's closing the connection with code 1006.
3.  **Subscription Logic:** There might be an issue with the subscription messages being sent from `EegDataHandler`. If the backend doesn't receive a valid subscription, it might close the connection. We should review the subscription logic in `EegDataHandler.tsx` and the corresponding handling logic on the backend.
4.  **Data Parsing:** An error during the parsing of incoming binary data in `EegDataHandler.tsx` could be causing an unhandled exception that crashes the WebSocket handler.

By creating this document, we can pick this up with fresh eyes and a clear plan.