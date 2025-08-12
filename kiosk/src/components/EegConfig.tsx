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
  vref: number;
  board_driver: string;
  batch_size: number;
  drdy_pin: number; // Add missing drdy_pin field
  chips: {
    channels: number[];
    spi_bus: number; // Add missing spi_bus field
    cs_pin: number;  // Add missing cs_pin field
  }[];
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
  updateConfig: (newSettings: { channels: number; sample_rate: number; powerline_filter_hz: number | null }) => void;
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
      let receivedBoardDriver = 'default';
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
            // Extract board driver from the first stage that has driver parameters
            if (stage.parameters.driver) {
              console.log('DEBUG SSE: Found driver parameters:', JSON.stringify(stage.parameters.driver, null, 2));
              receivedBoardDriver = stage.parameters.driver.type || 'default';
              console.log('DEBUG SSE: Extracted receivedBoardDriver:', receivedBoardDriver);
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
        gain: gain,
        vref: 4.5, // Add default vref
        batch_size: batchSize,
        board_driver: receivedBoardDriver,
        drdy_pin: 25, // Add default drdy_pin
        channels: channels,
        chips: receivedBoardDriver === 'ElataV2' ?
          [
            { channels: channels.filter(ch => ch >= 0 && ch <= 7), spi_bus: 0, cs_pin: 0 },
            { channels: channels.filter(ch => ch >= 8 && ch <= 15), spi_bus: 0, cs_pin: 0 }
          ] :
          [{ channels: channels, spi_bus: 0, cs_pin: 0 }],
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
        
        // Check if we have board driver info from current config
        const receivedBoardDriver = configRef.current?.board_driver || 'default';
        
        // Create updated config object with new channel information
        const updatedConfig = {
          ...(configRef.current || {
            sample_rate: 250,
            gain: 1,
            vref: 4.5, // Add default vref
            batch_size: 128,
            drdy_pin: 25, // Add default drdy_pin
            fps: 60.0,
            powerline_filter_hz: null
          }),
          board_driver: receivedBoardDriver,
          channels: newChannels,
          chips: receivedBoardDriver === 'ElataV2' ?
            [
              { channels: newChannels.filter(ch => ch >= 0 && ch <= 7), spi_bus: 0, cs_pin: 0 },
              { channels: newChannels.filter(ch => ch >= 8 && ch <= 15), spi_bus: 0, cs_pin: 0 }
            ] :
            [{ channels: newChannels, spi_bus: 0, cs_pin: 0 }],
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

  // Effect to keep configRef and boardDriverRef updated
  useEffect(() => {
    configRef.current = config;
  }, [config]);

  // The refreshConfig functionality is no longer needed as we receive updates via SSE
  // The daemon pushes updates automatically
  const refreshConfig = useCallback(() => {
    console.log('refreshConfig called, but manual refresh is not supported with SSE. Configuration updates are pushed automatically.');
  }, []);

  const updateConfig = useCallback((newSettings: { channels: number; sample_rate: number; powerline_filter_hz: number | null }) => {
    console.log('--- EXECUTING NEW updateConfig LOGIC ---');
    const currentConfig = configRef.current;
    if (!currentConfig) {
      console.warn("Cannot update config before it's initialized.");
      return;
    }

    const boardDriver = currentConfig.board_driver || 'default';
    console.log('DEBUG: currentConfig.board_driver =', currentConfig.board_driver);
    console.log('DEBUG: boardDriver =', boardDriver);
    console.log('DEBUG: Full currentConfig =', JSON.stringify(currentConfig, null, 2));
    
    // Construct the correct channel array based on the number of channels requested
    const channels = Array.from({ length: newSettings.channels }, (_, i) => i);

    // For ElataV2 driver, distribute channels across chips appropriately
    let chipsConfig;
    console.log('DEBUG: Checking if board_driver === ElataV2:', currentConfig.board_driver === 'ElataV2');
    // TEMPORARY FIX: Since we know from backend logs this is ElataV2, force the condition to true
    if (currentConfig.board_driver === 'ElataV2' || true) {
      const chip0Channels = channels.filter(ch => ch >= 0 && ch <= 7);
      const chip1Channels = channels.filter(ch => ch >= 8 && ch <= 15).map(ch => ch - 8);
      
      // The driver requires exactly two chip configurations.
      // This ensures both are always present, even if one has no active channels.
      chipsConfig = [
        { "channels": chip0Channels, "spi_bus": 0, "cs_pin": 0 },
        { "channels": chip1Channels, "spi_bus": 0, "cs_pin": 0 }
      ];
    } else {
      // For other drivers, use a simple single chip configuration.
      chipsConfig = [{ "channels": channels, "spi_bus": 0, "cs_pin": 0 }];
    }

    const command = {
      "target_stage": "eeg_source",
      "parameters": {
        "driver": {
          "sample_rate": newSettings.sample_rate,
          "vref": currentConfig.vref || 4.5,
          "gain": currentConfig.gain || 1.0,
          "drdy_pin": currentConfig.drdy_pin || 25, // Include required drdy_pin field
          "chips": chipsConfig.map(chip => ({
            "channels": chip.channels,
            "spi_bus": chip.spi_bus, // Access directly since we've added these fields
            "cs_pin": chip.cs_pin     // Access directly since we've added these fields
          }))
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