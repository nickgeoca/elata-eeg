'use client';

import React, { createContext, useContext, useState, ReactNode, useMemo, useRef, useCallback, useEffect } from 'react';
import { useEventStream } from './EventStreamContext';
import { usePipeline } from './PipelineContext'; // Import the usePipeline hook
import { SampleChunk, SensorMeta, MetaUpdateMsg, DataPacketHeader } from '../types/eeg'; // Import shared types

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
  const [metadata, setMetadata] = useState<Map<string, SensorMeta>>(new Map());
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

  const handleSamples = useCallback((newChunk: SampleChunk) => {
    const now = Date.now();
    
    // The new chunk is now directly what we want to store.
    const newSamples = [...rawSamplesRef.current, newChunk];
    sampleTimestamps.current.push(now);

    // Enforce a hard limit on the number of chunks to prevent memory leaks
    if (newSamples.length > MAX_SAMPLE_CHUNKS) {
      const excess = newSamples.length - MAX_SAMPLE_CHUNKS;
      sampleTimestamps.current.splice(0, excess);
      rawSamplesRef.current = newSamples.slice(excess);
    } else {
      rawSamplesRef.current = newSamples;
    }
    
    setDataVersion(v => v + 1);
    // Always update data received status when we get new data
    handleDataUpdateRef.current(true);
    
    // Publish the new data to all subscribers. We wrap it in an array to maintain
    // the existing callback signature which expects an array of chunks.
    Object.values(rawDataSubscribersRef.current.raw).forEach(callback => callback([newChunk]));
    
    // Periodic cleanup of old data (every 10 seconds)
    if (now - lastCleanupTime.current > 10000) {
      cleanupOldData();
      lastCleanupTime.current = now;
    }
  }, [cleanupOldData]);

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

  // Use a window property to prevent duplicate logging in development mode
  const configChangeGuardKey = '__eeg_config_change_guard__';

  // Clear buffer when the stable configuration key changes
  useEffect(() => {
    // Don't clear the buffer on the initial load when configKey is null
    if (configKey === null) return;

    // Check if we're in React Strict Mode development double-run scenario
    // @ts-ignore - Accessing custom property on window object
    if (process.env.NODE_ENV === 'development' && window[configChangeGuardKey]) {
      return;
    }

    rawSamplesRef.current = [];
    sampleTimestamps.current = [];
    setDataVersion(v => v + 1); // Atomically notify consumers of the change
    
    // Set the guard and log the message
    if (process.env.NODE_ENV === 'development') {
      // @ts-ignore - Adding custom property to window object
      window[configChangeGuardKey] = true;
    }
    console.log('[EegDataContext] Cleared buffer due to configuration change');
  }, [configKey]);

  // Use a window property to prevent duplicate logging in development mode
  const systemReadyGuardKey = '__eeg_system_ready_guard__';

  // Effect to determine when the system is truly ready
  useEffect(() => {
    // Ready when pipeline is started and the final config with channel names is available
    if (pipelineStatus === 'started' && sourceReadyMeta?.channel_names) {
      setIsReady(true);
      
      // Check if we're in React Strict Mode development double-run scenario
      // @ts-ignore - Accessing custom property on window object
      if (!(process.env.NODE_ENV === 'development' && window[systemReadyGuardKey])) {
        if (process.env.NODE_ENV === 'development') {
          // @ts-ignore - Adding custom property to window object
          window[systemReadyGuardKey] = true;
        }
        console.log('[EegDataContext] System is ready. Final configuration has been received.');
      }
    } else {
      setIsReady(false);
      // Reset the guard when system is not ready
      if (process.env.NODE_ENV === 'development') {
        // @ts-ignore - Adding custom property to window object
        window[systemReadyGuardKey] = false;
      }
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

  const handleDataUpdateRef = useRef(handleDataUpdate);
  
  useEffect(() => {
    handleDataUpdateRef.current = handleDataUpdate;
  }, [handleDataUpdate]);

  const handleError = useCallback((error: string | null) => {
    setDriverError(error);
    if (error && !isReconnecting) {
      setIsReconnecting(true);
      console.log('[EegDataContext] Connection error detected, entering reconnection mode');
    }
  }, [isReconnecting]);

  const [wsStatus, setWsStatus] = useState('Disconnected');
  const ws = useRef<WebSocket | null>(null);
  const isCleanupRef = useRef(false); // Track if we're in cleanup phase
  
  // Use window property to track connection attempts in React Strict Mode
  // This persists across double executions unlike refs which are reset per component instance
  const connectionGuardKey = '__eeg_websocket_connection_guard__';

  // This useEffect manages the WebSocket connection lifecycle. It runs once on mount.
  useEffect(() => {
    // Check if we're in React Strict Mode development double-run scenario
    // In Strict Mode, the first run sets the window guard, and the second run should be ignored
    // @ts-ignore - Accessing custom property on window object
    if (window[connectionGuardKey]) {
      console.log('[EegDataContext] Connection attempt already made, skipping duplicate connection attempt.');
      return;
    }

    // Ensure we don't create duplicate connections
    if (ws.current) {
      console.log('[EegDataContext] Duplicate connection.');
      return;
    }
    
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const host = window.location.hostname;
    const url = `${protocol}//${host}:9000/ws/data`;

    console.log('[EegDataContext] Connecting to WebSocket:', url);
    setWsStatus('Connecting...');
    const socket = new WebSocket(url);
    socket.binaryType = 'arraybuffer';
    
    // Set the connection guard on window to prevent duplicate connections
    // @ts-ignore - Adding custom property to window object
    window[connectionGuardKey] = true;
    ws.current = socket;

    socket.onopen = () => {
      console.log('[EegDataContext] WebSocket connection established');
      setWsStatus('Connected');
    };

    // Define the message handler inside the effect to create a stable closure
    // over the handleSamples and handleFftData callbacks.
    socket.onmessage = (event: MessageEvent) => {
      try {
        // Handle meta_update messages (JSON string)
        if (typeof event.data === 'string') {
          const msg = JSON.parse(event.data);
          if (msg.message_type === 'meta_update') {
            const metaUpdate = msg as MetaUpdateMsg;
            console.log(`[EegDataContext] Received metadata for topic: ${metaUpdate.topic}`, metaUpdate.meta);
            setMetadata(prev => new Map(prev).set(metaUpdate.topic, metaUpdate.meta));
          } else if (msg.topic === 'fft') {
            handleFftDataRef.current(msg);
          }
          return;
        }

        // Handle data_packet messages (binary)
        if (event.data instanceof ArrayBuffer) {
          const buffer = event.data;
          const dataView = new DataView(buffer);

          // 1. Read header length
          const jsonHeaderLen = dataView.getUint32(0, true);
          const jsonHeaderOffset = 4;

          // 2. Decode JSON header
          const jsonHeaderBytes = buffer.slice(jsonHeaderOffset, jsonHeaderOffset + jsonHeaderLen);
          const jsonHeaderStr = new TextDecoder().decode(jsonHeaderBytes);
          const header = JSON.parse(jsonHeaderStr) as DataPacketHeader;

          // 3. Look up metadata
          let meta = metadata.get(header.topic);
          if (!meta) {
            console.warn(`[EegDataContext] Received data packet for topic "${header.topic}" without metadata. Using default metadata.`);
            // Create default metadata to prevent data loss
            meta = {
              sensor_id: 1,
              meta_rev: 1,
              schema_ver: 1,
              source_type: "eeg_source",
              v_ref: 4.5,
              adc_bits: 24,
              gain: 1,
              sample_rate: 250,
              offset_code: 0,
              is_twos_complement: true,
              channel_names: ["CH0", "CH1", "CH2", "CH3", "CH4", "CH5", "CH6", "CH7"]
            };
          }

          // 4. Create Float332Array view on the sample data (zero-copy)
          const samplesOffset = jsonHeaderOffset + jsonHeaderLen;
          
          // Calculate padding added by backend to ensure 4-byte alignment
          const jsonPadding = (4 - (jsonHeaderLen % 4)) % 4;
          const alignedOffset = samplesOffset + jsonPadding;

          if (header.packet_type === 'Voltage') {
            const samples = new Float32Array(buffer, alignedOffset);
            
            const newChunk: SampleChunk = {
              meta: meta,
              samples: samples,
              timestamp: header.ts_ns,
            };

            handleSamplesRef.current(newChunk);
          } else {
            console.warn(`[EegDataContext] Received unhandled packet type: ${header.packet_type}`);
          }
        }
      } catch (error) {
        console.error("Failed to parse or handle WebSocket message:", error);
      }
    };

    socket.onerror = (err) => {
      // Only handle errors if the socket is in a connecting or open state
      // and we're not in a cleanup phase.
      // This prevents logging errors when the connection is intentionally closed by the cleanup function.
      if ((socket.readyState === WebSocket.CONNECTING || socket.readyState === WebSocket.OPEN) && 
          !isCleanupRef.current) {
        console.error('[EegDataContext] WebSocket error:', err);
        setWsStatus('Error');
        // Reset the WebSocket reference since the connection failed
        ws.current = null;
        // Reset connection guard on window when connection fails
        // @ts-ignore - Adding custom property to window object
        window[connectionGuardKey] = false;
      }
    };

    socket.onclose = (event) => {
      console.log('[EegDataContext] WebSocket connection closed', event);
      // Only update state if this is the active socket that was closed
      // and we're not in a cleanup phase.
      if (ws.current === socket && !isCleanupRef.current) {
        setWsStatus('Disconnected');
        // Reset the WebSocket reference
        ws.current = null;
        // Reset connection guard on window when connection closes
        // @ts-ignore - Adding custom property to window object
        window[connectionGuardKey] = false;
        
        // Attempt to reconnect after a delay
        setTimeout(() => {
          if (!isCleanupRef.current) {
            console.log('[EegDataContext] Attempting to reconnect WebSocket');
            // Reset the connection attempt guard to allow reconnection
            // @ts-ignore - Adding custom property to window object
            window[connectionGuardKey] = false;
            // Trigger reconnection by forcing a re-render
            setDataReceived(false);
          }
        }, 1000);
      }
    };
    // The cleanup function is critical for preventing memory leaks and race conditions.
    return () => {
      console.log('[EegDataContext] Cleanup: Closing WebSocket');
      // Mark this as intentional cleanup to prevent error handling
      isCleanupRef.current = true;
      // Remove event listeners to prevent them from being called on a stale socket instance.
      socket.onopen = null;
      socket.onmessage = null;
      socket.onerror = null;
      socket.onclose = null;
      socket.close();
      // Reset the connection guard on window to allow for new connection attempts.
      // @ts-ignore - Adding custom property to window object
      window[connectionGuardKey] = false;
      ws.current = null;
    };
  }, []); // Empty dependency array ensures this runs only once on mount.

  // This useEffect manages sending subscribe/unsubscribe messages based on system readiness.
  useEffect(() => {
    if (ws.current && ws.current.readyState === WebSocket.OPEN) {
      if (isReady) {
        console.log('[EegDataContext] System is ready, subscribing to eeg_voltage topic.');
        ws.current.send(JSON.stringify({ subscribe: 'eeg_voltage' }));
      } else {
        console.log('[EegDataContext] System not ready, unsubscribing from eeg_voltage topic.');
        ws.current.send(JSON.stringify({ unsubscribe: 'eeg_voltage' }));
      }
    }
  }, [isReady, wsStatus]); // Re-run when readiness or connection status changes.

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

// Custom hook to use the stable parts of the EEG data context
export const useEegData = () => {
  const context = useContext(EegDataStableContext);
  if (context === undefined) {
    throw new Error('useEegData must be used within an EegDataProvider');
  }
  return context;
};

// Custom hook to use the dynamic parts of the EEG data context
export const useEegDynamicData = () => {
  const context = useContext(EegDataDynamicContext);
  if (context === undefined) {
    throw new Error('useEegDynamicData must be used within an EegDataProvider');
  }
  return context;
};

// Custom hook to use the status parts of the EEG data context
export const useEegStatus = () => {
  const context = useContext(EegDataStatusContext);
  if (context === undefined) {
    throw new Error('useEegStatus must be used within an EegDataProvider');
  }
  return context;
};