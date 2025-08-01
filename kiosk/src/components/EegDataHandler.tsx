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
  enabled: boolean; // New prop to control the connection
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
}

export function useEegDataHandler({
  enabled,
  pipelineState,
  onDataUpdate,
  onError,
  onSamples,
  lastDataChunkTimeRef,
  latestTimestampRef,
  debugInfoRef,
  onFftData,
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
  
  const wsUrl = useMemo(() => {
    if (!enabled) {
        return null;
    }
    const wsProtocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'wss' : 'ws';
    const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
    // The port is now static per the new architecture.
    const wsPort = 9001;
    return `${wsProtocol}://${wsHost}:${wsPort}/ws/data`;
  }, [enabled]);

  /**
   * Main effect for WebSocket connection management.
   * Runs only when the wsUrl changes.
   */
  useEffect(() => {
    // Only run the effect if the handler is enabled and the URL is ready
    if (!enabled || !wsUrl) {
      if (wsRef.current) {
        console.log("[EegDataHandler] Ensuring WebSocket is closed because handler is disabled or URL is null.");
        wsRef.current.close(1000, "Handler disabled or configuration changed");
        wsRef.current = null;
      }
      return;
    }

    let isMounted = true;

    const connectWebSocket = () => {
      if (!isMounted) return;
      setWsConnectionStatus('Connecting...');

      console.log(`[EegDataHandler] Connecting to WebSocket at: ${wsUrl}`);
      const ws = new WebSocket(wsUrl);
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

        // Subscribe to the EEG data topic
        const subscriptionMessage = {
            subscribe: "eeg_voltage"
        };
        ws.send(JSON.stringify(subscriptionMessage));
        console.log('[EegDataHandler] Sent subscription request for "eeg_voltage"');
      };
      
      ws.onmessage = (event: MessageEvent) => {
        if (!isMounted) return;

        try {
            onDataUpdateRef.current?.(true);
            if (dataReceivedTimeoutRef.current) clearTimeout(dataReceivedTimeoutRef.current);
            dataReceivedTimeoutRef.current = setTimeout(() => onDataUpdateRef.current?.(false), 1000);

            const data = JSON.parse(event.data);

            // Check if it's an RtPacket and has the expected data
            if (data.topic === 'eeg_voltage' && data.payload.data) {
                // The actual data is in `data.payload.data`
                // This will likely be an array of numbers (samples)
                // The structure of the payload needs to be handled.
                // For now, let's assume it's an array of samples for each channel.
                // The `handleSamples` function expects a specific format.
                // We need to adapt the incoming data to what `handleSamples` expects.
                
                // This part requires knowing the exact structure of `data.payload.data`
                // For now, we will log it.
                console.log("Received RtPacket:", data.payload);

                // TODO: Adapt `data.payload` to the format expected by `onSamplesRef.current`.
                // The `onSamples` prop expects an array of objects, where each object has `values` and `timestamps`.
                // The new payload structure needs to be mapped to this.
            } else if (data.topic === 'fft') {
                onFftDataRef.current?.(data.payload);
            }

        } catch (error) {
            console.error("[EegDataHandler] Error in onmessage handler:", error);
            onErrorRef.current?.(`Error processing data: ${error}`);
        }
      };

      ws.onclose = (event) => {
        if (!isMounted) return;

        setWsConnectionStatus('Disconnected');
        console.log(`[EegDataHandler] WebSocket closed with code: ${event.code}, reason: ${event.reason || 'No reason provided'}`);

        // Do not reconnect if the closure was clean (1000) or initiated by the component unmounting (1005)
        if (event.code === 1000 || event.code === 1005) {
            console.log('[EegDataHandler] WebSocket closed normally, not reconnecting.');
            return;
        }

        // Exponential backoff for reconnection
        const maxReconnectDelay = 30000; // 30 seconds
        const baseDelay = 1000; // 1 second
        const reconnectDelay = Math.min(maxReconnectDelay, baseDelay * Math.pow(2, reconnectAttemptsRef.current));
        
        reconnectAttemptsRef.current++;

        console.log(`[EegDataHandler] Attempting to reconnect in ${reconnectDelay}ms (attempt ${reconnectAttemptsRef.current})`);
        reconnectTimeoutRef.current = setTimeout(connectWebSocket, reconnectDelay);
      };
      
      ws.onerror = (error) => {
        if (!isMounted) return;
        console.error('[EegDataHandler] WebSocket error:', error);
        setDebugInfo(prev => ({ ...prev, lastError: 'WebSocket error event' }));
        // The onclose event will handle reconnection logic, so we just log the error here.
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
  }, [enabled, wsUrl]); // Re-run effect when enabled status or URL changes

  // Return status and debug info
  return {
    status: wsConnectionStatus,
    debugInfo: !isProduction ? debugInfo : undefined
  };
}
