# Plan: Fix Data Buffering and Consumption Logic

**Date:** 2025-06-24
**Status:** Proposed

## 1. Problem Summary

Despite a stable UI and a confirmed WebSocket connection, the EEG graphs are not rendering any data. The root cause is a structural mismatch in how data is added to the rendering buffer versus how it is consumed.

## 2. Root Cause Analysis

The `useDataBuffer` hook's `addData` method is designed to accept a single item, but it is being passed an array of data chunks. This results in a nested array structure (`SampleChunk[][]`) within the buffer.

While the `EegRenderer` was correctly written to handle this nested structure with two loops, this implementation is unnecessarily complex and prone to errors. The buffer should contain a simple, flat list of data chunks (`SampleChunk[]`).

## 3. Solution: Flatten the Data Buffer

The solution is to correct the `useDataBuffer` hook and simplify the rendering components to work with a flat data array.

### Implementation Steps

1.  **Modify `kiosk/src/hooks/useDataBuffer.ts`:**
    *   Change the `addData` function signature to accept an array of items: `(newData: T[])`.
    *   Use the spread operator to push the items into the buffer: `buffer.current.push(...newData)`. This will create a flat array of data chunks.

2.  **Modify `kiosk/src/components/EegRenderer.tsx`:**
    *   In the `renderLoop`, remove the outer `forEach` loop.
    *   The variable `sampleChunks` (returned from `getAndClearData`) will now be a flat `SampleChunk[]`, so the code should iterate directly over it.

3.  **Modify `kiosk/src/components/CircularGraphWrapper.tsx`:**
    *   This component also uses `useDataBuffer`. It must be updated similarly.
    *   The rendering logic within its `requestAnimationFrame` loop needs to be simplified to handle a flat data array, removing any nested loops.

## 4. Expected Outcome

- The data buffer will have a simple, flat structure (`SampleChunk[]`).
- The rendering components will correctly and efficiently process the data from the buffer.
- The EEG graphs will finally render the data as it is received.