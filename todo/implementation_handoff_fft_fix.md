# Implementation Handoff: FFT Plugin and Protocol Fix

## 1. Current Status

The investigation and architectural planning phase for the `brain_waves_fft` plugin issue is **complete**.

A comprehensive plan was developed, refined with user feedback, and approved. The final plan is documented in detail in `todo/fft_protocol_refinement_plan.md`.

## 2. High-Level Objective

The objective is to implement the approved plan, which involves creating a new versioned, topic-based binary protocol for all WebSocket communications. This will fix the immediate bug while adhering to the project's decoupled plugin architecture.

## 3. Next Steps: The Starting Point

The implementation will begin with the backend `eeg_types` crate. This is the foundational step upon which all other changes will be built.

**File to be Modified First:** `crates/eeg_types/src/event.rs`

**Specific Changes for the First Task:**

1.  **Add Protocol Version:** Introduce a public constant for the wire protocol version.
    ```rust
    pub const PROTOCOL_VERSION: u8 = 1;
    ```
2.  **Add WebSocketTopic Enum:** Define the specific topics for our data streams.
    ```rust
    #[repr(u8)]
    pub enum WebSocketTopic {
        FilteredEeg = 0,
        Fft = 1,
        Log = 255,
    }
    ```
3.  **Update SensorEvent Enum:** Add the new `WebSocketBroadcast` variant. This requires importing the `Bytes` type.
    ```rust
    use bytes::Bytes;

    pub enum SensorEvent {
        // ... existing internal events
        
        WebSocketBroadcast {
            topic: WebSocketTopic,
            payload: Bytes,
        },
    }
    ```

## 4. Key Files for Subsequent Implementation Steps

After the `eeg_types` crate is updated, the following files will be modified in sequence:

1.  **Plugins (`plugins/brain_waves_fft/src/lib.rs`, etc.):** Update plugins to serialize their data to `Bytes` and use the new `WebSocketBroadcast` event.
2.  **Connection Manager (`crates/device/src/connection_manager.rs`):** Simplify the manager to be a content-agnostic forwarder that prepends the `VERSION` and `TOPIC` headers.
3.  **Frontend Handler (`kiosk/src/components/EegDataHandler.tsx`):** Implement the client-side parsing logic to read the headers and route the payload to the correct handler.

This document provides a clear and detailed starting point for our next session. Thank you for the productive collaboration today!