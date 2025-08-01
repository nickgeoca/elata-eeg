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
  isReady: boolean; // New flag to signal when the system is fully initialized
  // Add methods for data management
  clearOldData: () => void;
  // Subscription management
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
  const [isReady, setIsReady] = useState(false); // State to track final configuration readiness
  const rawDataSubscribersRef = useRef<Set<RawDataCallback>>(new Set());

  const { pipelineConfig, pipelineState } = usePipeline(); // Get the pipeline state object
  const { events } = useEventStream();

  const sourceReadyMeta = useMemo(() => {
    const event = events.find(e => e.type === 'SourceReady');
    return event && event.type === 'SourceReady' ? event.data : null;
  }, [events]);

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
    rawDataSubscribersRef.current.forEach(callback => callback(newSampleChunks));
    
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
    if (pipelineState.status === 'started' && sourceReadyMeta?.channel_names) {
      setIsReady(true);
      console.log('[EegDataContext] System is ready. Final configuration has been received.');
    } else {
      setIsReady(false);
    }
  }, [pipelineState.status, sourceReadyMeta]);

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

  // The WebSocket handler is now conditionally enabled
  const { status: wsStatus } = useEegDataHandler({
    enabled: isReady, // Only enable when the system is fully ready
    pipelineState: pipelineState,
    onDataUpdate: handleDataUpdate,
    onError: handleError,
    onSamples: handleSamples,
    onFftData: handleFftData,
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
    isReady,
    clearOldData,
    setConfig: () => {}, // No-op, since config is now derived
  }), [dataVersion, fftData, fullFftPacket, config, dataReceived, driverError, wsStatus, isReconnecting, isReady, getRawSamples, subscribeRaw, clearOldData]);

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