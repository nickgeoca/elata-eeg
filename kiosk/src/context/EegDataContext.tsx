'use client';

import React, { createContext, useContext, useState, ReactNode, useMemo, useRef, useCallback, useEffect } from 'react';
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
// --- Start of new context structure ---

// 1. Stable Context: For functions and stable configuration
interface EegDataStableContextType {
  subscribeRaw: (callback: RawDataCallback) => () => void;
  getRawSamples: () => SampleChunk[];
  clearOldData: () => void;
  config: any; // Config is considered stable; changes should be infrequent
}

// 2. Dynamic Context: For frequently updated data
interface EegDataDynamicContextType {
  dataVersion: number;
  fftData: Record<number, number[]>;
  fullFftPacket: FftPacket | null;
}

// 3. Status Context: For connection and data flow status
interface EegDataStatusContextType {
  dataStatus: {
    dataReceived: boolean;
    driverError: string | null;
    wsStatus: string;
    isReconnecting: boolean;
  };
  isReady: boolean;
}

const EegDataStableContext = createContext<EegDataStableContextType | undefined>(undefined);
const EegDataDynamicContext = createContext<EegDataDynamicContextType | undefined>(undefined);
const EegDataStatusContext = createContext<EegDataStatusContextType | undefined>(undefined);

// --- End of new context structure ---

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
  const [isReady, setIsReady] = useState(false); // State to track final configuration readiness
  const rawDataSubscribersRef = useRef({ raw: {} as Record<string, RawDataCallback> });

  const { pipelineConfig, pipelineStatus } = usePipeline(); // Get the pipeline state object
  const { subscribe } = useEventStream();
  const [sourceReadyMeta, setSourceReadyMeta] = useState<any | null>(null);

  useEffect(() => {
    const unsubscribe = subscribe('SourceReady', (data: any) => {
      // The actual metadata is nested inside the 'meta' property of the event data
      if (data.meta) {
        console.log('[EegDataContext] Received SourceReady event with meta:', data.meta);
        setSourceReadyMeta(data.meta);
      }
    });

    return () => {
      unsubscribe();
    };
  }, [subscribe]);

  const config = useMemo(() => {
    if (sourceReadyMeta) {
      const newChannelCount = sourceReadyMeta.channel_names?.length || 0;
      return {
        ...pipelineConfig,
        channels: Array.from({ length: newChannelCount }, (_, i) => i),
        sample_rate: sourceReadyMeta.sample_rate || 250,
      };
    }
    
    if (!pipelineConfig) {
      return null;
    }

    const eegSourceStage = pipelineConfig.stages.find(s => s.type === 'eeg_source');
    const channels = eegSourceStage ? Array.from({ length: eegSourceStage.params.channel_count || 0 }, (_, i) => i) : [];

    return {
      ...pipelineConfig,
      channels,
      sample_rate: eegSourceStage?.params.sample_rate || 250,
    };
  }, [pipelineConfig ? JSON.stringify(pipelineConfig) : null, sourceReadyMeta]);
 
  
  // Use refs to track data timestamps for cleanup
  const sampleTimestamps = useRef<number[]>([]);
  const lastCleanupTime = useRef<number>(Date.now());
  
  // Create stable refs for EegDataHandler to prevent unnecessary reconnections
  const lastDataChunkTimeRef = useRef<number[]>([]);
  const latestTimestampRef = useRef<number>(0);
  const debugInfoRef = useRef({ lastPacketTime: 0, packetsReceived: 0, samplesProcessed: 0 });

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

  const handleSamples = useCallback((channelSamples: { values: Float32Array; timestamps: BigUint64Array }[]) => {
    const now = Date.now();
    const currentChannelCount = config?.channels?.length || 1;
    const currentSampleRate = config?.sample_rate || 250;

    // Create a new SampleChunk for each channel's data
    const newSampleChunks: SampleChunk[] = channelSamples.map((channelData, channelIndex) => {
      const samples: EegSample[] = [];
      for (let i = 0; i < channelData.values.length; i++) {
        samples.push({
          value: channelData.values[i],
          timestamp: channelData.timestamps[i],
          channelIndex: channelIndex,
        });
      }
      return {
        config: {
          channelCount: currentChannelCount,
          sampleRate: currentSampleRate,
        },
        samples: samples,
      };
    });

    // Debug logging to verify channel assignment
    if (debugInfoRef.current.packetsReceived % 50 === 0) {
      const channelCounts = new Map<number, number>();
      newSampleChunks.forEach(chunk => {
        chunk.samples.forEach(sample => {
            channelCounts.set(sample.channelIndex, (channelCounts.get(sample.channelIndex) || 0) + 1);
        });
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
    Object.values(rawDataSubscribersRef.current.raw).forEach(callback => callback(newSampleChunks));
    
    // Periodic cleanup of old data (every 10 seconds)
    if (now - lastCleanupTime.current > 10000) {
      cleanupOldData();
      lastCleanupTime.current = now;
    }
  }, [config, cleanupOldData]);

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

  // Create refs to hold the latest versions of the data handlers.
  // This prevents them from becoming dependencies in the main WebSocket useEffect.
  const handleSamplesRef = useRef(handleSamples);
  const handleFftDataRef = useRef(handleFftData);

  useEffect(() => {
    handleSamplesRef.current = handleSamples;
    handleFftDataRef.current = handleFftData;
  }, [handleSamples, handleFftData]);

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
    const id = Date.now().toString();
    rawDataSubscribersRef.current.raw[id] = callback;
    return () => {
      delete rawDataSubscribersRef.current.raw[id];
    };
  }, [rawDataSubscribersRef]);

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
    setDataVersion(v => v + 1); // Atomically notify consumers of the change
    console.log('[EegDataContext] Cleared buffer due to configuration change');
  }, [configKey]);

  // Effect to determine when the system is truly ready
  useEffect(() => {
    // Ready when pipeline is started and the final config with channel names is available
    if (pipelineStatus === 'started' && sourceReadyMeta?.channel_names) {
      setIsReady(true);
      console.log('[EegDataContext] System is ready. Final configuration has been received.');
    } else {
      setIsReady(false);
    }
  }, [pipelineStatus, sourceReadyMeta]);

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

  const [wsStatus, setWsStatus] = useState('Disconnected');
  const ws = useRef<WebSocket | null>(null);
  const connectionGuard = useRef(false); // Prevents race conditions

  // This useEffect manages the WebSocket connection lifecycle.
  useEffect(() => {
    // If the system isn't ready, ensure any existing connection is closed
    // and reset the connection guard to allow a new connection attempt later.
    if (!isReady) {
      if (ws.current) {
        console.log('[EegDataContext] System not ready, closing existing WebSocket.');
        ws.current.close();
        ws.current = null;
      }
      connectionGuard.current = false;
      return;
    }

    // If a connection exists or is already in progress, do nothing.
    // The connectionGuard prevents a race condition from React Strict Mode's double-render.
    if (ws.current || connectionGuard.current) {
      return;
    }
    
    connectionGuard.current = true; // Set the guard
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const host = window.location.hostname;
    const url = `${protocol}//${host}:9000/ws/data`;

    console.log('[EegDataContext] Connecting to WebSocket:', url);
    setWsStatus('Connecting...');
    const socket = new WebSocket(url);
    socket.binaryType = 'arraybuffer';
    ws.current = socket;

    socket.onopen = () => {
      console.log('[EegDataContext] WebSocket connection established');
      setWsStatus('Connected');
      socket.send(JSON.stringify({ subscribe: 'eeg_voltage' }));
    };

    // Define the message handler inside the effect to create a stable closure
    // over the handleSamples and handleFftData callbacks.
    socket.onmessage = (event: MessageEvent) => {
      if (!(event.data instanceof ArrayBuffer)) {
        console.warn('[EegDataContext] Received non-binary WebSocket message, ignoring.');
        return;
      }
      try {
        const buffer = event.data;
        const dataView = new DataView(buffer);
        let offset = 0;
        const readString = () => {
          const len = Number(dataView.getBigUint64(offset, true));
          offset += 8;
          const str = new TextDecoder().decode(buffer.slice(offset, offset + Number(len)));
          offset += Number(len);
          return str;
        };
        const topic = readString();
        if (topic === 'eeg_voltage') {
          const variant = dataView.getUint32(offset, true);
          offset += 4;
          if (variant !== 1) return;
          const source_id = readString();
          const ts_ns = dataView.getBigUint64(offset, true);
          offset += 8;
          const batch_size = dataView.getUint32(offset, true);
          offset += 4;
          const num_channels = dataView.getUint32(offset, true);
          offset += 4;
          const sensor_id = dataView.getUint32(offset, true); offset += 4;
          const meta_rev = dataView.getUint32(offset, true); offset += 4;
          const schema_ver = dataView.getUint8(offset); offset += 1;
          const source_type = readString();
          const v_ref = dataView.getFloat32(offset, true); offset += 4;
          const adc_bits = dataView.getUint8(offset); offset += 1;
          const gain = dataView.getFloat32(offset, true); offset += 4;
          const sample_rate = dataView.getUint32(offset, true); offset += 4;
          const offset_code = dataView.getInt32(offset, true); offset += 4;
          const is_twos_complement = dataView.getUint8(offset) === 1; offset += 1;
          const channel_names_len = Number(dataView.getBigUint64(offset, true)); offset += 8;
          const channel_names = [];
          for (let i = 0; i < channel_names_len; i++) {
              channel_names.push(readString());
          }
          const samples_len = Number(dataView.getBigUint64(offset, true));
          offset += 8;
          const samples = new Float32Array(buffer, offset, samples_len);
          if (num_channels === 0 || batch_size === 0) return;
          const channelSamples = Array.from({ length: num_channels }, () => ({
            values: new Float32Array(batch_size),
            timestamps: new BigUint64Array(batch_size),
          }));
          const nsPerSample = 1_000_000_000 / sample_rate;
          for (let i = 0; i < batch_size; i++) {
            const sampleTimestamp = BigInt(ts_ns) + BigInt(i * nsPerSample);
            for (let j = 0; j < num_channels; j++) {
              const sampleIndex = i * num_channels + j;
              channelSamples[j].values[i] = samples[sampleIndex];
              channelSamples[j].timestamps[i] = sampleTimestamp;
            }
          }
          handleSamplesRef.current(channelSamples);
        } else if (topic === 'fft') {
          // The FFT data is expected to be a JSON string, so we parse it.
          handleFftDataRef.current(JSON.parse(new TextDecoder().decode(event.data)));
        }
      } catch (error) {
        console.error("Failed to parse or handle binary WebSocket message:", error);
      }
    };

    socket.onerror = (err) => {
      // Only handle errors if the socket is in a connecting or open state.
      // This prevents logging errors when the connection is intentionally closed by the cleanup function.
      if (socket.readyState === WebSocket.CONNECTING || socket.readyState === WebSocket.OPEN) {
        console.error('[EegDataContext] WebSocket error:', err);
        setWsStatus('Error');
      }
    };

    socket.onclose = () => {
      console.log('[EegDataContext] WebSocket connection closed');
      // Only update state if this is the active socket that was closed.
      if (ws.current === socket) {
        setWsStatus('Disconnected');
        ws.current = null;
      }
      // Reset the guard to allow for new connection attempts.
      // NOTE: The guard is now reset only when isReady is false, to prevent race conditions.
      // connectionGuard.current = false;
    };

    // The cleanup function is critical for preventing memory leaks and race conditions.
    return () => {
      console.log('[EegDataContext] Cleanup: Closing WebSocket');
      // Remove event listeners to prevent them from being called on a stale socket instance.
      socket.onopen = null;
      socket.onmessage = null;
      socket.onerror = null;
      socket.onclose = null;
      socket.close();
    };
  }, [isReady]);

  const setConfig = useCallback(() => {}, []); // No-op, since config is now derived

  const stableValue = useMemo(() => ({
    subscribeRaw,
    getRawSamples,
    clearOldData,
    config,
  }), [subscribeRaw, getRawSamples, clearOldData, config]);

  const dynamicValue = useMemo(() => ({
    dataVersion,
    fftData,
    fullFftPacket,
  }), [dataVersion, fftData, fullFftPacket]);

  const statusValue = useMemo(() => ({
    dataStatus: {
      dataReceived,
      driverError,
      wsStatus,
      isReconnecting,
    },
    isReady,
  }), [dataReceived, driverError, wsStatus, isReconnecting, isReady]);

  return (
    <EegDataStableContext.Provider value={stableValue}>
      <EegDataDynamicContext.Provider value={dynamicValue}>
        <EegDataStatusContext.Provider value={statusValue}>
          {children}
        </EegDataStatusContext.Provider>
      </EegDataDynamicContext.Provider>
    </EegDataStableContext.Provider>
  );
};

// Custom hooks to access the different contexts
export const useEegData = () => {
  const context = useContext(EegDataStableContext);
  if (context === undefined) throw new Error('useEegData must be used within an EegDataProvider');
  return context;
};

export const useEegDynamicData = () => {
  const context = useContext(EegDataDynamicContext);
  if (context === undefined) throw new Error('useEegDynamicData must be used within an EegDataProvider');
  return context;
};

export const useEegStatus = () => {
  const context = useContext(EegDataStatusContext);
  if (context === undefined) throw new Error('useEegStatus must be used within an EegDataProvider');
  return context;
};