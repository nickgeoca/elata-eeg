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
  // No queues or animation frame needed for immediate display
  
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
    
    // --- WebSocket Message Handler ---
    // Initialize queues based on channel count whenever config changes (handled in useEffect below)

    const handleWebSocketMessage = (event: MessageEvent) => {
      try {
        if (!(event.data instanceof ArrayBuffer)) {
          return;
        }
        const dataView = new DataView(event.data);

        // Timestamp parsing removed - not used in immediate display mode

        const errorFlag = dataView.getUint8(8); // Error flag at offset 8
        if (errorFlag === 1) {
          const errorBytes = new Uint8Array(event.data.slice(9));
          const errorMessage = new TextDecoder().decode(errorBytes);
          console.error("EEG driver error:", errorMessage);
          if (typeof onError === 'function') onError(`EEG driver error: ${errorMessage}`);
          return;
        }

        const channelCount = currentConfig?.channels?.length || 0;
        const sampleRate = currentConfig?.sample_rate || DEFAULT_SAMPLE_RATE;
        if (channelCount === 0 || sampleRate <= 0) return;

        // Queue logic removed
        // Data starts at offset 9
        const samplesPerChannel = Math.floor((event.data.byteLength - 9) / 4 / channelCount);
        if (samplesPerChannel <= 0) {
            if (!isProduction) console.warn("Received packet with no samples.");
            return;
        }

        let bufferOffset = 9; // Start reading samples after timestamp (8) and error flag (1)
        for (let ch = 0; ch < channelCount; ch++) {
          const samples = new Float32Array(samplesPerChannel);
          for (let i = 0; i < samplesPerChannel; i++) {
            const sampleBufferOffset = bufferOffset + i * 4;
            if (sampleBufferOffset + 4 <= event.data.byteLength) {
              const rawValue = dataView.getFloat32(sampleBufferOffset, true);
              samples[i] = isFinite(rawValue) ? rawValue : 0;
            } else {
              samples[i] = 0; // Handle potential buffer overrun
              if (!isProduction) console.warn(`Buffer overrun detected at sample ${i}, channel ${ch}`);
            }
          }
          bufferOffset += samplesPerChannel * 4; // Move offset for the next channel

          // Update per-channel timestamp
          if (lastDataChunkTimeRef.current) {
              lastDataChunkTimeRef.current[ch] = performance.now();
          }
          // Feed data directly into WebGL line
          if (linesRef.current && linesRef.current[ch]) {
            // Log the first few samples to check if they are non-zero
            if (!isProduction && samples.length > 0) {
                console.log(`[EegDataHandler] Ch ${ch} samples (first 5):`, samples.slice(0, 5));
            }
            linesRef.current[ch].shiftAdd(samples); // Add the whole batch
          } else if (!isProduction) {
            // console.warn(`linesRef missing or channel ${ch} not initialized`); // Reduce noise
          }
        }
        // Update the single latest timestamp ref after processing all channels for this packet
        if (latestTimestampRef) {
            // Use performance.now() for the latest timestamp in immediate mode
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
    }; // End of handleWebSocketMessage

    // Assign the raw message handler
    ws.onmessage = handleWebSocketMessage;
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
   * Effect for managing WebSocket connection and sample processing interval.
   */
  useEffect(() => {
    const currentConfig = config; // Capture config for this effect run
    const numChannels = currentConfig?.channels?.length || 0;
    const sampleRate = currentConfig?.sample_rate || DEFAULT_SAMPLE_RATE;

    // --- Connect WebSocket ---
    if (currentConfig) {
      connectWebSocket(currentConfig);
    }

    // --- No Sample Processing Loop Needed ---
    // Data is processed directly in handleWebSocketMessage

    // --- Cleanup Function ---
    return () => {
      if (!isProduction) {
        console.log("Cleaning up EegDataHandler effect...");
      }

      // Clear reconnect timeout
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
        reconnectTimeoutRef.current = null;
        if (!isProduction) console.log("Reconnect timeout cleared.");
      }

      // Clear data received timeout
      if (dataReceivedTimeoutRef.current) {
        clearTimeout(dataReceivedTimeoutRef.current);
        dataReceivedTimeoutRef.current = null;
        if (!isProduction) console.log("Data received timeout cleared.");
      }

      // Close WebSocket connection
      if (wsRef.current) {
        if (!isProduction) console.log("Closing WebSocket connection...");
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
  // Dependencies: Re-run effect if config changes that affect connection or processing rate/channels.
  // Also include connectWebSocket as it's defined outside but used inside.
  }, [config, connectWebSocket, isProduction, linesRef, latestTimestampRef]); // Removed lastDataChunkTimeRef, onDataUpdate, onError as they are stable refs/callbacks
  // Return status (FPS is now implicitly handled by sample rate)
  return { status };
}