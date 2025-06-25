# Investigation: Kiosk Circular Graph Freezing

**Date:** 2025-06-24
**Status:** In Progress. The initial refactoring attempt was unsuccessful. This document outlines the investigation and steps taken.

## 1. Problem Summary

The user reported that the "Circular Graph" visualization in the Kiosk UI freezes after running for a short time. The graph UI itself is responsive, but the data visualization stops updating. A persistent message shows "Data Points: 1600", which was found to be the total size of the renderer's internal buffer, not the number of points being received. This indicated the data stream to the component had been severed.

## 2. Root Cause Analysis

The root cause was identified as a critical logic error in the "pull" data-fetching model used by the `EegMonitor` component.

*   **The Buffer:** `EegDataContext` maintains a data buffer that is capped at a maximum of 100 `SampleChunk`s. When new data arrives and the buffer is full, the oldest chunk is removed to make space.
*   **The Flawed Tracking:** `EegMonitor` was tracking its position in this buffer by remembering the total length of the buffer from the previous render (`circularGraphLastProcessedLengthRef`).
*   **The Failure Condition:** Once the buffer reached its cap of 100 chunks, its length stopped increasing. The `EegMonitor`'s position tracker also settled at 100. The logic to get new data (`rawSamples.slice(100)`) began permanently returning an empty array, as the buffer's length would never exceed 100. This starved the circular graph of data, causing it to freeze.

## 3. Architectural Decision

After discussing the fragility of the pull model for this specific use case, we decided that a **Publish/Subscribe (Pub/Sub)** architecture was the correct and most robust long-term solution. This approach eliminates the need for consumer components to track their position in the buffer, making the system more reliable and scalable—a key consideration for an open-source project with a plugin architecture.

## 4. Implementation Steps Taken

The following changes were implemented to refactor the data flow to the Pub/Sub model:

### Phase 1: Refactor `EegDataContext` (The Publisher)
*   **File:** [`kiosk/src/context/EegDataContext.tsx`](../kiosk/src/context/EegDataContext.tsx)
*   **Changes:**
    *   Introduced a `subscribeRaw` method to the context.
    *   Components can now provide a callback function to subscribe to the live data stream.
    *   The context was updated to "push" new data chunks to all active subscribers as soon as the data arrives from the WebSocket.

### Phase 2: Refactor `EegMonitor` (The Subscriber)
*   **File:** [`kiosk/src/components/EegMonitor.tsx`](../kiosk/src/components/EegMonitor.tsx)
*   **Changes:**
    *   The old, buggy `useEffect` that pulled data from the context was completely removed.
    *   It was replaced with a new `useEffect` that calls `subscribeRaw` when the circular graph is active, and unsubscribes on cleanup.
    *   This ensures the component receives data via the new push mechanism, fixing the data flow.

### Phase 3: Optimize `EegCircularGraph` (The Consumer)
*   **File:** [`plugins/eeg-circular-graph/ui/EegCircularGraph.tsx`](../plugins/eeg-circular-graph/ui/EegCircularGraph.tsx)
*   **Changes:**
    *   The component's data handling logic was corrected.
    *   Previously, it only rendered the single last sample from each data batch.
    *   The logic was updated to loop through and render *all* samples in the batch, ensuring a smooth and complete visualization.

### Phase 4: Project Configuration Fix
*   **File:** [`kiosk/tsconfig.json`](../kiosk/tsconfig.json)
*   **Changes:**
    *   After the code changes, TypeScript errors ("Cannot find module 'react'") appeared for the plugin files.
    *   This was because the `plugins` directory was not included in the `tsconfig.json`.
    *   I added `../plugins/**/*.ts` and `../plugins/**/*.tsx` to the "include" array to resolve this, making the TypeScript server aware of the plugin source files.

## 5. Plan for Next Session

Since the issue persists, we need to perform live debugging. The failure is likely a subtle runtime issue not caught by TypeScript.

1.  **Check Browser Console:** Look for any runtime errors when the graph freezes.
2.  **Trace Data Flow with Logs:** Add `console.log` statements at key points to watch the data move through the new system:
    *   In `EegDataContext`, log the data chunk right before it's published to subscribers.
    *   In the `EegMonitor` subscription callback, log that data has been received.
    *   In `EegCircularGraph`, log the `data` prop as it's received.
3.  **Verify WebSocket:** Ensure the underlying WebSocket connection is stable and not silently closing or failing to send data.