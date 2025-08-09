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
  const configUpdateTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const [dataVersion, setDataVersion] = useState(0);
  const [fftData, setFftData] = useState<Record<number, number[]>>({});
  const [fullFftPacket, setFullFftPacket] = useState<FftPacket | null>(null);
  const metadataRef = useRef<Map<string, SensorMeta>>(new Map());
  const [dataReceived, setDataReceived] = useState(false);
  const [driverError, setDriverError] = useState<string | null>(null);
  const [isReconnecting, setIsReconnecting] = useState(false);
  const [isReady, setIsReady] = useState(false); // State to track final configuration readiness
  const [shouldConnect, setShouldConnect] = useState(false); // State to control when to connect
  const rawDataSubscribersRef = useRef({ raw: {} as Record<string, RawDataCallback> });

  const { pipelineConfig, pipelineStatus } = usePipeline(); // Get the pipeline state object
  const { subscribe } = useEventStream();
  const [sourceReadyMeta, setSourceReadyMeta] = useState<any | null>(null);

  useEffect(() => {
    const unsubscribe = subscribe('SourceReady', (data: any) => {
      if (data.meta) {
        console.log('[EegDataContext] HARD RESET: Received new SourceReady event.', data.meta);

        // 1. Clear ALL existing data buffers
        rawSamplesRef.current = [];
        sampleTimestamps.current = [];

        // 2. Set the new metadata as the source of truth
        setSourceReadyMeta(data.meta);
        metadataRef.current.set('eeg_voltage', data.meta);

        // 3. Force a re-render to propagate changes
        setDataVersion(v => v + 1);
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
    let channels: number[] = [];
    if (eegSourceStage && eegSourceStage.params?.driver?.chips?.length > 0) {
      // Sum up the number of channels from all chips
      const channelCount = eegSourceStage.params.driver.chips.reduce((acc: number, chip: any) => acc + (chip.channels?.length || 0), 0);
      channels = Array.from({ length: channelCount }, (_, i) => i);
    }

    return {
      ...pipelineConfig,
      channels,
      sample_rate: eegSourceStage?.params.sample_rate || 250,
    };
  }, [pipelineConfig, sourceReadyMeta]);
 
  // Create a ref to hold the latest config to avoid stale closures in WebSocket handler
  const configRef = useRef(config);
  useEffect(() => {
    configRef.current = config;
  }, [config]);
  
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
  
      // Clear existing timeout
      if (configUpdateTimeoutRef.current) {
          clearTimeout(configUpdateTimeoutRef.current);
      }
  
      // Debounce buffer clearing
      configUpdateTimeoutRef.current = setTimeout(() => {
          rawSamplesRef.current = [];
          sampleTimestamps.current = [];
          setDataVersion(v => v + 1);
          console.log('[EegDataContext] Cleared buffer due to configuration change');
          
          // Check if we're in React Strict Mode development double-run scenario
          // @ts-ignore - Accessing custom property on window object
          if (process.env.NODE_ENV === 'development' && window[configChangeGuardKey]) {
            return;
          }
          
          // Set the guard to prevent duplicate logging in development mode
          if (process.env.NODE_ENV === 'development') {
            // @ts-ignore - Adding custom property to window object
            window[configChangeGuardKey] = true;
          }
      }, 100); // Small debounce to handle rapid config changes
  
    }, [configKey]);

  // Use a window property to prevent duplicate logging in development mode
  const systemReadyGuardKey = '__eeg_system_ready_guard__';

  // Effect to determine when the system is truly ready
  useEffect(() => {
    // Ready when pipeline is started and the final config with channel names is available
    if (pipelineStatus === 'started' && sourceReadyMeta?.channel_names) {
      setIsReady(true);
      setShouldConnect(true); // Signal that we should connect to data WebSocket
      
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
      // Only reset isReady and shouldConnect if we're not in a reconnection state
      // During reconnection, we want to maintain the previous configuration
      if (!isReconnecting) {
        setIsReady(false);
        // Do not set shouldConnect to false here.
        // We want to keep the WebSocket connection alive during a pipeline restart
        // to avoid a "Disconnected" state on the frontend. The connection
        // will be reused when the new `SourceReady` event arrives.
        // Reset the guard when system is not ready
        if (process.env.NODE_ENV === 'development') {
          // @ts-ignore - Adding custom property to window object
          window[systemReadyGuardKey] = false;
        }
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
  const reconnectTimerRef = useRef<NodeJS.Timeout | null>(null); // For managing reconnection timer
  
  // Use window property to track connection attempts in React Strict Mode
  // This persists across double executions unlike refs which are reset per component instance
  const connectionGuardKey = '__eeg_websocket_connection_guard__';

  const connect = useCallback(() => {
    // Check if we should connect to WebSocket
    if (!shouldConnect) {
      return;
    }
    
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

      // Dynamically subscribe to the topic from the sourceReady event
      if (sourceReadyMeta?.source_type) {
        const topic = sourceReadyMeta.source_type === 'eeg_source' ? 'eeg_voltage' : 'fft';
        const subscriptionMessage = {
          type: 'subscribe',
          topic: topic,
        };
        socket.send(JSON.stringify(subscriptionMessage));
        console.log(`[EegDataContext] Subscribed to topic: ${subscriptionMessage.topic}`);
      } else {
        console.warn('[EegDataContext] Could not subscribe to data topic: sourceReadyMeta is not available.');
      }
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
            metadataRef.current.set(metaUpdate.topic, metaUpdate.meta);
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

          // 3. Look up metadata and validate its revision
          const meta = metadataRef.current.get(header.topic);
          if (!meta || meta.meta_rev !== header.meta_rev) {
            if (!meta) {
              console.warn(`[EegDataContext] Received data packet for topic "${header.topic}" without metadata. Dropping packet.`);
            } else {
              console.warn(`Dropping packet due to stale metadata. Frontend: ${meta?.meta_rev}, Packet: ${header.meta_rev}`);
            }
            return; // DROP THE PACKET
          }

          // 4. Create Float32Array view on the sample data (zero-copy)
          const samplesOffset = jsonHeaderOffset + jsonHeaderLen;
          
          // Calculate padding added by backend to ensure 4-byte alignment
          const jsonPadding = (4 - (jsonHeaderLen % 4)) % 4;
          const alignedOffset = samplesOffset + jsonPadding;

          if (header.packet_type === 'Voltage' || header.packet_type === 'RawI32') {
            let samples: Float32Array;

            if (header.packet_type === 'RawI32') {
              // Convert RawI32 to Voltage
              const rawSamples = new Int32Array(buffer, alignedOffset);
              samples = new Float32Array(rawSamples.length);
              
              const adcBits = meta.adc_bits || 24;
              const vRef = meta.v_ref || 4.5;
              const gain = meta.gain || 1.0;
              const scaleFactor = (vRef / (Math.pow(2, adcBits) - 1) / gain) * 1000000;

              for (let i = 0; i < rawSamples.length; i++) {
                samples[i] = rawSamples[i] * scaleFactor;
              }
            } else { // 'Voltage'
              samples = new Float32Array(buffer, alignedOffset);
            }
            
            if (meta) {
                const newChunk: SampleChunk = {
                    meta: meta,
                    samples: samples,
                    timestamp: header.ts_ns,
                };
                handleSamplesRef.current(newChunk);
            }
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
        // Reset shouldConnect to allow reconnection attempts
        setShouldConnect(false);
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
        if (!reconnectTimerRef.current) {
          reconnectTimerRef.current = setTimeout(() => {
            if (!isCleanupRef.current) {
              console.log('[EegDataContext] Attempting to reconnect WebSocket');
              // Reset the connection attempt guard to allow reconnection
              // @ts-ignore - Adding custom property to window object
              window[connectionGuardKey] = false;
              // Set shouldConnect to true to trigger reconnection attempts
              setShouldConnect(true);
              // Trigger reconnection by forcing a re-render
              setDataReceived(false);
            }
            reconnectTimerRef.current = null;
          }, 1000);
        }
      }
    };
    // The cleanup function is critical for preventing memory leaks and race conditions.
    return () => {
      console.log('[EegDataContext] Cleanup: Closing WebSocket');
      // Mark this as intentional cleanup to prevent error handling
      isCleanupRef.current = true;
      // Remove event listeners to prevent them from being called on a stale socket instance.
      if (ws.current) {
        ws.current.onopen = null;
        ws.current.onmessage = null;
        ws.current.onerror = null;
        ws.current.onclose = null;
        ws.current.close();
      }
      // Reset the connection guard on window to allow for new connection attempts.
      // @ts-ignore - Adding custom property to window object
      window[connectionGuardKey] = false;
      ws.current = null;
      // Reset the cleanup flag
      isCleanupRef.current = false;
    };
  }, [shouldConnect, sourceReadyMeta]); // Add sourceReadyMeta as a dependency

  // This useEffect manages the WebSocket connection lifecycle.
  // It runs ONLY when shouldConnect changes from false to true.
  useEffect(() => {
    if (shouldConnect) {
      connect();
    }
  }, [shouldConnect]); // REMOVED `connect` from dependency array


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