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
import { calculateFft } from '../utils/fftUtils';
import {
    DEFAULT_SAMPLE_RATE,
    DEFAULT_BATCH_SIZE,
    WINDOW_DURATION,
} from '../utils/eegConstants';
 
interface EegDataHandlerProps {
  config: any;
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
  onFftData?: (channelIndex: number, fftOutput: number[]) => void; // New callback for FFT data
  subscriptions: string[]; // <-- New prop for subscriptions
}

export function useEegDataHandler({
  config,
  onDataUpdate,
  onError,
  onSamples,
  lastDataChunkTimeRef,
  latestTimestampRef,
  debugInfoRef,
  onFftData,
  subscriptions, // <-- New prop
}: EegDataHandlerProps) {
  const [status, setStatus] = useState('Connecting...');
  const wsRef = useRef<WebSocket | null>(null);
  // handleMessageRef is no longer needed at this scope
  const dataReceivedTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectAttemptsRef = useRef<number>(0);
  const isProduction = process.env.NODE_ENV === 'production';
  const logCounterRef = useRef(0); // Ref for throttling logs

  // --- Refs for props to ensure stability ---
  const configRef = useRef(config);
  const onDataUpdateRef = useRef(onDataUpdate);
  const onErrorRef = useRef(onError);
  const onFftDataRef = useRef(onFftData);
  const onSamplesRef = useRef(onSamples);
  const subscriptionsRef = useRef(subscriptions);

  // Update refs whenever props change
  useEffect(() => {
    configRef.current = config;
  }, [config]);

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

  useEffect(() => {
    // This effect handles sending subscription messages when the list changes.
    const oldSubscriptions = new Set(subscriptionsRef.current);
    const newSubscriptions = new Set(subscriptions);
    const toSubscribe = [...newSubscriptions].filter(x => !oldSubscriptions.has(x));
    const toUnsubscribe = [...oldSubscriptions].filter(x => !newSubscriptions.has(x));

    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) {
      if (toSubscribe.length > 0) {
        console.log('[EegDataHandler] Subscribing to:', toSubscribe);
        ws.send(JSON.stringify({ action: 'subscribe', topics: toSubscribe }));
      }
      if (toUnsubscribe.length > 0) {
        console.log('[EegDataHandler] Unsubscribing from:', toUnsubscribe);
        ws.send(JSON.stringify({ action: 'unsubscribe', topics: toUnsubscribe }));
      }
    }

    subscriptionsRef.current = subscriptions;
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
  // Create a stable key from the config properties that necessitate a WebSocket restart.
  const configKey = useMemo(() => {
    if (!config) return null;
    // Sort channels to ensure ["0", "1"] and ["1", "0"] produce the same key
    const channelKey = config.channels?.slice().sort().join(',') || '';
    return `${config.sample_rate}-${channelKey}`;
  }, [config]);

  useEffect(() => {
    if (!configKey) {
      console.log("[EegDataHandler] Waiting for config to be ready...");
      return; // Don't connect until config is available
    }
    console.log(`[EegDataHandler] Effect running for configKey: ${configKey}`);
    let isMounted = true;

    const connectWebSocket = () => {
      const currentConfig = configRef.current;
      if (!currentConfig) {
        console.warn("[EegDataHandler] connectWebSocket called without config.");
        return; // Should not happen if configKey is set, but as a safeguard.
      }

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
      
      if (!isMounted) return;
      setStatus('Connecting...');
  
      const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
      const wsProtocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'wss' : 'ws';
      const ws = new WebSocket(`${wsProtocol}://${wsHost}:8080/ws/data`); // <-- Use the new endpoint
      wsRef.current = ws;
      
      ws.binaryType = 'arraybuffer';
      
      ws.onopen = () => {
        if (!isMounted) return;
        setStatus('Connected');
        reconnectAttemptsRef.current = 0;
        const now = performance.now();
        setDebugInfo(prev => ({
          ...prev,
          connectionAttempts: prev.connectionAttempts + 1,
          lastConnectionTime: now,
        }));
        console.log(`[EegDataHandler] WebSocket connection established.`);

        // Send initial subscription message
        const currentSubscriptions = subscriptionsRef.current;
        if (ws.readyState === WebSocket.OPEN && currentSubscriptions.length > 0) {
          console.log('[EegDataHandler] Sending initial subscriptions:', currentSubscriptions);
          ws.send(JSON.stringify({ action: 'subscribe', topics: currentSubscriptions }));
        }
      };
      
      ws.onmessage = (event: MessageEvent) => {
        if (!isMounted) return;
        
        try {
          const now = performance.now();
          // --- This is the fix ---
          // Acknowledge that data is flowing on ANY message, not just binary.
          // This prevents the "no data" status if the first message is text (e.g., subscription confirmation).
          onDataUpdateRef.current?.(true);
          if (dataReceivedTimeoutRef.current) {
            clearTimeout(dataReceivedTimeoutRef.current);
          }
          dataReceivedTimeoutRef.current = setTimeout(() => {
            onDataUpdateRef.current?.(false);
          }, 1000);
          // --- End of fix ---

          setDebugInfo(prev => ({
            ...prev,
            messagesReceived: prev.messagesReceived + 1,
            lastMessageTime: now,
            lastMessageType: event.data instanceof ArrayBuffer ? 'binary' : typeof event.data,
          }));

          // Handle both binary (EEG) and text (FFT) messages
          if (event.data instanceof ArrayBuffer) {
            // Existing binary processing logic...
            setDebugInfo(prev => ({ ...prev, binaryPacketsReceived: prev.binaryPacketsReceived + 1 }));

            const buffer = new Uint8Array(event.data);
            if (buffer.length === 0) {
              console.warn("[EegDataHandler] Received empty data packet");
              return;
            }

          // Use latest config from ref
          const currentConfig = configRef.current;
          const configuredChannelCount = currentConfig?.channels?.length || 0;
          
          if (configuredChannelCount === 0) {
            console.warn("[EegDataHandler] No channels configured, skipping packet");
            return;
          }

          const dataView = new DataView(event.data);
          let offset = 0;

          if (buffer.length < 4) {
            console.warn(`[EegDataHandler] Data packet too small for header: ${buffer.length} bytes`);
            return;
          }

          const totalSamples = dataView.getUint32(offset, true);
          offset += 4;

          const timestampBytes = totalSamples * 8;
          const sampleBytes = totalSamples * 4;

          if (buffer.length < offset + timestampBytes + sampleBytes) {
            console.warn(`[EegDataHandler] Incomplete data packet. Expected ${offset + timestampBytes + sampleBytes}, got ${buffer.length}`);
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
            console.warn(`[EegDataHandler] Invalid batch size: ${batchSize}`);
            return;
          }

          // If channel count changes, create a new array of the correct size
          if (sampleBuffersRef.current.length !== configuredChannelCount) {
            sampleBuffersRef.current = new Array(configuredChannelCount).fill(null);
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

            // If there's a subscription for FFT data, calculate and send it
            if (subscriptionsRef.current.includes('FftPacket') && onFftDataRef.current) {
              allChannelSamples.forEach((channelData, index) => {
                const fftOutput = calculateFft(channelData.values);
                onFftDataRef.current?.(index, fftOutput);
              });
            }

            // Log a sample of the data every 100 packets
            logCounterRef.current++;
            if (logCounterRef.current % 100 === 0) {
              const sampleToShow = allChannelSamples[0]?.values.slice(0, 5);
              console.log(`[EegDataHandler] Ch 0 Data Sample (packet #${debugInfoRef.current.packetsReceived}):`, sampleToShow);
            }
          }

          if (latestTimestampRef) {
            latestTimestampRef.current = performance.now();
          }

          if (debugInfoRef) {
            debugInfoRef.current.packetsReceived++;
            debugInfoRef.current.lastPacketTime = performance.now();
            debugInfoRef.current.samplesProcessed += batchSize * configuredChannelCount;
          }

          } else if (typeof event.data === 'string') {
            // New text message processing for FFT data
            setDebugInfo(prev => ({ ...prev, textPacketsReceived: prev.textPacketsReceived + 1 }));
            try {
              const message = JSON.parse(event.data);
              if (message.type === 'status' && message.status === 'subscription_ok') {
                console.log('[EegDataHandler] Subscription confirmed by backend.');
                // Can set a specific state here if needed, e.g., setSubscriptionActive(true)
              } else if (message.type === 'FftPacket' && onFftDataRef.current) {
                // The backend now sends FFT data per-channel in a structured way.
                // We assume the `data` field is an array of FFT results, one for each channel.
                if (Array.isArray(message.data)) {
                  message.data.forEach((channelFft: any, index: number) => {
                    // Assuming channelFft has a 'power' property which is the array of numbers.
                    if (channelFft && Array.isArray(channelFft.power)) {
                       onFftDataRef.current?.(index, channelFft.power);
                    }
                  });
                }
              } else if (message.type === 'error') {
                  console.error(`[EegDataHandler] Received error from backend: ${message.message}`);
                  onErrorRef.current?.(message.message);
              }
            } catch (error) {
              console.error("[EegDataHandler] Error parsing text message:", error);
            }
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
      
      ws.onclose = (event) => {
        if (!isMounted) return;
        
        // Log all unexpected closures, but handle them differently
        const isExpectedClosure = event.code === 1000 || event.code === 1005;
        const isUnexpectedClosure = event.code === 1006 || event.code === 1001;
        
        if (!isExpectedClosure) {
          console.log(`[EegDataHandler] WebSocket closed with code: ${event.code}, reason: ${event.reason || 'No reason provided'}`);
        }
        
        setStatus('Disconnected');
        
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
      console.log(`[EegDataHandler] Cleaning up effect for configKey: ${configKey}`);

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
  }, [configKey]); // Re-run effect only when the stable key changes
  // Return status and debug info
  return {
    status,
    debugInfo: !isProduction ? debugInfo : undefined
  };
}