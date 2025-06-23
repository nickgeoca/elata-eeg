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
import {
    DEFAULT_SAMPLE_RATE,
    DEFAULT_BATCH_SIZE,
    WINDOW_DURATION,
    FFT_WINDOW_DURATION_MS, // Import from constants
    FFT_HOP_DURATION_MS     // Import from constants
} from '../utils/eegConstants';
 
interface EegDataHandlerProps {
  config: any;
  onDataUpdate: (dataReceived: boolean) => void;
  onError?: (error: string) => void;
  linesRef: React.MutableRefObject<any[]>; // Array of WebglStep instances (e.g., WebglLineRoll)
  lastDataChunkTimeRef: React.MutableRefObject<number[]>; // Ref holding array of per-channel timestamps
  latestTimestampRef: React.MutableRefObject<number>; // Ref holding the single latest timestamp
  debugInfoRef: React.MutableRefObject<{
    lastPacketTime: number;
    packetsReceived: number;
    samplesProcessed: number;
  }>; // Ref for debug information including packet count
  onFftData?: (channelIndex: number, fftOutput: number[]) => void; // New callback for FFT data
}

export function useEegDataHandler({
  config,
  onDataUpdate,
  onError,
  linesRef,
  lastDataChunkTimeRef,
  latestTimestampRef,
  debugInfoRef,
  onFftData // Destructure the new FFT callback
}: EegDataHandlerProps) {
  const [status, setStatus] = useState('Connecting...');
  const wsRef = useRef<WebSocket | null>(null);
  // handleMessageRef is no longer needed at this scope
  const dataReceivedTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectAttemptsRef = useRef<number>(0);
  const isProduction = process.env.NODE_ENV === 'production';
  
  // Enhanced debugging state
  const [debugInfo, setDebugInfo] = useState({
    connectionAttempts: 0,
    lastConnectionTime: 0,
    messagesReceived: 0,
    lastMessageTime: 0,
    lastMessageType: 'none',
    lastError: '',
    binaryPacketsReceived: 0,
    textPacketsReceived: 0,
  });
  // No queues or animation frame needed for immediate display
  const sampleBuffersRef = useRef<Float32Array[]>([]); // For raw data display
  // const fftBuffersRef = useRef<number[][]>([]); // Removed: FFT calculation is now backend-driven
  // const samplesSinceLastFftRef = useRef<number[]>([]); // Removed: FFT calculation is now backend-driven
  
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
 
    const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
    const wsProtocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'wss' : 'ws';
    // Connect to basic EEG data endpoint
    const ws = new WebSocket(`${wsProtocol}://${wsHost}:8080/eeg`);
    wsRef.current = ws;
    
    // Set binary type for WebSocket
    ws.binaryType = 'arraybuffer';
    
    ws.onopen = () => {
      setStatus('Connected');
      reconnectAttemptsRef.current = 0; // Reset reconnect attempts on successful connection
      const now = performance.now();
      setDebugInfo(prev => ({
        ...prev,
        connectionAttempts: prev.connectionAttempts + 1,
        lastConnectionTime: now,
      }));
      console.log(`[EegDataHandler DEBUG] WebSocket connection established at ${new Date().toISOString()}`);
      console.log(`[EegDataHandler DEBUG] Connection URL: ${wsProtocol}://${wsHost}:8080/eeg`);
      console.log(`[EegDataHandler DEBUG] Binary type set to: ${ws.binaryType}`);
    };
    
    // --- WebSocket Message Handler ---
    // Initialize queues based on channel count whenever config changes (handled in useEffect below)

    const handleWebSocketMessage = (event: MessageEvent) => {
      try {
        const now = performance.now();
        setDebugInfo(prev => ({
          ...prev,
          messagesReceived: prev.messagesReceived + 1,
          lastMessageTime: now,
          lastMessageType: event.data instanceof ArrayBuffer ? 'binary' : typeof event.data,
        }));

        console.log(`[EegDataHandler DEBUG] Message received at ${new Date().toISOString()}`);
        console.log(`[EegDataHandler DEBUG] Message type: ${event.data instanceof ArrayBuffer ? 'ArrayBuffer' : typeof event.data}`);
        console.log(`[EegDataHandler DEBUG] Message size: ${event.data instanceof ArrayBuffer ? event.data.byteLength : 'N/A'} bytes`);

        // Handle binary data from EEG endpoint
        if (!(event.data instanceof ArrayBuffer)) {
          console.warn("[EegDataHandler DEBUG] Received non-binary data:", typeof event.data, event.data);
          setDebugInfo(prev => ({
            ...prev,
            textPacketsReceived: prev.textPacketsReceived + 1,
          }));
          return;
        }

        setDebugInfo(prev => ({
          ...prev,
          binaryPacketsReceived: prev.binaryPacketsReceived + 1,
        }));

        const buffer = new Uint8Array(event.data);
        console.log(`[EegDataHandler DEBUG] Binary buffer length: ${buffer.length} bytes`);
        
        if (buffer.length === 0) {
          console.warn("[EegDataHandler DEBUG] Received empty data packet");
          return;
        }

        const configuredChannelCount = currentConfig?.channels?.length || 0;
        console.log(`[EegDataHandler DEBUG] Configured channel count: ${configuredChannelCount}`);
        
        if (configuredChannelCount === 0) {
          console.warn("[EegDataHandler DEBUG] No channels configured, skipping packet");
          return;
        }

        // NEW PARSING LOGIC
        const dataView = new DataView(event.data);
        let offset = 0;

        // Packet structure: [timestamp_u64_le] [error_flag_u8] [data_payload]
        if (buffer.length < 9) { // 8 bytes for timestamp + 1 for error flag
          console.warn(`[EegDataHandler DEBUG] Data packet too small for header: ${buffer.length} bytes (need at least 9)`);
          return;
        }

        // Read timestamp (8 bytes, little endian) - using BigInt for u64
        const timestamp = dataView.getBigUint64(offset, true);
        offset += 8;
        console.log(`[EegDataHandler DEBUG] Packet timestamp: ${timestamp}`);

        // Read error flag (1 byte)
        const errorFlag = dataView.getUint8(offset);
        offset += 1;
        console.log(`[EegDataHandler DEBUG] Error flag: ${errorFlag}`);

        if (errorFlag === 1) {
          // Handle error message
          const errorMsg = new TextDecoder().decode(buffer.slice(offset));
          console.error(`[EegDataHandler] Received error from backend: ${errorMsg}`);
          if (typeof onError === 'function') onError(errorMsg);
          return;
        }

        // Calculate batch size from remaining data
        const dataBytes = buffer.length - offset;
        const bytesPerSample = 4; // f32
        const totalSamples = dataBytes / bytesPerSample;
        const batchSize = totalSamples / configuredChannelCount;

        console.log(`[EegDataHandler DEBUG] Data bytes after header: ${dataBytes}`);
        console.log(`[EegDataHandler DEBUG] Total samples: ${totalSamples}`);
        console.log(`[EegDataHandler DEBUG] Calculated batch size: ${batchSize}`);

        if (dataBytes % (bytesPerSample * configuredChannelCount) !== 0) {
            console.warn(`[EegDataHandler DEBUG] Incomplete data packet. Data bytes: ${dataBytes}, Channels: ${configuredChannelCount}, Remainder: ${dataBytes % (bytesPerSample * configuredChannelCount)}`);
            return;
        }

        if (batchSize === 0) {
          console.warn("[EegDataHandler DEBUG] Received packet with no sample data");
          return;
        }

        if (!Number.isInteger(batchSize)) {
          console.warn(`[EegDataHandler DEBUG] Non-integer batch size: ${batchSize}`);
          return;
        }

        // Ensure we have enough sample buffers
        if (sampleBuffersRef.current.length < configuredChannelCount) {
          sampleBuffersRef.current = Array(configuredChannelCount).fill(null).map((_, i) => sampleBuffersRef.current[i] || null);
        }

        // Process data for each channel
        for (let ch = 0; ch < configuredChannelCount; ch++) {
          let currentSampleBuffer = sampleBuffersRef.current[ch];

          if (!currentSampleBuffer || currentSampleBuffer.length !== batchSize) {
            currentSampleBuffer = new Float32Array(batchSize);
            sampleBuffersRef.current[ch] = currentSampleBuffer;
          }

          // Read samples for this channel
          for (let i = 0; i < batchSize; i++) {
            const sampleOffset = offset + (i * configuredChannelCount + ch) * 4;
            const rawValue = dataView.getFloat32(sampleOffset, true); // little endian
            currentSampleBuffer[i] = isFinite(rawValue) ? rawValue : 0;
            
            // DEBUG: Log some sample values for first few packets
            if (ch === 0 && i < 3 && debugInfoRef.current.packetsReceived < 5) {
              console.log(`[EegDataHandler DEBUG] Ch${ch} Sample${i}: ${rawValue} (finite: ${isFinite(rawValue)}, offset: ${sampleOffset})`);
            }
          }

          console.log(`[EegDataHandler DEBUG] Channel ${ch}: processed ${batchSize} samples, first few: [${currentSampleBuffer.slice(0, 3).join(', ')}]`);

          // Update timestamps
          if (lastDataChunkTimeRef.current && lastDataChunkTimeRef.current[ch] !== undefined) {
            lastDataChunkTimeRef.current[ch] = performance.now();
          }

          // Add data to WebGL lines
          if (linesRef.current && linesRef.current[ch] && batchSize > 0) {
            linesRef.current[ch].shiftAdd(currentSampleBuffer);
            console.log(`[EegDataHandler DEBUG] Added ${batchSize} samples to WebGL line ${ch}`);
          } else {
            console.warn(`[EegDataHandler DEBUG] Could not add data to WebGL line ${ch}: linesRef.current=${!!linesRef.current}, line exists=${!!(linesRef.current && linesRef.current[ch])}, batchSize=${batchSize}`);
          }
        }

        // Update global timestamp
        if (latestTimestampRef) {
          latestTimestampRef.current = performance.now();
        }

        // Update debug info
        if (debugInfoRef) {
          debugInfoRef.current.packetsReceived++;
          debugInfoRef.current.lastPacketTime = performance.now();
          debugInfoRef.current.samplesProcessed += batchSize * configuredChannelCount;
        }

        console.log(`[EegDataHandler DEBUG] Successfully processed packet ${debugInfoRef.current.packetsReceived}, calling onDataUpdate(true)`);

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
        console.error("[EegDataHandler DEBUG] Error parsing EEG binary data:", error);
        console.error("[EegDataHandler DEBUG] Error stack:", error instanceof Error ? error.stack : 'No stack trace');
        setDebugInfo(prev => ({
          ...prev,
          lastError: error instanceof Error ? error.message : String(error),
        }));
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
  // linesRef, lastDataChunkTimeRef, latestTimestampRef, debugInfoRef, onDataUpdate, onError, onFftData are refs/callbacks assumed stable.
  }, [isProduction, linesRef, lastDataChunkTimeRef, latestTimestampRef, debugInfoRef, onDataUpdate, onError, onFftData]);
  
  /**
   * Effect for managing WebSocket connection and sample processing interval.
   */
  useEffect(() => {
    const currentConfig = config; // Capture config for this effect run
    const numChannels = currentConfig?.channels?.length || 0;
    const sampleRate = currentConfig?.sample_rate || DEFAULT_SAMPLE_RATE;

    // Initialize/Reset FFT buffers when config changes (e.g., channel count) - REMOVED
    // if (numChannels > 0) {
    //   fftBuffersRef.current = Array(numChannels).fill(null).map(() => []);
    //   samplesSinceLastFftRef.current = Array(numChannels).fill(0);
    // } else {
    //   fftBuffersRef.current = [];
    //   samplesSinceLastFftRef.current = [];
    // }
 
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
  // onFftData is added to dependencies of connectWebSocket, so not strictly needed here if connectWebSocket handles it.
  // However, including config directly ensures re-initialization of FFT buffers if channel count changes.
  }, [config, connectWebSocket, isProduction, linesRef, latestTimestampRef, debugInfoRef]);
  // Return status and debug info
  return {
    status,
    debugInfo: !isProduction ? debugInfo : undefined
  };
}