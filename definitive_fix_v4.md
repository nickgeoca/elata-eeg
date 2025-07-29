# Definitive Fix v4: State Flow Refactoring

This plan resolves the persistent race condition by simplifying the state flow and consolidating all connection logic into the `EegDataHandler`. It contains the complete, final source code for all affected files.

---

### 1. Final Code for `kiosk/src/context/PipelineContext.tsx`

The context will now expose the entire `pipelineState` object to its consumers, ensuring atomic state propagation.

```tsx
'use client';

import React, { createContext, useContext, useState, ReactNode, useEffect, useCallback } from 'react';
import { getPipelines, startPipeline as apiStartPipeline, getPipelineState } from '../utils/api';
import { SystemConfig } from '@/types/eeg';

// Define the shape of a single pipeline
interface Pipeline {
  id: string;
  name: string;
}

// New interface for the combined pipeline state
export interface PipelineState {
  status: 'stopped' | 'starting' | 'started' | 'error';
  config: SystemConfig | null;
}

// Define the shape of the context data
interface PipelineContextType {
  pipelines: Pipeline[];
  selectedPipeline: Pipeline | null;
  pipelineConfig: SystemConfig | null;
  pipelineStatus: 'stopped' | 'starting' | 'started' | 'error';
  selectAndStartPipeline: (id: string) => Promise<void>;
  pipelineState: PipelineState; // Expose the whole state object
}

// Create the context with a default value
const PipelineContext = createContext<PipelineContextType | undefined>(undefined);

// Define the props for the provider component
interface PipelineProviderProps {
  children: ReactNode;
}

export const PipelineProvider = ({ children }: PipelineProviderProps) => {
  const [pipelines, setPipelines] = useState<Pipeline[]>([]);
  const [selectedPipeline, setSelectedPipeline] = useState<Pipeline | null>(null);
  
  // Combine pipelineConfig and pipelineStatus into a single state object
  const [pipelineState, setPipelineState] = useState<PipelineState>({
    status: 'stopped',
    config: null,
  });

  // Fetch available pipelines on mount
  useEffect(() => {
    const fetchPipelines = async () => {
      try {
        const availablePipelines = await getPipelines();
        setPipelines(availablePipelines);
      } catch (error) {
        console.error("Failed to fetch pipelines on mount:", error);
        setPipelineState({ status: 'error', config: null });
      }
    };
    fetchPipelines();
  }, []);

  const selectAndStartPipeline = useCallback(async (id: string) => {
    const pipelineToStart = pipelines.find(p => p.id === id);
    if (!pipelineToStart) {
      console.error(`Pipeline with id ${id} not found.`);
      setPipelineState({ status: 'error', config: null });
      return;
    }

    setPipelineState(prevState => ({ ...prevState, status: 'starting' }));
    setSelectedPipeline(pipelineToStart);

    try {
      await apiStartPipeline(id);
      console.log(`Pipeline ${id} start command issued successfully.`);

      // After starting, fetch the full state
      const state = await getPipelineState();
      
      // Atomic update of both status and config
      setPipelineState({ status: 'started', config: state });
      
      console.log(`Pipeline ${id} is running and state has been fetched.`);
    } catch (error) {
      console.error(`Failed to start pipeline ${id}:`, error);
      setPipelineState({ status: 'error', config: null });
    }
  }, [pipelines]);

  const value = {
    pipelines,
    selectedPipeline,
    pipelineConfig: pipelineState.config,
    pipelineStatus: pipelineState.status,
    selectAndStartPipeline,
    pipelineState: pipelineState,
  };

  return (
    <PipelineContext.Provider value={value}>
      {children}
    </PipelineContext.Provider>
  );
};

// Custom hook to use the pipeline context
export const usePipeline = () => {
  const context = useContext(PipelineContext);
  if (context === undefined) {
    throw new Error('usePipeline must be used within a PipelineProvider');
  }
  return context;
};
```

---

### 2. Final Code for `kiosk/src/context/EegDataContext.tsx`

This context becomes a simple passthrough, forwarding the `pipelineState` object directly to the `useEegDataHandler` hook.

