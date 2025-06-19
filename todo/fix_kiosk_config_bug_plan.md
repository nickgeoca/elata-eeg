# Refactoring Plan: Robust WebSocket Connection Handling

## 1. Problem

The Kiosk application is experiencing frequent WebSocket disconnections (error code 1006) from the `eeg_daemon`. This prevents the frontend from receiving the initial configuration and subsequent data streams, rendering the application unusable.

The root cause has been identified as an overly complex and deadlock-prone implementation of the WebSocket connection handlers in `crates/device/src/server.rs`. The use of a combined `tokio::select!` loop with `mpsc` channels for both incoming and outgoing messages creates race conditions and potential deadlocks, leading to abrupt connection closures by the server.

## 2. Solution

This plan refactors the WebSocket handling logic to be simpler, more robust, and free of deadlocks. It complements the existing event-driven architecture by ensuring the data produced by the internal pipeline is reliably delivered to clients.

The solution involves three main steps:

1.  **Create a `connection_manager.rs` Module:** A new module will be created to provide a generic, reusable, and robust pattern for handling WebSocket connections. It will encapsulate the best practice of splitting the WebSocket into independent "sender" and "receiver" tasks, which run concurrently. This prevents a slow receiver from blocking the sender and vice-versa.

2.  **Refactor `handle_config_websocket`:** This function will be simplified to use the new `ConnectionManager`. It will no longer use the complex `select!` loop or the intermediate `mpsc` channel. The logic will be cleanly separated:
    *   A dedicated task will listen for broadcasted configuration updates and send them to the client.
    *   A separate task will handle incoming messages from the client.

3.  **Refactor `handle_eeg_websocket`:** This function will also be updated to use the `ConnectionManager`, simplifying its logic for streaming EEG data and ensuring it does not get blocked by client-side message handling.

## 3. Implementation Steps

1.  **Create `crates/device/src/connection_manager.rs`:**
    *   Define a generic `ConnectionManager` that can be used by all WebSocket handlers.
    *   Implement the logic to split the WebSocket and manage the sender/receiver tasks.

2.  **Update `crates/device/src/server.rs`:**
    *   Add `mod connection_manager;` to bring the new module into scope.
    *   Rewrite `handle_config_websocket` to use `ConnectionManager`.
    *   Rewrite `handle_eeg_websocket` to use `ConnectionManager`.
    *   The `handle_command_websocket` can also be updated to follow this pattern for consistency.

4.  **Update `todo/README.md`:**
    *   Add an entry for this new plan file.

This refactoring will resolve the disconnection bug and significantly improve the stability and maintainability of the daemon's network communication layer.