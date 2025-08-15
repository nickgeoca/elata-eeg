'use client';

import { useEffect, useState, createContext, useContext, useCallback, useMemo, useRef } from 'react';
import { useEventStream, useEventStreamData } from '@/context/EventStreamContext';
import { usePipeline } from '@/context/PipelineContext';
import { buildDriverPayload } from '@/utils/driverPayload';

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
  updateConfig: (newSettings: { channels: number; sample_rate: number; powerline_filter_hz: number | null; gain?: number }) => void;
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
  const lastDriverTypeRef = useRef<string | null>(null); // Track latest driver.type from SSE
  const [status, setStatus] = useState('Initializing...'); // Start as Initializing
  const [isConfigReady, setIsConfigReady] = useState(false);
  const isProduction = process.env.NODE_ENV === 'production';
  const { subscribe } = useEventStream();
  const { isConnected, error } = useEventStreamData();
  const { sendCommand, pipelineState } = usePipeline();

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

  // Effect to subscribe to backend config events (PipelineStarted/ConfigUpdated)
  useEffect(() => {
    const handleConfigEvent = (payload: any) => {
      // Payload shapes:
      // - PipelineStarted: { id, config }
      // - ConfigUpdated: { config }
      const cfg = payload?.config ?? payload;
      const stages = cfg?.stages;

      let sampleRate = 250;
      let channels: number[] = [];
      let gain = 1;
      let receivedBoardDriver = 'default';
      let batchSize = 128;
      let powerlineFilterHz: number | null = null;

      if (Array.isArray(stages)) {
        // Prefer values from the eeg_source stage only
        const eegStage = stages.find((s: any) => (s?.type === 'eeg_source' || s?.stage_type === 'eeg_source' || s?.name === 'eeg_source')) || stages[0];
        const params = eegStage?.params || eegStage?.parameters; // tolerate both
        if (params) {
          // sample_rate may be on stage or nested under driver
          sampleRate = params.sample_rate ?? params?.driver?.sample_rate ?? sampleRate;
          gain = params.gain ?? params?.driver?.gain ?? gain;
          if (params.powerline_filter_hz !== undefined) powerlineFilterHz = params.powerline_filter_hz;
          if (params.batch_size !== undefined) batchSize = params.batch_size;

          if (params.driver) {
            console.log('DEBUG SSE: Found driver parameters:', JSON.stringify(params.driver, null, 2));
            receivedBoardDriver = params.driver.type || 'default';
            console.log('DEBUG SSE: Extracted receivedBoardDriver:', receivedBoardDriver);
            lastDriverTypeRef.current = receivedBoardDriver; // cache latest driver type

            // Derive channel list from driver chips when present
            if (Array.isArray(params.driver.chips)) {
              if (receivedBoardDriver === 'ElataV2') {
                const chip0 = params.driver.chips[0]?.channels || [];
                const chip1Local = params.driver.chips[1]?.channels || [];
                const chip1Global = chip1Local.map((c: number) => c + 8);
                channels = ([] as number[]).concat(chip0, chip1Global);
              } else if (params.driver.chips[0]?.channels) {
                channels = params.driver.chips[0].channels.slice();
              }
            }
          }

          // Only override channels from stage params if it is an array
          if (Array.isArray(params.channels)) {
            channels = params.channels;
          }
        }
      }

      if (!Array.isArray(channels) || channels.length === 0) channels = [0, 1, 2, 3];

      const serverConfig = {
        sample_rate: sampleRate,
        gain: gain,
        vref: 4.5,
        batch_size: batchSize,
        board_driver: receivedBoardDriver,
        drdy_pin: 25,
        channels,
        chips: receivedBoardDriver === 'ElataV2'
          ? [
              { channels: channels.filter((ch: number) => ch >= 0 && ch <= 7), spi_bus: 0, cs_pin: 0 },
              { channels: channels.filter((ch: number) => ch >= 8 && ch <= 15).map((ch: number) => ch - 8), spi_bus: 0, cs_pin: 0 },
            ]
          : [{ channels, spi_bus: 0, cs_pin: 0 }],
        fps: 60.0,
        powerline_filter_hz: powerlineFilterHz,
      };

      if (!areConfigsEqual(configRef.current, serverConfig)) {
        console.log('EegConfigProvider: Received new configuration via SSE, applying update.');
        setConfig(serverConfig);
        if (!isConfigReady) setIsConfigReady(true);
      } else if (!isProduction) {
        console.log('EegConfigProvider: Received identical configuration via SSE. No update needed.');
      }
    };

    const unsubscribeStarted = subscribe('PipelineStarted', handleConfigEvent);
    const unsubscribeUpdated = subscribe('ConfigUpdated', handleConfigEvent);

    return () => {
      unsubscribeStarted();
      unsubscribeUpdated();
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
        if (receivedBoardDriver) {
          lastDriverTypeRef.current = receivedBoardDriver; // keep cache in sync
        }
        
        // Create updated config object with new channel information
        const prev = (configRef.current || {
          sample_rate: 250,
          gain: 1,
          vref: 4.5,
          batch_size: 128,
          drdy_pin: 25,
          fps: 60.0,
          powerline_filter_hz: null
        });
        const updatedConfig = {
          ...prev,
          sample_rate: data.meta?.sample_rate ?? prev.sample_rate,
          gain: data.meta?.gain ?? prev.gain,
          vref: data.meta?.v_ref ?? prev.vref,
          board_driver: receivedBoardDriver,
          channels: newChannels,
          chips: receivedBoardDriver === 'ElataV2' ?
            [
              { channels: newChannels.filter((ch: number) => ch >= 0 && ch <= 7), spi_bus: 0, cs_pin: 0 },
              { channels: newChannels.filter((ch: number) => ch >= 8 && ch <= 15).map((ch: number) => ch - 8), spi_bus: 0, cs_pin: 0 }
            ] :
            [{ channels: newChannels, spi_bus: 0, cs_pin: 0 }],
        } as EegConfig;
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

  // On mount or when pipelineState changes, try to prime lastDriverType from current state
  useEffect(() => {
    const cfg = (pipelineState && (pipelineState as any).config) || null;
    const stages = cfg?.stages;
    if (Array.isArray(stages)) {
      const eegStage = stages.find((s: any) => (s?.type === 'eeg_source' || s?.stage_type === 'eeg_source' || s?.name === 'eeg_source')) || stages[0];
      const params = eegStage?.params || eegStage?.parameters;
      const drvType = params?.driver?.type;
      if (typeof drvType === 'string' && drvType.length > 0) {
        lastDriverTypeRef.current = drvType;
      }
    }
  }, [pipelineState]);

  // Effect to keep configRef and boardDriverRef updated
  useEffect(() => {
    configRef.current = config;
  }, [config]);

  // The refreshConfig functionality is no longer needed as we receive updates via SSE
  // The daemon pushes updates automatically
  const refreshConfig = useCallback(() => {
    console.log('refreshConfig called, but manual refresh is not supported with SSE. Configuration updates are pushed automatically.');
  }, []);

  const updateConfig = useCallback((newSettings: { channels: number; sample_rate: number; powerline_filter_hz: number | null; gain?: number }) => {
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

    // TEMP: force two-chip board until SSE carries driver.type (toggle with localStorage flag)
    const forceTwoChip =
      typeof window !== 'undefined' &&
      window.localStorage &&
      window.localStorage.getItem('eeg_force_two_chip') === '1';

    // Use latest known driver.type from SSE to avoid timing races where board_driver is still 'default'
    // Try to infer driver from the freshest sources in priority order
    const inferredDriver = forceTwoChip
      ? 'ElataV2'
      : (
          lastDriverTypeRef.current ||
          // fallback to pipeline state
          (() => {
            try {
              const cfg = (pipelineState && (pipelineState as any).config) || null;
              const stages = cfg?.stages;
              if (Array.isArray(stages)) {
                const eegStage = stages.find((s: any) => (s?.type === 'eeg_source' || s?.stage_type === 'eeg_source' || s?.name === 'eeg_source')) || stages[0];
                const params = eegStage?.params || eegStage?.parameters;
                return params?.driver?.type;
              }
            } catch {}
            return undefined;
          })() ||
          currentConfig.board_driver ||
          'default'
        );

    if (!forceTwoChip && (!inferredDriver || inferredDriver === 'default')) {
      console.warn('EEG driver type unknown; aborting Apply. Wait for SSE to populate driver.type or set override with localStorage.setItem("eeg_force_two_chip","1") and refresh.');
      return;
    }

    const effectiveConfig = inferredDriver === 'ElataV2'
      ? { ...currentConfig, board_driver: 'ElataV2', chips: currentConfig.chips?.length === 2 ? currentConfig.chips : [{ channels: [], spi_bus: 0, cs_pin: 0 }, { channels: [], spi_bus: 0, cs_pin: 0 }] }
      : currentConfig;

    const driver = buildDriverPayload(effectiveConfig, {
      channels: newSettings.channels,
      sample_rate: newSettings.sample_rate,
      gain: newSettings.gain,
    });

    const command = {
      target_stage: 'eeg_source',
      parameters: { driver },
    };

    console.log("Sending SetParameter command with payload:", JSON.stringify(command, null, 2));
    sendCommand('SetParameter', command)
      .then(() => {
        // Also align the GUI filter channel count to avoid mismatched interleave
        try {
          const cfg = (pipelineState && (pipelineState as any).config) || null;
          const stages = cfg?.stages;
          if (Array.isArray(stages)) {
            const filterStage = stages.find((s: any) => (s?.type === 'gui_filter' || s?.name === 'filter'));
            const params = filterStage?.params || {};
            const filterParams = {
              channels: Math.max(1, Number(newSettings.channels) || 1),
              high_pass: params.high_pass ?? 1.0,
              low_pass: params.low_pass ?? 40.0,
              notch: params.notch ?? null,
              output: params.output ?? 'filtered_data',
            };
            const filterCmd = { target_stage: filterStage?.name || 'filter', parameters: filterParams };
            console.log('Sending filter reconfigure with payload:', JSON.stringify(filterCmd, null, 2));
            return sendCommand('SetParameter', filterCmd);
          }
        } catch (e) {
          console.warn('Could not reconfigure gui_filter channels:', e);
        }
      })
      .catch(err => console.error('Driver reconfigure failed:', err));
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
