# Implementation Plan: FFT Protocol Refinement

The goal is to implement a new versioned, topic-based binary protocol for our WebSocket communications. This will resolve the current FFT display issue and create a more robust, maintainable system.

Here is a visual representation of the new data flow:

```mermaid
graph TD
    subgraph Backend
        A[Plugins e.g., brain_waves_fft] -- "SensorEvent::WebSocketBroadcast { topic, payload: Bytes }" --> B{Event Bus};
        B -- "SensorEvent::WebSocketBroadcast" --> C[ConnectionManager];
        C -- "Prepends [VERSION, TOPIC] bytes" --> D[WebSocket Server];
    end

    subgraph Frontend
        D -- "Binary Message: [VERSION, TOPIC, PAYLOAD]" --> E[EegDataHandler.tsx];
        E -- "Parses VERSION, TOPIC" --> F{Topic Router};
        F -- "topic == Fft" --> G[handleFftPayload(payload)];
        F -- "topic == FilteredEeg" --> H[handleEegPayload(payload)];
        G --> I[FFT UI Component];
        H --> J[EEG Monitor UI Component];
    end

    style A fill:#f9f,stroke:#333,stroke-width:2px
    style C fill:#f9f,stroke:#333,stroke-width:2px
    style E fill:#ccf,stroke:#333,stroke-width:2px
```

---

### Phased Rollout

We will implement this in four distinct phases:

**Phase 1: Backend Foundation (`eeg_types` Crate)** - ✅ **Completed**
*   **Goal:** Establish the core data structures for the new protocol.
*   **Outcome:** The `bytes` dependency was added and `event.rs` was updated with the new `WebSocketTopic` enum and `SensorEvent::WebSocketBroadcast` variant.

**Phase 2: Backend Logic (Plugins & Connection Manager)** - ✅ **Completed**
*   **Goal:** Adapt backend components to use the new protocol.
*   **Outcome:** `basic_voltage_filter` and `brain_waves_fft` plugins were updated to broadcast data using the new event. The `ConnectionManager` was refactored into a content-agnostic forwarder.

**Phase 3: Frontend Implementation (`kiosk`)** - ✅ **Completed**
*   **Goal:** Update the frontend to parse the new protocol.
*   **Outcome:** `EegDataHandler.tsx` was refactored to parse the new binary protocol, identify topics, and route payloads to the correct handlers.

**Phase 4: Testing & Debugging** - 🟡 **In Progress**
*   **Goal:** Ensure the end-to-end solution is robust and correct.
*   **Current Status (End of Session):**
    *   The new binary protocol is **working**. The frontend is successfully receiving binary messages.
    *   The `EegDataHandler` correctly identifies and routes both `FilteredEeg` (Topic 0) and `Fft` (Topic 1) payloads.
    *   **New Issue:** While data is flowing, the FFT graph is not rendering, showing a "Loading Brain Waves Applet..." message instead. This indicates the issue lies downstream from the data handler, likely within the UI components themselves.

**Phase 5: Frontend Rendering Fix (Next Session)**
*   **Goal:** Diagnose and fix the FFT rendering issue in the Kiosk UI.
*   **Files to Investigate:**
    *   `plugins/brain_waves_fft/ui/FftDisplay.tsx`
    *   `plugins/brain_waves_fft/ui/FftRenderer.tsx`
    *   `kiosk/src/components/EegMonitor.tsx`
    *   `kiosk/src/context/EegDataContext.tsx`
*   **Plan for Tomorrow:**
    1.  **Verify Data Flow:** Add logging to confirm the `onFftData` callback in `EegDataHandler` passes a valid, parsed `FftPacket` object to the context.
    2.  **Trace Prop Drilling:** Follow the `fftData` prop from `EegDataContext` through `EegMonitor` to the `FftDisplay` component, ensuring it's not being lost or malformed.
    3.  **Component-Level Debugging:** Inspect the props and state within `FftDisplay.tsx` to understand why the "Loading..." state persists.
    4.  **Analyze Renderer:** Examine the logic in `FftRenderer.tsx` to see if it's receiving data correctly and if there are any conditions preventing it from drawing the graph.

---