'use client';

/**
 * EegDataHandler.tsx
 *
 * This component handles WebSocket connections to the EEG data server and processes incoming data.
 *
 * This implementation uses a constant FPS rendering approach, removing the need for
 * render flags and simplifying the overall rendering process.
 */

import { useEffect, useRef, useState, useCallback, useMemo } from 'react';
import {
    DEFAULT_SAMPLE_RATE,
    DEFAULT_BATCH_SIZE,
    WINDOW_DURATION,
} from '../utils/eegConstants';
import { SystemConfig } from '@/types/eeg';
import { PipelineState } from '../context/PipelineContext';
 
interface EegDataHandlerProps {
  pipelineState: PipelineState;
  onDataUpdate: (dataReceived: boolean) => void;
  onError?: (error: string) => void;
  onSamples: (samples: { values: Float32Array; timestamps: BigUint64Array }[]) => void;
  lastDataChunkTimeRef: React.MutableRefObject<number[]>; // Ref holding array of per-channel timestamps
  latestTimestampRef: React.MutableRefObject<number>; // Ref holding the single latest timestamp
  debugInfoRef: React.MutableRefObject<{
    lastPacketTime: number;
    packetsReceived: number;
    samplesProcessed: number;
  }>; // Ref for debug information including packet count
  onFftData?: (data: any) => void; // Updated callback for structured FFT data
  subscriptions?: string[]; // Made optional as it's no longer used for data handling
}

