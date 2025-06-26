# Plan to Fix Graph Data Disappearing Issue

## 1. Problem Diagnosis

The EEG graph data disappears when cycling through different views (Signal -> Circular -> FFT -> Signal). Console logs indicate that the `EegDataHandler` WebSocket connection is closing unexpectedly (`code: 1006`) and then attempting to reconnect.

The root cause was traced to the main `useEffect` hook in `kiosk/src/components/EegDataHandler.tsx`. This hook, responsible for managing the WebSocket lifecycle, had a dependency on `configKey`. The `configKey` is derived from the EEG configuration (sample rate, channels, etc.).

When switching views, the application logic would cause a change in the configuration, which in turn changed the `configKey`, triggering a complete teardown and reconnection of the WebSocket. This brief period of disconnection caused the interruption in the data stream.

## 2. Solution

The fix involves making the WebSocket connection persistent and independent of configuration changes that do not require a full connection restart.

### Key Changes:

1.  **Decouple WebSocket from `configKey`**:
    *   In `kiosk/src/components/EegDataHandler.tsx`, remove `configKey` from the dependency array of the main `useEffect` hook that establishes the WebSocket connection.
    *   This ensures the `useEffect` runs only once on component mount, creating a stable, long-lived WebSocket connection.

2.  **Rely on Dynamic Subscription Messages**:
    *   The application already has the necessary logic to send `subscribe` and `unsubscribe` messages to the backend when the `subscriptions` prop changes.
    *   This existing mechanism is sufficient for managing which data streams (e.g., `FilteredEeg`, `Fft`) are active without needing to tear down the entire connection. The connection itself remains open, and only the data being sent over it changes based on these messages.

By implementing these changes, the WebSocket connection will remain active as the user switches between views, ensuring a continuous flow of data to the frontend and resolving the "disappearing data" issue.