```tsx
'use client';

import React, { createContext, useContext, useState, ReactNode, useMemo, useRef, useCallback, useEffect } from 'react';
import { useEegDataHandler } from '../components/EegDataHandler';
import { useEventStream } from './EventStreamContext';
import { usePipeline } from './PipelineContext'; // Import the usePipeline hook
import { EegSample, SampleChunk } from '../types/eeg'; // Import shared types

// Constants for data management
const MAX_SAMPLE_CHUNKS = 100;
const RECONNECTION_DATA_RETENTION_MS = 5000; // Keep data for 5 seconds during reconnections

// Callback type for live data subscribers
type RawDataCallback = (data: SampleChunk[]) => void;

// Type for the full FFT data packet, matching the backend and FftDisplay component
export interface FftPacket {
  psd_packets: { channel: number; psd: number[] }[];
  fft_config: {
    fft_size: number;
    sample_rate: number;
    window_function: string;
  };
  timestamp: number;
  source_frame_id: number;
}

 // Define the shape of the context data
interface EegDataContextType {
  dataVersion: number; // Increments on new data
  getRawSamples: () => SampleChunk[]; // Function to get the current samples
  subscribeRaw: (callback: RawDataCallback) => () => void; // Returns an unsubscribe function
  fftData: Record<number, number[]>; // Latest FFT data per channel
  fullFftPacket: FftPacket | null; // The complete, most recent FFT packet
  config: any;
  dataStatus: {
    dataReceived: boolean;
    driverError: string | null;
    wsStatus: string;
    isReconnecting: boolean;
  };
  // Add methods for data management
  clearOldData: () => void;
  // Subscription management
  subscribe: (topics: string[]) => void;
  unsubscribe: (topics: string[]) => void;
  setConfig: (config: any) => void;
}

// Create the context with a default value
const EegDataContext = createContext<EegDataContextType | undefined>(undefined);

// Define the props for the provider component
interface EegDataProviderProps {
  children: ReactNode;
}

export const EegDataProvider = ({ children }: EegDataProviderProps) => {
  const rawSamplesRef = useRef<SampleChunk[]>([]);
  const [dataVersion, setDataVersion] = useState(0);
  const [fftData, setFftData] = useState<Record<number, number[]>>({});
  const [fullFftPacket, setFullFftPacket] = useState<FftPacket | null>(null);
  const [dataReceived, setDataReceived] = useState(false);
  const [driverError, setDriverError] = useState<string | null>(null);
  const [isReconnecting, setIsReconnecting] = useState(false);
  const [subscriptions, setSubscriptions] = useState<string[]>([]);
  const rawDataSubscribersRef = useRef<Set<RawDataCallback>>(new Set());

  const { pipelineConfig, pipelineState } = usePipeline(); // Get the pipeline state object

  // Derive a stable config object directly from the pipeline context
  const config = useMemo(() => {
    if (!pipelineConfig) {
      return null;
    }
    const eegSourceStage = pipelineConfig.stages.find(s => s.plugin_id === 'eeg_source');
    const channels = eegSourceStage ? Array.from({ length: eegSourceStage.params.channel_count || 0 }, (_, i) => i) : [];

    return {
      ...pipelineConfig,
      channels,
      sample_rate: eegSourceStage?.params.sample_rate || 250,
    };
  }, [pipelineConfig]);
 
   useEffect(() => {
     // Automatically subscribe to the raw EEG data topic when the provider mounts.
     // This is the primary data stream for the main graphs.
     subscribe(['FilteredEeg']);
 
     // No cleanup needed, subscriptions are managed by the component lifecycle.
   }, []); // Empty dependency array ensures this runs only once on mount.
  
  // Use refs to track data timestamps for cleanup
  const sampleTimestamps = useRef<number[]>([]);
  const lastCleanupTime = useRef<number>(Date.now());
  
  // Create stable refs for EegDataHandler to prevent unnecessary reconnections
  const lastDataChunkTimeRef = useRef<number[]>([]);
  const latestTimestampRef = useRef<number>(0);
  const debugInfoRef = useRef({ lastPacketTime: 0, packetsReceived: 0, samplesProcessed: 0 });

  const handleSamples = useCallback((channelSamples: { values: Float32Array; timestamps: BigUint64Array }[]) => {
    const now = Date.now();
    const currentChannelCount = config?.channels?.length || 1;
    const currentSampleRate = config?.sample_rate || 250;

    // Create one SampleChunk with all channel data combined, preserving temporal order
    const allSamples: EegSample[] = [];
    const batchSize = channelSamples[0]?.values.length || 0;
    
    // Reconstruct the original interleaved temporal order
    for (let timeIndex = 0; timeIndex < batchSize; timeIndex++) {
      for (let channelIndex = 0; channelIndex < channelSamples.length; channelIndex++) {
        const channelData = channelSamples[channelIndex];
        if (channelData && timeIndex < channelData.values.length) {
          allSamples.push({
            value: channelData.values[timeIndex],
            timestamp: channelData.timestamps[timeIndex],
            channelIndex: channelIndex,
          });
        }
      }
    }

    const newSampleChunk: SampleChunk = {
      config: {
        channelCount: currentChannelCount,
        sampleRate: currentSampleRate,
      },
      samples: allSamples,
    };

    const newSampleChunks: SampleChunk[] = [newSampleChunk];

    // Debug logging to verify channel assignment
    if (debugInfoRef.current.packetsReceived % 50 === 0) {
      const channelCounts = new Map<number, number>();
      allSamples.forEach(sample => {
        channelCounts.set(sample.channelIndex, (channelCounts.get(sample.channelIndex) || 0) + 1);
      });
      console.log(`[EegDataContext] Packet #${debugInfoRef.current.packetsReceived}: Channel distribution:`,
        Array.from(channelCounts.entries()).map(([ch, count]) => `Ch${ch}:${count}`).join(', '));
    }

    const newSamples = [...rawSamplesRef.current, ...newSampleChunks];
    sampleTimestamps.current.push(...Array(newSampleChunks.length).fill(now));

    // Enforce a hard limit on the number of chunks to prevent memory leaks
    if (newSamples.length > MAX_SAMPLE_CHUNKS) {
      const excess = newSamples.length - MAX_SAMPLE_CHUNKS;
      sampleTimestamps.current.splice(0, excess);
      rawSamplesRef.current = newSamples.slice(excess);
    } else {
      rawSamplesRef.current = newSamples;
    }
    
    setDataVersion(v => v + 1);
    
    // Publish the new data to all subscribers
    rawDataSubscribersRef.current.forEach(callback => callback(newSampleChunks));
    
    // Periodic cleanup of old data (every 10 seconds)
    if (now - lastCleanupTime.current > 10000) {
      cleanupOldData();
      lastCleanupTime.current = now;
    }
  }, [config]);

  const handleFftData = useCallback((data: FftPacket) => {
    setFullFftPacket(data); // Store the full packet

    // Also update the simplified fftData for compatibility if needed elsewhere
    if (data && Array.isArray(data.psd_packets)) {
      const newFftData: Record<number, number[]> = {};
      for (const packet of data.psd_packets) {
        newFftData[packet.channel] = packet.psd;
      }
      setFftData(prevFftData => ({
        ...prevFftData,
        ...newFftData,
      }));
    }
  }, []);

  const cleanupOldData = useCallback(() => {
    const now = Date.now();
    const cutoffTime = now - RECONNECTION_DATA_RETENTION_MS;
    
    // Find the first index to keep
    const firstValidIndex = sampleTimestamps.current.findIndex(timestamp => timestamp > cutoffTime);
    
    if (firstValidIndex > 0) {
      // Remove old timestamps and samples
      sampleTimestamps.current.splice(0, firstValidIndex);
      rawSamplesRef.current = rawSamplesRef.current.slice(firstValidIndex);
      setDataVersion(v => v + 1); // Notify consumers of the change
    }
  }, []);

  const clearOldData = useCallback(() => {
    rawSamplesRef.current = [];
    sampleTimestamps.current = [];
    setDataVersion(v => v + 1);
    console.log('[EegDataContext] Cleared old data due to manual request');
  }, []);

  const getRawSamples = useCallback(() => {
    return rawSamplesRef.current;
  }, []);

 const subscribeRaw = useCallback((callback: RawDataCallback) => {
   rawDataSubscribersRef.current.add(callback);
   // Return an unsubscribe function
   return () => {
     rawDataSubscribersRef.current.delete(callback);
   };
 }, []);

  const subscribe = useCallback((topics: string[]) => {
    setSubscriptions(prev => {
      const newSubs = [...new Set([...prev, ...topics])];
      // The subscription message will be sent by EegDataHandler when subscriptions change
      console.log('[EegDataContext] Subscribing to topics:', topics);
      return newSubs;
    });
  }, []);

  const unsubscribe = useCallback((topics: string[]) => {
    setSubscriptions(prev => {
      const newSubs = prev.filter(t => !topics.includes(t));
      // The unsubscription message will be sent by EegDataHandler when subscriptions change
      console.log('[EegDataContext] Unsubscribing from topics:', topics);
      return newSubs;
    });
  }, []);

  // Clear buffer when configuration changes to prevent misalignment
  // Create a stable key for the configuration to prevent unnecessary effect runs
  const configKey = useMemo(() => {
    if (!config) return null;
    // Sort channels to ensure key is consistent regardless of order
    const sortedChannels = config.channels.slice().sort((a: number, b: number) => a - b).join(',');
    return `${config.sample_rate}-${sortedChannels}`;
  }, [config]);

  // Clear buffer when the stable configuration key changes
  useEffect(() => {
    // Don't clear the buffer on the initial load when configKey is null
    if (configKey === null) return;

    rawSamplesRef.current = [];
    sampleTimestamps.current = [];
    console.log('[EegDataContext] Cleared buffer due to configuration change');
  }, [configKey]);

  // Handle WebSocket status changes to detect reconnections
  const handleDataUpdate = useCallback((received: boolean) => {
    setDataReceived(received);
    if (received && isReconnecting) {
      setIsReconnecting(false);
      console.log('[EegDataContext] Reconnection successful, data flow restored');
    }
  }, [isReconnecting]);

  const handleError = useCallback((error: string | null) => {
    setDriverError(error);
    if (error && !isReconnecting) {
      setIsReconnecting(true);
      console.log('[EegDataContext] Connection error detected, entering reconnection mode');
    }
  }, [isReconnecting]);

  const { status: wsStatus } = useEegDataHandler({
    pipelineState: pipelineState, // Pass the entire state object
    onDataUpdate: handleDataUpdate,
    onError: handleError,
    onSamples: handleSamples,
    onFftData: handleFftData,
    subscriptions, // Pass subscriptions to the handler
    // Use stable refs to prevent unnecessary WebSocket reconnections
    lastDataChunkTimeRef,
    latestTimestampRef,
    debugInfoRef,
  });

  const value = useMemo(() => ({
    dataVersion,
    getRawSamples,
    subscribeRaw,
    fftData,
    fullFftPacket,
    config,
    dataStatus: {
      dataReceived,
      driverError,
      wsStatus,
      isReconnecting,
    },
    clearOldData,
    subscribe,
    unsubscribe,
    setConfig: () => {}, // No-op, since config is now derived
  }), [dataVersion, fftData, fullFftPacket, config, dataReceived, driverError, wsStatus, isReconnecting, getRawSamples, subscribeRaw, clearOldData, subscribe, unsubscribe]);

  return (
    <EegDataContext.Provider value={value}>
      {children}
    </EegDataContext.Provider>
  );
};

// Custom hook to use the EEG data context
export const useEegData = () => {
  const context = useContext(EegDataContext);
  if (context === undefined) {
    throw new Error('useEegData must be used within an EegDataProvider');
  }
  return context;
};
```

---

### 3. Final Code for `kiosk/src/components/EegDataHandler.tsx`

This hook now contains all connection logic, driven by the single `pipelineState` object. This consolidation is the key to the fix.

```tsx
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
  
      const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
      const wsProtocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'wss' : 'ws';
      const port = '9001'; // Always use port 9001 for data service
      const ws = new WebSocket(`${wsProtocol}://${wsHost}:${port}/ws/data`);
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
        const configuredChannelCount = config?.channels?.length || 0;
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