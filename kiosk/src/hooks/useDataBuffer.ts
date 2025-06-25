'use client';

import { useRef, useCallback } from 'react';

/**
 * A custom hook to manage a buffer for streaming data.
 * It's designed to decouple data reception from data processing/rendering.
 *
 * @template T The type of data chunks being buffered.
 * @returns An object with methods to interact with the buffer.
 */
export function useDataBuffer<T>() {
  const buffer = useRef<T[]>([]);

  /**
   * Adds a new data chunk to the buffer.
   * This operation is lightweight and designed to be called frequently
   * (e.g., from a WebSocket onmessage handler or a subscription callback).
   * @param newData The new data chunk to add.
   */
  const addData = useCallback((newData: T[]) => {
    buffer.current.push(...newData);
  }, []);

  /**
   * Retrieves all data currently in the buffer and clears it.
   * This is intended to be called by a consumer (e.g., a rendering component
   * inside a requestAnimationFrame loop) to process data in batches.
   * @returns An array of all the data chunks that were in the buffer.
   */
  const getAndClearData = useCallback(() => {
    if (buffer.current.length === 0) {
      return [];
    }
    const bufferedData = buffer.current;
    buffer.current = [];
    return bufferedData;
  }, []);

  return {
    addData,
    getAndClearData,
  };
}