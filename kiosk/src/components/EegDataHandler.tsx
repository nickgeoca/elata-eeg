'use client';

/**
 * EegDataHandler.tsx
 *
 * This component handles WebSocket connections to the EEG data server and processes incoming data.
 *
 * This implementation uses a constant FPS rendering approach, removing the need for
 * render flags and simplifying the overall rendering process.
 */

import { useEffect, useRef, useState, useCallback } from 'react';
import throttle from 'lodash.throttle';
import { DEFAULT_SAMPLE_RATE, DEFAULT_BATCH_SIZE, WINDOW_DURATION } from '../utils/eegConstants';

interface EegDataHandlerProps {
  config: any;
  onDataUpdate: (dataReceived: boolean) => void;
  onError?: (error: string) => void;
  linesRef: React.MutableRefObject<any[]>; // Array of WebglStep instances (e.g., WebglLineRoll)
  lastDataChunkTimeRef: React.MutableRefObject<number[]>; // Ref holding array of per-channel timestamps
  latestTimestampRef: React.MutableRefObject<number>; // Ref holding the single latest timestamp
}

export function useEegDataHandler({
  config,
  onDataUpdate,
  onError,
  linesRef,
  lastDataChunkTimeRef,
  latestTimestampRef // Destructure the new prop
}: EegDataHandlerProps) {
  const [status, setStatus] = useState('Connecting...');
  const wsRef = useRef<WebSocket | null>(null);
  // handleMessageRef is no longer needed at this scope
  const dataReceivedTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectAttemptsRef = useRef<number>(0);
  const isProduction = process.env.NODE_ENV === 'production';
  const currentMessageHandler = useRef<any>(null); // Keep track of the active throttled handler
  
  // createMessageHandler logic is now moved inside connectWebSocket



  /**
   * Function to establish WebSocket connection with automatic reconnection
   */
  const connectWebSocket = useCallback((currentConfig: any) => {
    // Clear any existing reconnect timeout
    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current);
      reconnectTimeoutRef.current = null;
    }
    
    // Close existing connection if any
    if (wsRef.current) {
      try {
        wsRef.current.close();
      } catch (e) {
        // Ignore errors on close
      }
    }
    
    setStatus('Connecting...');
    
    // Use currentConfig passed to the function
    if (!currentConfig) {
        console.warn("connectWebSocket called without config.");
        return;
    }

    const ws = new WebSocket('ws://localhost:8080/eeg');
    wsRef.current = ws;
    
    // Set binary type for WebSocket
    ws.binaryType = 'arraybuffer';
    
    ws.onopen = () => {
      setStatus('Connected');
      reconnectAttemptsRef.current = 0; // Reset reconnect attempts on successful connection
      if (!isProduction) {
        console.log('WebSocket connection established');
      }
    };
    
    // --- Create message handler logic moved inside ---
    const interval = 1000 / (currentConfig?.fps || 60);
    if (!isProduction) {
      console.log(`Setting throttle interval to ${interval.toFixed(2)}ms (${(1000 / interval).toFixed(2)} FPS)`);
    }

    // Cancel previous handler if it exists
    if (currentMessageHandler.current) {
        currentMessageHandler.current.cancel();
    }

    const messageHandler = throttle((event: MessageEvent) => {
      try {
        if (!(event.data instanceof ArrayBuffer)) {
          return;
        }
        const dataView = new DataView(event.data);

        const errorFlag = dataView.getUint8(8);
        if (errorFlag === 1) {
          const errorBytes = new Uint8Array(event.data.slice(9));
          const errorMessage = new TextDecoder().decode(errorBytes);
          console.error("EEG driver error:", errorMessage);
          if (typeof onError === 'function') onError(`EEG driver error: ${errorMessage}`);
          return;
        }

        const channelCount = currentConfig?.channels?.length || 0; // Use currentConfig
        if (channelCount === 0) return; // Avoid division by zero

        const samplesPerChannel = Math.floor((event.data.byteLength - 9) / 4 / channelCount);
        if (samplesPerChannel <= 0) return; // No data samples

        let offset = 9;
        for (let ch = 0; ch < channelCount; ch++) {
          const samples = new Float32Array(samplesPerChannel);
          for (let i = 0; i < samplesPerChannel; i++) {
            const sampleOffset = offset + i * 4;
            if (sampleOffset + 4 <= event.data.byteLength) {
              const value = dataView.getFloat32(sampleOffset, true);
              samples[i] = isFinite(value) ? value : 0;
            } else {
              samples[i] = 0; // Handle potential buffer overrun
            }
          }
          offset += samplesPerChannel * 4;

          // Update per-channel timestamp
          if (lastDataChunkTimeRef.current) {
              lastDataChunkTimeRef.current[ch] = performance.now();
          }
          // Feed data into WebGL line
          if (linesRef.current && linesRef.current[ch]) {
            linesRef.current[ch].shiftAdd(samples);
          } else if (!isProduction) {
            // console.warn(`linesRef missing or channel ${ch} not initialized`); // Reduce noise
          }
        }
        // Update the single latest timestamp ref after processing all channels for this packet
        if (latestTimestampRef) {
            latestTimestampRef.current = performance.now();
        }
        // Update the single latest timestamp ref after processing all channels for this packet
        if (latestTimestampRef) {
            latestTimestampRef.current = performance.now();
        }

        if (typeof onDataUpdate === 'function') {
          onDataUpdate(true);
        }

        if (dataReceivedTimeoutRef.current) {
          clearTimeout(dataReceivedTimeoutRef.current);
        }
        dataReceivedTimeoutRef.current = setTimeout(() => {
          if (typeof onDataUpdate === 'function') {
            onDataUpdate(false);
          }
        }, 1000);

      } catch (error) {
        console.error("Error parsing EEG binary data:", error);
        if (typeof onError === 'function') onError(`Error parsing EEG data: ${error}`);
      }
    }, interval, { leading: true, trailing: true }); // Use leading: true as well

    currentMessageHandler.current = messageHandler; // Store the handler
    ws.onmessage = messageHandler;
    // --- End of moved message handler logic ---
    
    ws.onclose = (event) => {
      if (!isProduction) {
        console.log(`WebSocket closed with code: ${event.code}, reason: ${event.reason}`);
      }
      
      setStatus('Disconnected');
      
      // Implement exponential backoff for reconnection
      const maxReconnectDelay = 5000; // Maximum delay of 5 seconds
      const baseDelay = 500; // Start with 500ms delay
      const reconnectDelay = Math.min(
        maxReconnectDelay,
        baseDelay * Math.pow(1.5, reconnectAttemptsRef.current)
      );
      
      reconnectAttemptsRef.current++;
      
      if (!isProduction) {
        console.log(`Attempting to reconnect in ${reconnectDelay}ms (attempt ${reconnectAttemptsRef.current})`);
      }
      
      // Schedule reconnection
      reconnectTimeoutRef.current = setTimeout(() => {
        if (!isProduction) {
          console.log('Attempting to reconnect...');
        }
        // Pass the config again when reconnecting
        connectWebSocket(currentConfig);
      }, reconnectDelay);
    };
    
    ws.onerror = (error) => {
      if (!isProduction) {
        console.error('WebSocket error:', error);
      }
      // Don't update timestamp on error, just report it
      // if (latestTimestampRef) {
      //     latestTimestampRef.current = performance.now();
      // }
      if (typeof onError === 'function') onError(`WebSocket error: ${error}`);
      // onclose will handle reconnection attempt
    };

  // Dependencies: Only include stable references or primitives if possible.
  // config is passed directly when called.
  // linesRef, lastDataChunkTimeRef, latestTimestampRef, onDataUpdate, onError are refs/callbacks assumed stable.
  }, [isProduction, linesRef, lastDataChunkTimeRef, latestTimestampRef, onDataUpdate, onError]); // Keep latestTimestampRef here
  
  /**
   * Set up WebSocket connection with stable lifecycle
   */
  useEffect(() => {
    // Connect when config becomes available or changes
    if (config) {
      connectWebSocket(config);
    }
    
    // Cleanup function remains the same, runs on unmount or when config/connectWebSocket changes
    return () => {
      // Clean up on component unmount or when dependencies change
      if (currentMessageHandler.current) {
        currentMessageHandler.current.cancel();
        currentMessageHandler.current = null;
      }
      
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      
      if (dataReceivedTimeoutRef.current) {
        clearTimeout(dataReceivedTimeoutRef.current);
      }
      
      if (wsRef.current) {
        // Ensure WebSocket is closed cleanly
        try {
          wsRef.current.onclose = null; // Prevent reconnect logic during manual close
          wsRef.current.onerror = null;
          wsRef.current.close();
        } catch (e) {
          // Ignore errors during cleanup close
        }
        wsRef.current = null;
      }
    };
  // useEffect depends on specific config values needed for connection (fps, channels length)
  // and the stable connectWebSocket callback. This prevents unnecessary reconnects
  // if the config object reference changes but these key values do not.
  }, [config?.fps, config?.channels?.length, connectWebSocket]);

  // Get FPS directly from config with no fallback
  const fps = config?.fps || 0;

  return { status, fps: config?.fps || 0 };
}