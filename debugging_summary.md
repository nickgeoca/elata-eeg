# WebSocket Connection Debugging Summary

## 1. Initial Problem

The Kiosk web application fails to connect to the backend daemon's WebSocket, displaying a "Disconnected" status. The initial error was a **404 Not Found** for the endpoint `ws://raspberrypi.local:3000/api/ws`.

## 2. Debugging Chronology & Evolving Hypotheses

Our debugging process has been a process of elimination, peeling back layers of the problem.

### Attempt 1: Incorrect Frontend URL & Proxy
*   **Hypothesis:** The frontend is using an outdated URL, and the Next.js proxy isn't configured for WebSockets.
*   **Actions:**
    1.  Corrected the WebSocket URL in `kiosk/src/context/EventStreamContext.tsx` from `/api/ws` to `/ws/data` based on outdated documentation.
    2.  Later, corrected it again to the true path, `/ws`, based on the routing code in `crates/daemon/src/server.rs`.
    3.  Modified `kiosk/next.config.js` to properly proxy WebSocket requests.
*   **Result:** The 404 error was resolved, but a new error appeared: **500 Internal Server Error**. This indicated the request was now reaching the daemon but causing it to crash.

### Attempt 2: Backend Panic (`.unwrap()`)
*   **Hypothesis:** The daemon is panicking due to an unhandled error, likely an `.unwrap()` on a `Result` or `Option`.
*   **Action:** Replaced a potential `.unwrap()` on `serde_json::to_vec` in `crates/daemon/src/websocket_broker.rs` with graceful error handling.
*   **Result:** The **500 Internal Server Error** persisted, proving the `.unwrap()` was not the root cause.

### Attempt 3: Axum State Injection
*   **Hypothesis:** The `axum` web server is not correctly injecting the `AppState` into the `websocket_handler`, causing a panic before the handler's code executes. This was based on the observation that our added logs were not appearing.
*   **Action:** Refactored the router construction in `crates/daemon/src/server.rs` to ensure the state was provided correctly using `.with_state()`.
*   **Result:** The **500 Internal Server Error** persisted. This was a strong indicator that the issue was more complex than a simple configuration error.

### Attempt 4: Incorrect State Extraction
*   **Hypothesis:** The `websocket_handler` function signature was attempting to extract a *part* of the `AppState` (`State(broker): State<Arc<WebSocketBroker>>`) instead of accepting the *whole* state (`State(state): State<AppState>`), which is how the `axum` extractor works.
*   **Action:** Corrected the function signature in `crates/daemon/src/server.rs` to match the working pattern.
*   **Result:** The **500 Internal Server Error** still persists. This is the most baffling result, as the code now appears logically correct according to `axum`'s patterns.

## 3. Key Breakthrough: The Minimal Reproducible Example

To escape the cycle of failed fixes, we created a minimal, standalone test server (`crates/daemon/src/ws_test.rs`).

*   **Action:** Built a tiny `axum` server with a mock `AppState` and a WebSocket handler.
*   **Result:** **Success.** The test server runs and accepts WebSocket connections perfectly.

## 4. Current Status & Next Steps

*   **Current Core Problem:** The main daemon application (`main.rs` and `server.rs`) still crashes with a **500 Internal Server Error** on WebSocket connection, even though the code now mirrors the structure of the working `ws_test.rs` example.
*   **Confirmed:** The issue is **not** with the environment, core libraries (`axum`), or the fundamental logic of stateful WebSocket handlers.
*   **Leading Hypothesis:** There is a subtle but critical difference in the **initialization or composition of the `AppState`** in `main.rs` compared to the simple, working mock state in `ws_test.rs`. Something within the complex, real `AppState` (e.g., the `Mutex`-wrapped `HashMap`, the `flume` channels, the `tokio::sync::broadcast` channel) is not `Clone`-able or `Send`-able in a way that `axum`'s state layer can handle, causing a panic when the state is moved into the handler.

**When you return, the next logical step is to:**

1.  **Compare `main.rs` and `ws_test.rs` again**, but this time focus intensely on the `AppState` struct and how each of its fields is created and shared.
2.  Systematically comment out fields from the real `AppState` in `main.rs` and replace them with mock/default versions (like in `ws_test.rs`) to pinpoint which specific component is causing the crash.