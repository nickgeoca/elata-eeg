# Plan: Fix EEG "No Data" Issue

**Date:** 2025-06-24
**Status:** Proposed

## 1. Problem Summary

After fixing the component re-render loop, the UI is now stable but incorrectly displays a "Status: no data" message, even though the WebSocket connection is successfully established. This indicates that the data stream is not being correctly identified by the frontend.

## 2. Root Cause Analysis

The root cause is a logic error in the `onmessage` handler within `kiosk/src/components/EegDataHandler.tsx`. The code that signals a healthy data flow (`onDataUpdate(true)`) and resets the "no data" timer is located exclusively within the processing block for binary `ArrayBuffer` messages.

If the backend sends any text-based messages after the connection is established (e.g., a subscription confirmation, status update), this logic is never triggered. The application therefore never acknowledges that data is being received, leading to the erroneous "no data" status.

## 3. Solution: Acknowledge All Message Types

The solution is to treat any message from the WebSocket—binary or text—as an indication of a live connection.

### Implementation Steps

1.  **File to Modify:** `kiosk/src/components/EegDataHandler.tsx`

2.  **Action:**
    *   Locate the `onmessage` handler.
    *   Find the block of code responsible for managing the data-received status:
        ```typescript
        onDataUpdateRef.current?.(true);

        if (dataReceivedTimeoutRef.current) {
          clearTimeout(dataReceivedTimeoutRef.current);
        }
        dataReceivedTimeoutRef.current = setTimeout(() => {
          onDataUpdateRef.current?.(false);
        }, 1000);
        ```
    *   Move this entire block from inside the `if (event.data instanceof ArrayBuffer)` statement to the main body of the `onmessage` handler, so it executes for every message.

## 4. Expected Outcome

- The frontend will correctly recognize both binary and text messages as signs of an active data stream.
- The "Status: no data" message will no longer appear as long as the WebSocket connection is active and sending any type of message.
- The graph will correctly display data as soon as binary packets are received.