export function useEegDataHandler({
  pipelineState,
  onDataUpdate,
  onError,
  onSamples,
  lastDataChunkTimeRef,
  latestTimestampRef,
  debugInfoRef,
  onFftData,
  // subscriptions now has a default value as it's optional
  subscriptions = [],
}: EegDataHandlerProps) {
  const [wsConnectionStatus, setWsConnectionStatus] = useState('Disconnected');
  const wsRef = useRef<WebSocket | null>(null);
  // handleMessageRef is no longer needed at this scope
  const dataReceivedTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectAttemptsRef = useRef<number>(0);
  const isProduction = process.env.NODE_ENV === 'production';
  const logCounterRef = useRef(0); // Ref for throttling logs

  // --- Refs for props to ensure stability ---
  const onDataUpdateRef = useRef(onDataUpdate);
  const onErrorRef = useRef(onError);
  const onFftDataRef = useRef(onFftData);
  const onSamplesRef = useRef(onSamples);
  const subscriptionsRef = useRef(subscriptions);

  // Update refs whenever props change
  useEffect(() => {
    onDataUpdateRef.current = onDataUpdate;
  }, [onDataUpdate]);

  useEffect(() => {
    onErrorRef.current = onError;
  }, [onError]);

  useEffect(() => {
    onFftDataRef.current = onFftData;
  }, [onFftData]);

  useEffect(() => {
    onSamplesRef.current = onSamples;
  }, [onSamples]);

  // Watch for subscription changes and send messages to backend
  useEffect(() => {
    const prevSubscriptions = subscriptionsRef.current;
    subscriptionsRef.current = subscriptions;
    
    // Only send messages if WebSocket is connected and subscriptions actually changed
    if (wsRef.current && wsRef.current.readyState === WebSocket.OPEN &&
        JSON.stringify(prevSubscriptions) !== JSON.stringify(subscriptions)) {
      
      // Find newly added subscriptions
      const newSubscriptions = subscriptions.filter(topic => !prevSubscriptions.includes(topic));
      if (newSubscriptions.length > 0) {
        const message = { type: 'subscribe', topics: newSubscriptions };
        console.log('[EegDataHandler] Sending subscribe message:', message);
        wsRef.current.send(JSON.stringify(message));
      }
      
      // Find removed subscriptions
      const removedSubscriptions = prevSubscriptions.filter(topic => !subscriptions.includes(topic));
      if (removedSubscriptions.length > 0) {
        const message = { type: 'unsubscribe', topics: removedSubscriptions };
        console.log('[EegDataHandler] Sending unsubscribe message:', message);
        wsRef.current.send(JSON.stringify(message));
      }
    }
  }, [subscriptions]);


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
  
  /**
   * Main effect for WebSocket connection management.
   * Runs only once on mount.
   */
  useEffect(() => {
    console.log(`[EegDataHandler] Effect running to establish WebSocket connection.`);
    let isMounted = true;

    const { status, config } = pipelineState;

    // Only connect if the pipeline is started and we have a valid config.
    // Clean up any existing connection and exit the effect.
    if (status !== 'started' || !config) {
      if (wsRef.current) {
        console.log("[EegDataHandler] Ensuring WebSocket is closed due to status or config change.");
        wsRef.current.close();
        wsRef.current = null;
      }
      return;
    }

    // Function to send subscription messages to the backend
    const sendSubscription = (topics: string[], action: 'subscribe' | 'unsubscribe') => {
      if (wsRef.current && wsRef.current.readyState === WebSocket.OPEN) {
        const message = {
          type: action,
          topics: topics
        };
        console.log(`[EegDataHandler] Sending ${action} message:`, message);
        wsRef.current.send(JSON.stringify(message));
      }
    };

    const connectWebSocket = () => {
      
      // Close existing connection if any
      if (wsRef.current) {
        try {
          wsRef.current.close();
        } catch (e) {
          // Ignore errors on close
        }
      }
      
      if (!isMounted) return;
      setWsConnectionStatus('Connecting...');
  
      const wsProtocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'wss' : 'ws';
      const wsHost = typeof window !== 'undefined' ? window.location.host : 'localhost:3000'; // Use the same host as the web page
      const ws = new WebSocket(`${wsProtocol}://${wsHost}/ws/data`);
      wsRef.current = ws;
      
      ws.binaryType = 'arraybuffer';
      
      ws.onopen = () => {
        if (!isMounted) return;
        setWsConnectionStatus('Connected');
        reconnectAttemptsRef.current = 0;
        const now = performance.now();
        setDebugInfo(prev => ({
          ...prev,
          connectionAttempts: prev.connectionAttempts + 1,
          lastConnectionTime: now,
        }));
        console.log(`[EegDataHandler] WebSocket connection established.`);

        // Send initial subscriptions if any
        if (subscriptionsRef.current.length > 0) {
          sendSubscription(subscriptionsRef.current, 'subscribe');
        }
      };
      
      ws.onmessage = (event: MessageEvent) => {
        if (!isMounted) return;

        try {
          const now = performance.now();
          onDataUpdateRef.current?.(true);
          if (dataReceivedTimeoutRef.current) clearTimeout(dataReceivedTimeoutRef.current);
          dataReceivedTimeoutRef.current = setTimeout(() => onDataUpdateRef.current?.(false), 1000);

          setDebugInfo(prev => ({
            ...prev,
            messagesReceived: prev.messagesReceived + 1,
            lastMessageTime: now,
            lastMessageType: event.data instanceof ArrayBuffer ? 'binary' : 'string',
          }));

          if (event.data instanceof ArrayBuffer) {
            setDebugInfo(prev => ({ ...prev, binaryPacketsReceived: prev.binaryPacketsReceived + 1 }));
            const buffer = new Uint8Array(event.data);
            if (buffer.length < 2) {
              console.warn(`[EegDataHandler] Received packet too small for protocol header: ${buffer.length} bytes`);
              return;
            }

            const version = buffer[0];
            const topic = buffer[1];
            const payload = buffer.slice(2); // Zero-copy view of the payload

            if (version !== 1) {
              console.error(`[EegDataHandler] Received unsupported protocol version: ${version}`);
              return;
            }

            switch (topic) {
              case 0: // FilteredEeg
                handleEegPayload(payload);
                break;
              case 1: // Fft
                handleFftPayload(payload);
                break;
              case 255: // Log
                handleLogPayload(payload);
                break;
              default:
                console.warn(`[EegDataHandler] Received message with unknown topic ID: ${topic}`);
            }
          } else {
            // This path can be used for legacy text messages or control signals if needed.
            console.log("[EegDataHandler] Received non-binary message:", event.data);
          }
        } catch (error) {
          console.error("[EegDataHandler] Error in onmessage handler:", error);
          setDebugInfo(prev => ({
            ...prev,
            lastError: error instanceof Error ? error.message : String(error),
          }));
          onErrorRef.current?.(`Error processing data: ${error}`);
        }
      };

      const handleEegPayload = (payload: Uint8Array) => {
        const eegSourceStage = config?.stages.find(s => s.plugin_id === 'eeg_source');
        const configuredChannelCount = eegSourceStage?.params.channel_count || 0;
        if (configuredChannelCount === 0) return;

        const dataView = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
        let offset = 0;

        if (payload.length < 4) {
          console.warn(`[EegDataHandler] EEG payload too small for header: ${payload.length} bytes`);
          return;
        }

        const totalSamples = dataView.getUint32(offset, true);
        offset += 4;

        const timestampBytes = totalSamples * 8;
        const sampleBytes = totalSamples * 4;

        if (payload.length < offset + timestampBytes + sampleBytes) {
          console.warn(`[EegDataHandler] Incomplete EEG payload. Expected ${offset + timestampBytes + sampleBytes}, got ${payload.length}`);
          return;
        }

        const timestamps = new BigUint64Array(totalSamples);
        for (let i = 0; i < totalSamples; i++) {
          timestamps[i] = dataView.getBigUint64(offset, true);
          offset += 8;
        }

        const samples = new Float32Array(totalSamples);
        for (let i = 0; i < totalSamples; i++) {
          samples[i] = dataView.getFloat32(offset, true);
          offset += 4;
        }

        const batchSize = totalSamples / configuredChannelCount;
        if (batchSize === 0 || !Number.isInteger(batchSize)) {
          console.warn(`[EegDataHandler] Invalid batch size for EEG data: ${batchSize}`);
          return;
        }

        const allChannelSamples: { values: Float32Array; timestamps: BigUint64Array }[] = [];
        for (let ch = 0; ch < configuredChannelCount; ch++) {
          const channelValues = new Float32Array(batchSize);
          const channelTimestamps = new BigUint64Array(batchSize);
          for (let i = 0; i < batchSize; i++) {
            const sampleIndex = i * configuredChannelCount + ch;
            channelValues[i] = samples[sampleIndex];
            channelTimestamps[i] = timestamps[sampleIndex];
          }
          allChannelSamples.push({ values: channelValues, timestamps: channelTimestamps });
        }

        if (allChannelSamples.length > 0) {
          onSamplesRef.current?.(allChannelSamples);
        }

        if (latestTimestampRef) latestTimestampRef.current = performance.now();
        if (debugInfoRef) {
          debugInfoRef.current.packetsReceived++;
          debugInfoRef.current.lastPacketTime = performance.now();
          debugInfoRef.current.samplesProcessed += batchSize * configuredChannelCount;
        }
      };

      const handleFftPayload = (payload: Uint8Array) => {
        try {
          const text = new TextDecoder().decode(payload);
          const fftData = JSON.parse(text);
          onFftDataRef.current?.(fftData);
        } catch (error) {
          console.error("[EegDataHandler] Error parsing FFT payload:", error);
        }
      };

      const handleLogPayload = (payload: Uint8Array) => {
        try {
          const text = new TextDecoder().decode(payload);
          console.log(`[EEG-DEVICE-LOG] ${text}`);
        } catch (error) {
          console.error("[EegDataHandler] Error parsing log payload:", error);
        }
      };

      ws.onclose = (event) => {
        if (!isMounted) return;
        
        // Log all unexpected closures, but handle them differently
        const isExpectedClosure = event.code === 1000 || event.code === 1005;
        const isUnexpectedClosure = event.code === 1006 || event.code === 1001;
        
        if (!isExpectedClosure) {
          console.log(`[EegDataHandler] WebSocket closed with code: ${event.code}, reason: ${event.reason || 'No reason provided'}`);
        }
        
        setWsConnectionStatus('Disconnected');
        
        // Don't reconnect for expected closures (normal shutdown)
        if (isExpectedClosure) {
          console.log('[EegDataHandler] WebSocket closed normally, not reconnecting');
          return;
        }
        
        // For unexpected closures (like 1006), implement smarter reconnection
        const maxReconnectDelay = 5000;
        const baseDelay = isUnexpectedClosure ? 1000 : 500; // Longer delay for unexpected closures
        const reconnectDelay = Math.min(
          maxReconnectDelay,
          baseDelay * Math.pow(1.5, reconnectAttemptsRef.current)
        );
        
        reconnectAttemptsRef.current++;
        
        // Limit reconnection attempts for persistent issues
        if (reconnectAttemptsRef.current > 10) {
          console.error('[EegDataHandler] Too many reconnection attempts, stopping');
          onErrorRef.current?.('WebSocket connection failed after multiple attempts');
          return;
        }
        
        // Only log if we are actually going to try reconnecting
        if (isMounted) {
          console.log(`[EegDataHandler] Attempting to reconnect in ${reconnectDelay}ms (attempt ${reconnectAttemptsRef.current})`);
          reconnectTimeoutRef.current = setTimeout(connectWebSocket, reconnectDelay);
        }
      };
      
      ws.onerror = (error) => {
        if (!isMounted) return;
        console.error('WebSocket error:', error);
        onErrorRef.current?.(`WebSocket error: ${error}`);
      };
    };

    connectWebSocket();

    return () => {
      isMounted = false;
      console.log(`[EegDataHandler] Cleaning up WebSocket effect.`);

      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      if (dataReceivedTimeoutRef.current) {
        clearTimeout(dataReceivedTimeoutRef.current);
      }
      if (wsRef.current) {
        wsRef.current.onclose = null;
        wsRef.current.onerror = null;
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [pipelineState]); // Re-run effect only when the pipelineState object changes

  // Return status and debug info
  return {
    status: wsConnectionStatus,
    debugInfo: !isProduction ? debugInfo : undefined
  };
}