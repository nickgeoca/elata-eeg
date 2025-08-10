'use client';

import { useEffect, useState, createContext, useContext, useCallback, useMemo, useRef } from 'react';
import { useEventStream, useEventStreamData } from '@/context/EventStreamContext';
import { usePipeline } from '@/context/PipelineContext';

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
  updateConfig: (newConfig: Partial<EegConfig>) => void;
}

export const EegConfigContext = createContext<EegConfigContextType>({
  config: null,
  status: 'Initializing...',
  refreshConfig: () => { console.warn('EegConfigContext: refreshConfig called before provider initialization'); },
  isConfigReady: false,
  updateConfig: () => { console.warn('EegConfigContext: updateConfig called before provider initialization'); },
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
  const { subscribe } = useEventStream();
  const { isConnected, error } = useEventStreamData();
  const { sendCommand } = usePipeline();

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

  // Main effect to handle SSE connection status
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

  // Effect to subscribe to pipeline_state events
  useEffect(() => {
    const unsubscribe = subscribe('pipeline_state', (pipelineData: any) => {
      // Extract configuration from pipeline stages if available
      // Look for a stage that might contain configuration parameters
      let sampleRate = 250;
      let channels: number[] = [];
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
            // Extract channels from the first stage that has them
            if (stage.parameters.channels) {
              channels = stage.parameters.channels;
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
      
      // If no channels were found in pipeline stages, set a default
      if (channels.length === 0) {
        channels = [0, 1, 2, 3];
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
    });
    
    return () => {
      unsubscribe();
    };
  }, [subscribe, areConfigsEqual, isConfigReady, isProduction]);

  // Effect to also listen for SourceReady events that contain channel metadata
  useEffect(() => {
    const unsubscribe = subscribe('SourceReady', (data: any) => {
      // The actual metadata is nested inside the 'meta' property of the event data
      if (data && data.meta && data.meta.channel_names && Array.isArray(data.meta.channel_names)) {
        // Update the channel configuration based on received metadata
        const newChannelCount = data.meta.channel_names.length;
        const newChannels = Array.from({ length: newChannelCount }, (_, i) => i);
        
        // Create updated config object with new channel information
        const updatedConfig = {
          ...(configRef.current || {
            sample_rate: 250,
            gain: 1,
            board_driver: 'default',
            batch_size: 128,
            fps: 60.0,
            powerline_filter_hz: null
          }),
          channels: newChannels
        };
        
        // Check if the configuration has changed
        if (!areConfigsEqual(configRef.current, updatedConfig)) {
          console.log('EegConfigProvider: Received new channel configuration via SourceReady event, applying update.');
          setConfig(updatedConfig); // This will trigger a re-render for consumers
          
          if (!isConfigReady) {
            setIsConfigReady(true);
          }
        } else {
          if (!isProduction) {
            console.log('EegConfigProvider: Received identical channel configuration via SourceReady. No update needed.');
          }
        }
      }
    });

    return () => {
      unsubscribe();
    };
  }, [subscribe, areConfigsEqual, isConfigReady, isProduction]);

  // Effect to keep configRef updated
  useEffect(() => {
    configRef.current = config;
  }, [config]);

  // The refreshConfig functionality is no longer needed as we receive updates via SSE
  // The daemon pushes updates automatically
  const refreshConfig = useCallback(() => {
    console.log('refreshConfig called, but manual refresh is not supported with SSE. Configuration updates are pushed automatically.');
  }, []);

  const updateConfig = useCallback((newConfig: Partial<EegConfig>) => {
      if (!configRef.current) {
        console.warn("Cannot update config before it's initialized.");
        return;
      }
  
      // Explicitly build the command to ensure all fields are present.
      const sampleRate = configRef.current.sample_rate || 250; // Use current or default
      const channels = newConfig.channels || configRef.current.channels || [];
      const vref = configRef.current.vref || 4.5; // Use current or default
      const gain = configRef.current.gain || 1.0; // Use current or default
  
      const command = {
        "eeg_source": {
          "driver": {
            "sample_rate": sampleRate,
            "vref": vref,
            "gain": gain,
            "chips": [
              {
                "channels": channels,
                "spi_bus": 0, // Default value
                "cs_pin": 0   // Default value
              }
            ]
          }
        }
      };
  
      console.log("Sending SetParameter command with payload:", JSON.stringify(command, null, 2));
      sendCommand('SetParameter', command);
    }, [sendCommand]);

  const contextValue = useMemo(() => ({
    config,
    status,
    refreshConfig,
    isConfigReady,
    updateConfig,
  }), [config, status, refreshConfig, isConfigReady, updateConfig]);

  return (
    <EegConfigContext.Provider value={contextValue}>
      {children}
    </EegConfigContext.Provider>
  );
}

// Display component removed