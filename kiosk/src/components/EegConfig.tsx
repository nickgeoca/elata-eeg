'use client';

import { useEffect, useState, createContext, useContext, useCallback, useMemo, useRef } from 'react';
import { useEventStream } from '@/context/EventStreamContext';

// Define the EEG configuration interface
export interface EegConfig {
  // Defined from daemon
  sample_rate: number;
  channels: number[];
  gain: number;
  board_driver: string;
  batch_size: number;
  // Defined from config file
  fps: number;
  // Powerline filter setting
  powerline_filter_hz: number | null; // Changed from optional to required, can be null
}

// Create a context to share the configuration across components
interface EegConfigContextType {
  config: EegConfig | null;
  status: string;
  refreshConfig: () => void;
  isConfigReady: boolean;
}

export const EegConfigContext = createContext<EegConfigContextType>({
  config: null,
  status: 'Initializing...',
  refreshConfig: () => { console.warn('EegConfigContext: refreshConfig called before provider initialization'); },
  isConfigReady: false,
});

// Hook to use the EEG configuration
export const useEegConfig = () => useContext(EegConfigContext);

// Provider component
export function EegConfigProvider({ children }: { children: React.ReactNode }) {
  const [config, setConfig] = useState<EegConfig | null>(null);
  const configRef = useRef<EegConfig | null>(null); // Add a ref for the latest config
  const [status, setStatus] = useState('Initializing...'); // Start as Initializing
  const [isConfigReady, setIsConfigReady] = useState(false);
  const isProduction = process.env.NODE_ENV === 'production';
  const { events, isConnected, error } = useEventStream();

  // Helper function to deeply compare relevant parts of EEG configurations
  // Compares server-provided data against the current state (excluding client-added 'fps')
  const areConfigsEqual = (currentConfig: EegConfig | null, newServerData: any): boolean => {
    if (!currentConfig) return false;
    // Create string representations to safely compare.
    const currentChannels = JSON.stringify(currentConfig.channels?.slice().sort());
    const newChannels = JSON.stringify(newServerData.channels?.slice().sort());

    return (
      currentConfig.sample_rate === newServerData.sample_rate &&
      currentConfig.gain === newServerData.gain &&
      currentConfig.board_driver === newServerData.board_driver &&
      currentConfig.batch_size === newServerData.batch_size &&
      currentConfig.powerline_filter_hz === newServerData.powerline_filter_hz &&
      currentChannels === newChannels
    );
  };

  // Main effect to handle SSE events
  useEffect(() => {
    // Update status based on SSE connection
    if (error) {
      setStatus('Error');
    } else if (!isConnected) {
      setStatus('Connecting...');
    } else {
      setStatus('Connected');
    }
  }, [isConnected, error]);

  // Effect to process incoming events from the EventStream
  useEffect(() => {
    // Process the latest event
    if (events.length > 0) {
      const latestEvent = events[events.length - 1];
      
      // Only process pipeline_state events which contain the full configuration
      if (latestEvent.type === 'pipeline_state') {
        const pipelineData = latestEvent.data;
        
        // Extract configuration from pipeline stages if available
        // Look for a stage that might contain configuration parameters
        let sampleRate = 250;
        let channels = [0, 1, 2, 3];
        let gain = 1;
        let boardDriver = 'default';
        let batchSize = 128;
        let powerlineFilterHz: number | null = null;
        
        // Check if there's a stage with configuration parameters
        if (pipelineData.stages && Array.isArray(pipelineData.stages)) {
          for (const stage of pipelineData.stages) {
            if (stage.parameters) {
              if (stage.parameters.sample_rate) {
                sampleRate = stage.parameters.sample_rate;
              }
              if (stage.parameters.gain) {
                gain = stage.parameters.gain;
              }
              if (stage.parameters.powerline_filter_hz !== undefined) {
                powerlineFilterHz = stage.parameters.powerline_filter_hz;
              }
            }
          }
        }
        
        const serverConfig = {
          sample_rate: sampleRate,
          channels: channels,
          gain: gain,
          board_driver: boardDriver,
          batch_size: batchSize,
          fps: 60.0,
          powerline_filter_hz: powerlineFilterHz
        };
        
        // Check if the configuration has changed
        if (!areConfigsEqual(configRef.current, serverConfig)) {
          console.log('EegConfigProvider: Received new configuration via SSE, applying update.');
          setConfig(serverConfig); // This will trigger a re-render for consumers
          
          if (!isConfigReady) {
            setIsConfigReady(true);
          }
        } else {
          if (!isProduction) {
            console.log('EegConfigProvider: Received identical configuration via SSE. No update needed.');
          }
        }
      }
    }
  }, [events, areConfigsEqual, configRef, setConfig, isConfigReady, isProduction]);

  // Effect to keep configRef updated
  useEffect(() => {
    configRef.current = config;
  }, [config]);

  // The refreshConfig functionality is no longer needed as we receive updates via SSE
  // The daemon pushes updates automatically
  const refreshConfig = useCallback(() => {
    console.log('refreshConfig called, but manual refresh is not supported with SSE. Configuration updates are pushed automatically.');
  }, []);

  const contextValue = useMemo(() => ({
    config,
    status,
    refreshConfig,
    isConfigReady,
  }), [config, status, refreshConfig, isConfigReady]);

  return (
    <EegConfigContext.Provider value={contextValue}>
      {children}
    </EegConfigContext.Provider>
  );
}

// Display component removed