'use client';

import React, { createContext, useContext, useState, ReactNode, useMemo, useRef, useCallback } from 'react';
import { useEegDataHandler } from '../components/EegDataHandler';
import { useEegConfig } from '../components/EegConfig';

// Constants for data management
const MAX_SAMPLE_CHUNKS = 100; // Maximum number of sample chunks to keep in memory
const RECONNECTION_DATA_RETENTION_MS = 5000; // Keep data for 5 seconds during reconnections

// Define the shape of the context data
interface EegSample {
  value: number;
  timestamp: bigint;
}

interface EegDataContextType {
  rawSamples: EegSample[][]; // A circular buffer of raw data chunks
  fftData: Record<number, number[]>; // Latest FFT data per channel
  config: any;
  dataStatus: {
    dataReceived: boolean;
    driverError: string | null;
    wsStatus: string;
    isReconnecting: boolean;
  };
  // Add methods for data management
  clearOldData: () => void;
  getLatestSamples: (count?: number) => EegSample[][];
}

// Create the context with a default value
const EegDataContext = createContext<EegDataContextType | undefined>(undefined);

// Define the props for the provider component
interface EegDataProviderProps {
  children: ReactNode;
}

export const EegDataProvider = ({ children }: EegDataProviderProps) => {
  const { config } = useEegConfig();
  const [rawSamples, setRawSamples] = useState<EegSample[][]>([]);
  const [fftData, setFftData] = useState<Record<number, number[]>>({});
  const [dataReceived, setDataReceived] = useState(false);
  const [driverError, setDriverError] = useState<string | null>(null);
  const [isReconnecting, setIsReconnecting] = useState(false);
  
  // Use refs to track data timestamps for cleanup
  const sampleTimestamps = useRef<number[]>([]);
  const lastCleanupTime = useRef<number>(Date.now());
  
  // Create stable refs for EegDataHandler to prevent unnecessary reconnections
  const lastDataChunkTimeRef = useRef<number[]>([]);
  const latestTimestampRef = useRef<number>(0);
  const debugInfoRef = useRef({ lastPacketTime: 0, packetsReceived: 0, samplesProcessed: 0 });

  const handleSamples = useCallback((channelSamples: { values: Float32Array; timestamps: BigUint64Array }[]) => {
    const now = Date.now();

    const newSampleChunks: EegSample[][] = channelSamples.map(channelData => {
      const samples: EegSample[] = [];
      for (let i = 0; i < channelData.values.length; i++) {
        samples.push({
          value: channelData.values[i],
          timestamp: channelData.timestamps[i],
        });
      }
      return samples;
    });

    setRawSamples(prevSamples => {
      const newSamples = [...prevSamples, ...newSampleChunks];
      sampleTimestamps.current.push(now);
      
      // Implement circular buffer - remove old samples if we exceed the limit
      if (newSamples.length > MAX_SAMPLE_CHUNKS) {
        const removeCount = newSamples.length - MAX_SAMPLE_CHUNKS;
        sampleTimestamps.current.splice(0, removeCount);
        return newSamples.slice(removeCount);
      }
      
      return newSamples;
    });
    
    // Periodic cleanup of old data (every 10 seconds)
    if (now - lastCleanupTime.current > 10000) {
      cleanupOldData();
      lastCleanupTime.current = now;
    }
  }, []);

  const handleFftData = useCallback((channelIndex: number, fftOutput: number[]) => {
    setFftData(prevFftData => ({
      ...prevFftData,
      [channelIndex]: fftOutput,
    }));
  }, []);

  const cleanupOldData = useCallback(() => {
    const now = Date.now();
    const cutoffTime = now - RECONNECTION_DATA_RETENTION_MS;
    
    setRawSamples(prevSamples => {
      // Find the first index to keep
      const firstValidIndex = sampleTimestamps.current.findIndex(timestamp => timestamp > cutoffTime);
      
      if (firstValidIndex > 0) {
        // Remove old timestamps and samples
        sampleTimestamps.current.splice(0, firstValidIndex);
        return prevSamples.slice(firstValidIndex);
      }
      
      return prevSamples;
    });
  }, []);

  const clearOldData = useCallback(() => {
    setRawSamples([]);
    sampleTimestamps.current = [];
    console.log('[EegDataContext] Cleared old data due to manual request');
  }, []);

  const getLatestSamples = useCallback((count: number = 10) => {
    return rawSamples.slice(-count);
  }, [rawSamples]);

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
    config,
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
    rawSamples,
    fftData,
    config,
    dataStatus: {
      dataReceived,
      driverError,
      wsStatus,
      isReconnecting,
    },
    clearOldData,
    getLatestSamples,
  }), [rawSamples, fftData, config, dataReceived, driverError, wsStatus, isReconnecting, clearOldData, getLatestSamples]);

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