'use client';
import React from 'react'; // Added to resolve React.Fragment error

import { useRef, useState, useEffect, useLayoutEffect } from 'react';
import { useEegConfig } from './EegConfig';
// EegConfigDisplay is removed as we are inlining and modifying its logic.
// EegChannelConfig is also not used in the settings panel.
import { EegRenderer } from './EegRenderer';
import EegRecordingControls from './EegRecordingControls'; // Import the actual controls
// import { ScrollingBuffer } from '../utils/ScrollingBuffer'; // Removed - Unused and file doesn't exist
import { GRAPH_HEIGHT, WINDOW_DURATION, TIME_TICKS, DISPLAY_FPS } from '../utils/eegConstants';
import { useCommandWebSocket } from '../context/CommandWebSocketContext';
/* eslint-disable @typescript-eslint/ban-ts-comment */
// @ts-ignore: WebglStep might be missing from types but exists at runtime
import { WebglStep, ColorRGBA } from 'webgl-plot';
import { getChannelColor } from '../utils/colorUtils';
import BrainWavesDisplay from '../../../plugins/brain-waves-display/ui/BrainWavesDisplay';
import { CircularGraphWrapper } from './CircularGraphWrapper';
import { useEegData } from '../context/EegDataContext';
import { useDataBuffer } from '../hooks/useDataBuffer'; // Import the new hook

export default function EegMonitorWebGL() {
  const containerRef = useRef<HTMLDivElement>(null);
  const dataRef = useRef<any[]>([]); // Holds the WebGL line objects
  const canvasRef = useRef<HTMLCanvasElement>(null); // Re-added for EegRenderer
  const latestTimestampRef = useRef<number>(0); // Re-added for EegRenderer
  const debugInfoRef = useRef({ // Re-added for EegRenderer
    lastPacketTime: 0,
    packetsReceived: 0,
    samplesProcessed: 0,
  });
  // Removed canvasDimensions state, EegRenderer handles this

  type DataView = 'signalGraph' | 'appletBrainWaves' | 'circularGraph'; // fftGraph removed
  type ActiveView = DataView | 'settings';
 
  const [activeView, setActiveView] = useState<ActiveView>('signalGraph');
  const activeViewRef = useRef(activeView);
  useEffect(() => {
    activeViewRef.current = activeView;
  }, [activeView]);
  const [lastActiveDataView, setLastActiveDataView] = useState<DataView>('signalGraph'); // To store the last non-settings view
  const [linesReady, setLinesReady] = useState(false); // State to track line readiness
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 }); // State for container dimensions
  const [dataVersion, setDataVersion] = useState(0); // Version counter for dataRef updates
  const circularGraphBuffer = useDataBuffer<any>(); // Use the new buffer hook
  const signalGraphBuffer = useDataBuffer<any>(); // Buffer for the main signal graph
  const circularGraphLastProcessedLengthRef = useRef<number>(0);
  
  const [configWebSocket, setConfigWebSocket] = useState<WebSocket | null>(null); // Restored
  const [isConfigWsOpen, setIsConfigWsOpen] = useState(false); // Restored
  const [configUpdateStatus, setConfigUpdateStatus] = useState<string | null>(null); // Kept for user feedback
  const [uiVoltageScaleFactor, setUiVoltageScaleFactor] = useState<number>(0.25); // Added for UI Voltage Scaling
  const settingsScrollRef = useRef<HTMLDivElement>(null); // Ref for settings scroll container
  const [canScrollSettings, setCanScrollSettings] = useState(false); // True if settings panel has enough content to scroll
  const [isAtSettingsBottom, setIsAtSettingsBottom] = useState(false); // True if scrolled to the bottom of settings

  // Get all data and config from the new central context
  const { dataVersion: eegDataVersion, getRawSamples, subscribeRaw, fftData, config, dataStatus, subscribe, unsubscribe } = useEegData();
  const { dataReceived, driverError, wsStatus } = dataStatus;
  const { status: configStatus, refreshConfig } = useEegConfig(); // Keep for settings UI

  // State for UI selections, initialized from config when available
  const [selectedChannelCount, setSelectedChannelCount] = useState<string | undefined>(undefined);
  const [selectedSampleRate, setSelectedSampleRate] = useState<string | undefined>(undefined);
  const [selectedPowerlineFilter, setSelectedPowerlineFilter] = useState<string | undefined>(undefined);

  useEffect(() => {
    if (config) {
      if (config.channels?.length !== undefined) {
        setSelectedChannelCount(String(config.channels.length));
      }
      if (config.sample_rate !== undefined) {
        setSelectedSampleRate(String(config.sample_rate));
      }
      if (config.powerline_filter_hz !== undefined) {
        setSelectedPowerlineFilter(config.powerline_filter_hz === null ? 'off' : String(config.powerline_filter_hz));
      }
    }
  }, [config]);

  // Effect to manage data subscriptions based on the active view
  useEffect(() => {
    const isSignalGraphView = activeView === 'signalGraph';
    const isCircularGraphView = activeView === 'circularGraph';
    const isFftView = activeView === 'appletBrainWaves';

    let unsubSignal: (() => void) | null = null;
    if (isSignalGraphView) {
      console.log('[EegMonitor] Subscribing to raw data for Signal Graph.');
      unsubSignal = subscribeRaw((newSampleChunks) => {
        if (newSampleChunks.length > 0) {
          signalGraphBuffer.addData(newSampleChunks);
        }
      });
    }

    let unsubCircular: (() => void) | null = null;
    if (isCircularGraphView) {
      console.log('[EegMonitor] Subscribing to raw data for Circular Graph.');
      unsubCircular = subscribeRaw((newSampleChunks) => {
        if (newSampleChunks.length > 0) {
          circularGraphBuffer.addData(newSampleChunks);
        }
      });
    }
    
    if (isFftView) {
      console.log('[EegMonitor] Subscribing to FftPacket');
      subscribe(['FftPacket']);
    }

    return () => {
      if (unsubSignal) {
        console.log('[EegMonitor] Unsubscribing from raw data for Signal Graph.');
        unsubSignal();
      }
      if (unsubCircular) {
        console.log('[EegMonitor] Unsubscribing from raw data for Circular Graph.');
        unsubCircular();
      }
      if (isFftView) {
        console.log('[EegMonitor] Unsubscribing from FftPacket');
        unsubscribe(['FftPacket']);
      }
    };
  }, [activeView, subscribe, unsubscribe, subscribeRaw, signalGraphBuffer, circularGraphBuffer]);


  const { sendPowerlineFilterCommand } = useCommandWebSocket(); // Keep for potential direct use if needed

  const handleUpdateConfig = () => {
    if (!configWebSocket || configWebSocket.readyState !== WebSocket.OPEN) {
      console.error('Config WebSocket (EegMonitor) not connected or not ready.');
      setConfigUpdateStatus('Error: Config service not connected. Cannot send update.');
      return;
    }

    if (recordingStatus.startsWith('Currently recording')) {
        setConfigUpdateStatus('Cannot change configuration during recording.');
        return;
    }

    const newConfigPayload: { channels?: number[]; sample_rate?: number; powerline_filter_hz?: number | null } = {};
    let changesMade = false;

    if (selectedChannelCount !== undefined) {
      const numChannels = parseInt(selectedChannelCount, 10);
      if (!isNaN(numChannels) && numChannels >= 0 && numChannels <= 8) { // Max 8 channels for ADS1299
        const currentChannels = config?.channels || [];
        const newChannelsArray = Array.from({ length: numChannels }, (_, i) => i);
        // Compare arrays properly
        if (JSON.stringify(currentChannels) !== JSON.stringify(newChannelsArray)) {
            newConfigPayload.channels = newChannelsArray;
            changesMade = true;
        }
      } else {
        setConfigUpdateStatus('Invalid number of channels selected.');
        return;
      }
    }

    if (selectedSampleRate !== undefined) {
      const rate = parseInt(selectedSampleRate, 10);
      const validRates = [250, 500, 1000, 2000]; // Example valid rates
      if (!isNaN(rate) && validRates.includes(rate)) {
        if (config?.sample_rate !== rate) {
            newConfigPayload.sample_rate = rate;
            changesMade = true;
        }
      } else {
        setConfigUpdateStatus(`Invalid sample rate: ${rate}. Valid: ${validRates.join(', ')}`);
        return;
      }
    }
    
    if (selectedPowerlineFilter !== undefined) {
      let filterValue: number | null = null;
      if (selectedPowerlineFilter === 'off') {
        filterValue = null;
      } else {
        const parsedFilter = parseInt(selectedPowerlineFilter, 10);
        if (!isNaN(parsedFilter) && (parsedFilter === 50 || parsedFilter === 60)) {
          filterValue = parsedFilter;
        } else {
          setConfigUpdateStatus(`Invalid powerline filter value: ${selectedPowerlineFilter}`);
          return;
        }
      }
      if (config?.powerline_filter_hz !== filterValue) {
        newConfigPayload.powerline_filter_hz = filterValue;
        changesMade = true;
      }
    }
    
    if (!changesMade) {
      setConfigUpdateStatus('No changes selected to update.');
      console.log('No changes to send for config update.');
      return;
    }
    
    console.log('Sending config update via EegMonitor /config WebSocket:', newConfigPayload);
    setConfigUpdateStatus('Sending configuration update...');
    configWebSocket.send(JSON.stringify(newConfigPayload));
  };

  // Use the command WebSocket context
  const {
    wsConnected,
    startRecording,
    stopRecording,
    // sendPowerlineFilterCommand is now available here but used in handleUpdateConfig
    recordingStatus,
    recordingFilePath,
  } = useCommandWebSocket();

  // Update canvas dimensions based on container size
 
  // Effect to update lastActiveDataView when activeView changes (and is not settings)
  useEffect(() => {
    if (activeView !== 'settings') {
      setLastActiveDataView(activeView as DataView);
    }
  }, [activeView]);

  // Effect to setup ResizeObserver to monitor container size
  useLayoutEffect(() => {
    const target = containerRef.current;
    // Only run if the target element exists and we are in a view that shows the graph
    if (!target || (activeView !== 'signalGraph' && activeView !== 'circularGraph')) {
      return;
    }

    const resizeObserver = new ResizeObserver(entries => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        // Update state with the new dimensions
        setContainerSize({ width, height });
      }
    });

    resizeObserver.observe(target);

    // Set initial size
    setContainerSize({ width: target.offsetWidth, height: target.offsetHeight });

    // Cleanup observer on unmount or when activeView changes
    return () => {
      resizeObserver.disconnect();
    };
  }, [activeView]); // Rerun when the active view changes

  // Effect to create/update WebGL lines when config or container size changes
  // Constants for scaling
  const MICROVOLT_CONVERSION_FACTOR = 1e6; // V to uV
  const REFERENCE_UV_RANGE = 100.0; // The peak-to-peak microvolt range we want to fit in the channel's vertical space by default
  const UI_SCALE_FACTORS = [0.125, 0.25, 0.5, 1, 2, 4, 8]; // Added UI Scale Factors

  useEffect(() => {
    const numChannels = config?.channels?.length || 0;
    const sampleRate = config?.sample_rate;
    const MIN_VALID_WIDTH = 50;

    // Create or update lines only if necessary
    if (config && sampleRate && numChannels > 0 && containerSize.width > MIN_VALID_WIDTH) {
      const initialNumPoints = Math.ceil(sampleRate * (WINDOW_DURATION / 1000));
      const ySpacing = 2.0 / numChannels;

      // If number of lines has changed, recreate them
      if (dataRef.current.length !== numChannels) {
        console.log(`[EegMonitor] Creating ${numChannels} WebGL lines with ${initialNumPoints} points each.`);
        const lines: WebglStep[] = [];
        for (let i = 0; i < numChannels; i++) {
          const line = new WebglStep(new ColorRGBA(1, 1, 1, 1), initialNumPoints);
          line.lineSpaceX(-1, 2 / initialNumPoints);
          lines.push(line);
        }
        dataRef.current = lines;
        setLinesReady(true);
      }

      // Always update scaling and positioning as it can change dynamically
      dataRef.current.forEach((line, i) => {
        line.lineWidth = 1;
        const calculatedScaleY = ((ySpacing * MICROVOLT_CONVERSION_FACTOR) / REFERENCE_UV_RANGE) * uiVoltageScaleFactor;
        line.scaleY = calculatedScaleY;
        line.offsetY = 1 - (i + 0.5) * ySpacing;
      });

      setDataVersion(v => v + 1);
    } else {
      // If no channels or invalid config, clear the lines
      if (dataRef.current.length > 0) {
        dataRef.current = [];
        setLinesReady(false);
        setDataVersion(v => v + 1);
      }
    }
  }, [config?.channels?.length, config?.sample_rate, containerSize.width, uiVoltageScaleFactor]);

  // This entire synchronous data processing block is now handled by the EegRenderer's async loop.
  // It is safe to remove.

  // This useEffect is now handled by the consolidated subscription logic above.

  // Reset processed index when WebGL lines are recreated or view changes
  useEffect(() => {
    // lastProcessedLengthRef is no longer needed
    console.log('[EegMonitor] Reset processed data index');
  }, [linesReady, activeView]);
  
  // Use the FPS from config with no fallback
  const displayFps = config?.fps || 0;

  const getViewName = (view: DataView | 'settings'): string => {
    switch (view) {
        case 'signalGraph': return 'Signal Graph';
        case 'circularGraph': return 'Circular Graph';
        // case 'fftGraph': return 'FFT Graph'; // Removed
        case 'appletBrainWaves': return 'Brain Waves (FFT)'; // Updated name
        case 'settings': return 'Settings';
        default: return '';
    }
  };

  // Handler for cycling between Signal Graph, Circular Graph, and FFT Applet
  const handleToggleSignalFftView = () => {
    if (activeView === 'signalGraph') {
      setActiveView('circularGraph');
    } else if (activeView === 'circularGraph') {
      setActiveView('appletBrainWaves');
    } else if (activeView === 'appletBrainWaves') {
      setActiveView('signalGraph');
    }
    // If currently in settings, this function shouldn't be called, but handle gracefully
    // by defaulting to signal graph
    else if (activeView === 'settings') {
      setActiveView('signalGraph');
    }
  };
 
  // Handler for the "Settings" / "Back to [View]" button
  const handleToggleSettingsView = () => {
    if (activeView !== 'settings') {
        // lastActiveDataView is already updated by an effect
        setActiveView('settings');
    } else {
        setActiveView(lastActiveDataView);
        // If switching from settings to a graph view, ensure container size is reset
        if (lastActiveDataView === 'signalGraph' || lastActiveDataView === 'circularGraph') { // fftGraph condition removed
            console.log("[EegMonitor handleToggleSettingsView] Switching from settings to graph view. Resetting containerSize.");
            setContainerSize({ width: 0, height: 0 });
        }
    }
  };
  

  // Effect for settings panel scroll detection
  useEffect(() => {
    const scrollElement = settingsScrollRef.current;

    const checkScroll = () => {
      if (scrollElement) {
        // Check if there's enough content to scroll (scrollbar is visible)
        const hasScrollbar = scrollElement.scrollHeight > scrollElement.clientHeight;
        // Check if scrolled to the bottom (with a small buffer for precision)
        const atBottom = scrollElement.scrollTop + scrollElement.clientHeight >= scrollElement.scrollHeight - 5;
        
        setCanScrollSettings(hasScrollbar && !atBottom); // Only show arrow if scrollable and not at bottom
        setIsAtSettingsBottom(hasScrollbar && atBottom); // Show end marker if scrollable and at bottom
        
        // If no scrollbar, effectively at bottom for "end" marker logic if content fits perfectly
        if (!hasScrollbar) {
            setIsAtSettingsBottom(true);
        }

      } else {
        setCanScrollSettings(false);
        setIsAtSettingsBottom(false);
      }
    };

    if (activeView === 'settings' && scrollElement) {
      // Initial check
      // Use a timeout to allow content to render fully before checking scroll height
      const timerId = setTimeout(checkScroll, 100);


      // Listen for scroll events
      scrollElement.addEventListener('scroll', checkScroll);
      // Also check on resize of the window or content (e.g. config load)
      const resizeObserver = new ResizeObserver(checkScroll);
      resizeObserver.observe(scrollElement);
      // Observe children too, as their size changes can affect scrollHeight
      Array.from(scrollElement.children).forEach(child => resizeObserver.observe(child));


      return () => {
        clearTimeout(timerId);
        scrollElement.removeEventListener('scroll', checkScroll);
        resizeObserver.disconnect(); // Disconnects from all observed elements
      };
    } else {
      // Reset when settings are hidden
      setCanScrollSettings(false);
      setIsAtSettingsBottom(false);
    }
  }, [activeView, config, uiVoltageScaleFactor]); // Re-check when settings are shown, config (content height might change), or other UI elements change


  return (
    <div className="h-screen w-screen bg-gray-900 flex flex-col">
      {/* Header with controls */}
      <div className="flex justify-between items-center p-2 bg-gray-800 border-b border-gray-700">
        <div className="flex items-center">
          <h1 className="text-xl font-bold text-white mr-4">EEG Monitor</h1>
          <div className="flex items-center text-white">
            <span>Status:</span>
            <span className={`inline-block w-3 h-3 rounded-full mx-2 ${dataReceived ? 'bg-green-500' : 'bg-gray-500'}`}></span>
            <span>{dataReceived ? 'receiving data' : 'no data'}</span>
          </div>
          <div className="ml-4 text-xs text-gray-300">
            <span>WS: {wsStatus}</span>
          </div>
        </div>
        <div className="flex items-baseline space-x-2">
          {/* Use the EegRecordingControls component */}
          <EegRecordingControls />
          
          {/* Recordings button */}
          <a
            href="/recordings"
            className="px-4 py-1 rounded-md bg-purple-600 hover:bg-purple-700 text-white flex items-center"
          >
            <svg xmlns="http://www.w3.org/2000/svg" className="h-4 w-4 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
            </svg>
            Recordings
          </a>
          
          {/* Signal / Circular / FFT Graph toggle button */}
          <button
            onClick={handleToggleSignalFftView}
            className="px-4 py-1 rounded-md bg-teal-600 hover:bg-teal-700 text-white"
            disabled={activeView === 'settings'}
          >
            {activeView === 'signalGraph' ? 'Show Circular' :
             activeView === 'circularGraph' ? 'Show FFT' : 'Show Signal'}
          </button>
 
          {/* Settings button */}
          <button
            onClick={handleToggleSettingsView}
            className="px-4 py-1 rounded-md bg-blue-600 hover:bg-blue-700 text-white"
          >
            {activeView === 'settings'
              ? `Back to ${getViewName(lastActiveDataView)}`
              : 'Settings'}
          </button>
        </div>
      </div>
      
      {/* Recording status indicator */}
      {recordingStatus.startsWith('Currently recording') && (
        <div className="bg-red-900 text-white px-2 py-1 text-sm flex justify-between">
          <div className="flex items-center">
            <span className="inline-block w-2 h-2 rounded-full bg-red-500 animate-pulse mr-2"></span>
            {recordingStatus}
          </div>
          {recordingFilePath && (
            <div className="text-gray-300 truncate">
              File: {recordingFilePath}
            </div>
          )}
        </div>
      )}
      
      {/* Driver error display */}
      {driverError && (
        <div className="bg-yellow-800 text-white px-2 py-1 text-sm flex items-center">
          <svg xmlns="http://www.w3.org/2000/svg" className="h-5 w-5 mr-2 text-yellow-300" viewBox="0 0 20 20" fill="currentColor">
            <path fillRule="evenodd" d="M8.257 3.099c.765-1.36 2.722-1.36 3.486 0l5.58 9.92c.75 1.334-.213 2.98-1.742 2.98H4.42c-1.53 0-2.493-1.646-1.743-2.98l5.58-9.92zM11 13a1 1 0 11-2 0 1 1 0 012 0zm-1-8a1 1 0 00-1 1v3a1 1 0 002 0V6a1 1 0 00-1-1z" clipRule="evenodd" />
          </svg>
          <span>Driver Error: {driverError}</span>
        </div>
      )}
      
      {/* Main content area */}
      <div className="flex-grow overflow-hidden">
        {activeView === 'settings' ? (
          <div ref={settingsScrollRef} className="relative h-full p-4 overflow-auto"> {/* Settings Panel Component (Existing UI) */}
            {/* Scroll Down Indicator */}
            {canScrollSettings && (
              <div className="absolute top-1/2 right-2 transform -translate-y-1/2 z-10 text-gray-400 animate-bounce pointer-events-none">
                <span className="block text-2xl">↓</span>
                <span className="block text-2xl -mt-4">↓</span>
                <span className="block text-2xl -mt-4">↓</span>
              </div>
            )}
            <div>
              <h2 className="text-xl font-semibold mb-4 text-gray-200 border-b border-gray-700 pb-2">EEG Settings</h2>
              <div className="mb-2 mt-4">
                <span className="font-medium text-gray-400">Connection Status:</span>
                <span className={`ml-2 px-2 py-0.5 rounded-full text-sm ${
                  configStatus === 'Connected' ? 'bg-green-700 text-green-100' :
                  configStatus === 'Connecting...' || configStatus === 'Initializing...' ? 'bg-yellow-700 text-yellow-100' :
                  'bg-red-700 text-red-100'
                }`}>
                  {configStatus}
                </span>
              </div>

              {config ? (
                <div className="grid grid-cols-1 md:grid-cols-2 gap-x-8 gap-y-4 mt-4 text-gray-300">
                  {/* Sample Rate */}
                  <div className="md:col-span-2 flex items-center space-x-4">
                    <label htmlFor="sampleRate" className="block text-sm font-medium text-gray-400 w-48">Sample Rate (SPS):</label>
                    <select
                      id="sampleRate"
                      name="sampleRate"
                      value={selectedSampleRate || ''}
                      onChange={(e) => setSelectedSampleRate(e.target.value)}
                      className="block w-40 pl-3 pr-10 py-2 text-base bg-gray-700 border-gray-600 focus:outline-none focus:ring-blue-500 focus:border-blue-500 sm:text-sm rounded-md text-white"
                    >
                      <option value="250">250 SPS</option>
                      <option value="500">500 SPS</option>
                    </select>
                    <span className="text-sm text-gray-500">(Currently: {config.sample_rate} Hz)</span>
                  </div>

                  {/* Number of Active Channels */}
                  <div className="md:col-span-2 flex items-center space-x-4">
                    <label htmlFor="channelCount" className="block text-sm font-medium text-gray-400 w-48">Number of Active Channels:</label>
                    <select
                      id="channelCount"
                      name="channelCount"
                      value={selectedChannelCount || ''}
                      onChange={(e) => setSelectedChannelCount(e.target.value)}
                      className="block w-40 pl-3 pr-10 py-2 text-base bg-gray-700 border-gray-600 focus:outline-none focus:ring-blue-500 focus:border-blue-500 sm:text-sm rounded-md text-white"
                    >
                      {[...Array(9).keys()].map(i => (
                        <option key={i} value={String(i)}>{i} Channels</option>
                      ))}
                    </select>
                    <span className="text-sm text-gray-500">(Currently: {config.channels?.length} channels - [{config.channels?.join(', ')}])</span>
                  </div>
                  
                  {/* Powerline Filter */}
                  <div className="md:col-span-2 flex items-center space-x-4">
                    <label htmlFor="powerlineFilter" className="block text-sm font-medium text-gray-400 w-48">Powerline Filter:</label>
                    <select
                      id="powerlineFilter"
                      name="powerlineFilter"
                      value={selectedPowerlineFilter || ''}
                      onChange={(e) => setSelectedPowerlineFilter(e.target.value)}
                      className="block w-40 pl-3 pr-10 py-2 text-base bg-gray-700 border-gray-600 focus:outline-none focus:ring-blue-500 focus:border-blue-500 sm:text-sm rounded-md text-white"
                    >
                      <option value="off">Off</option>
                      <option value="50">50 Hz</option>
                      <option value="60">60 Hz</option>
                    </select>
                    <span className="text-sm text-gray-500">
                      (Currently: {config.powerline_filter_hz === null ? 'Off' :
                                  config.powerline_filter_hz ? `${config.powerline_filter_hz} Hz` : 'Not set'})
                    </span>
                  </div>
                  
                  {/* Other Config Items - Display Only */}
                  <div className="text-sm"><span className="font-medium text-gray-400">Gain:</span> {config.gain}</div>
                  <div className="text-sm"><span className="font-medium text-gray-400">Board Driver:</span> {config.board_driver}</div>

                  {/* Update Config Button */}
                  <div className="md:col-span-2 mt-6">
                    <button
                      onClick={handleUpdateConfig}
                      disabled={!isConfigWsOpen || recordingStatus.startsWith('Currently recording')}
                      className={`px-6 py-2 rounded-md text-white focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-opacity-50 ${
                        (!isConfigWsOpen || recordingStatus.startsWith('Currently recording'))
                          ? 'bg-gray-500 cursor-not-allowed'
                          : 'bg-blue-600 hover:bg-blue-700'
                      }`}
                    >
                      Update Config
                    </button>
                    {configUpdateStatus && (
                      <p className={`mt-2 text-xs ${configUpdateStatus.includes('Error') || configUpdateStatus.includes('error') || configUpdateStatus.includes('Invalid') || configUpdateStatus.includes('timed out') ? 'text-red-400' : 'text-gray-400'}`}>
                        {configUpdateStatus}
                      </p>
                    )}
                    <p className="mt-2 text-xs text-gray-500">Note: Updating configuration may restart the data stream if successful.</p>
                  </div>
                </div>
              ) : (
                <p className="text-gray-500 mt-4">Waiting for configuration data...</p>
              )}
            </div>

            {/* UI Settings Section */}
            <div className="mt-8"> {/* Add some margin-top */}
              <h2 className="text-xl font-semibold mb-4 text-gray-200 border-b border-gray-700 pb-2">UI Settings</h2>
              <div className="grid grid-cols-1 md:grid-cols-2 gap-x-8 gap-y-4 mt-4 text-gray-300">
                {/* Moved Batch Size and Effective FPS */}
                {config && (
                  <>
                    <div className="text-sm"><span className="font-medium text-gray-400">Batch Size:</span> {config.batch_size} samples</div>
                    <div className="text-sm"><span className="font-medium text-gray-400">Effective FPS:</span> {config.fps?.toFixed(2)} frames/sec</div>
                  </>
                )}

                {/* UI Voltage Scaling Control */}
                <div className="md:col-span-2 flex items-center space-x-3">
                  <label className="block text-sm font-medium text-gray-400 w-48">UI Voltage Scaling:</label>
                  <button
                    onClick={() => {
                      const currentIndex = UI_SCALE_FACTORS.indexOf(uiVoltageScaleFactor);
                      if (currentIndex > 0) {
                        setUiVoltageScaleFactor(UI_SCALE_FACTORS[currentIndex - 1]);
                      }
                    }}
                    disabled={UI_SCALE_FACTORS.indexOf(uiVoltageScaleFactor) === 0}
                    className="px-3 py-1 rounded-md bg-gray-600 hover:bg-gray-500 text-white disabled:opacity-50"
                  >
                    Zoom Out
                  </button>
                  <span className="text-sm w-16 text-center">{uiVoltageScaleFactor}x</span>
                  <button
                    onClick={() => {
                      const currentIndex = UI_SCALE_FACTORS.indexOf(uiVoltageScaleFactor);
                      if (currentIndex < UI_SCALE_FACTORS.length - 1) {
                        setUiVoltageScaleFactor(UI_SCALE_FACTORS[currentIndex + 1]);
                      }
                    }}
                    disabled={UI_SCALE_FACTORS.indexOf(uiVoltageScaleFactor) === UI_SCALE_FACTORS.length - 1}
                    className="px-3 py-1 rounded-md bg-gray-600 hover:bg-gray-500 text-white disabled:opacity-50"
                  >
                    Zoom In
                  </button>
                </div>
              </div>
            </div>
            {/* EegChannelConfig (Per-Channel Settings) section remains removed */}
          </div>
        ) : activeView === 'signalGraph' ? (
          <div className="h-full p-4"> {/* This outer div might be slightly different from original, ensure it matches structure */}
            {/* Show loading state if config is not ready */}
            {!config || !config.channels || !Array.isArray(config.channels) || config.channels.length === 0 ? (
              <div className="h-full flex items-center justify-center">
                <div className="text-center">
                  <div className="text-gray-400 text-lg mb-2">Loading EEG Configuration...</div>
                  <div className="text-gray-500 text-sm">
                    Status: {configStatus}
                  </div>
                  {config && (
                    <div className="text-gray-600 text-xs mt-2">
                      Config loaded but channels: {config.channels ? `${config.channels.length} channels` : 'not available'}
                    </div>
                  )}
                </div>
              </div>
            ) : (
              <>
                {/* Time markers */}
                <div className="relative h-full"> {/* This div is important for positioning */}
                  <div className="absolute w-full flex justify-between px-2 -top-6 text-gray-400 text-sm">
                    {[...TIME_TICKS].reverse().map(time => (
                      <div key={time}>{time}s</div>
                    ))}
                  </div>
                  
                  <div className="relative h-full min-h-[300px]" ref={containerRef}> {/* EegRenderer and related elements - added min-height */}
                {/* Channel labels */}
                <div className="absolute -left-8 h-full flex flex-col justify-between">
                  {config?.channels && config.channels.length > 0 ? (
                    config.channels.map((chIdx: number) => (
                      <div key={chIdx} className="text-gray-400 font-medium">Ch{chIdx}</div>
                    ))
                  ) : (
                    <div className="text-gray-400 font-medium">No channel info</div>
                  )}
                </div>
                
                <canvas
                  ref={canvasRef}
                  className="w-full h-full border border-gray-700 rounded-lg"
                />
                
                <EegRenderer
                  canvasRef={canvasRef}
                  dataRef={dataRef}
                  config={config}
                  latestTimestampRef={latestTimestampRef}
                  debugInfoRef={debugInfoRef}
                  containerWidth={containerSize.width}
                  containerHeight={containerSize.height}
                  linesReady={linesReady}
                  dataBuffer={signalGraphBuffer} // Pass the new buffer
                  targetFps={displayFps}
                />
                </div>
              </div>
            </>
            )}
          </div>
        ) : activeView === 'circularGraph' ? (
          // Circular Graph View
          <div className="relative h-full p-4" ref={containerRef}>
            {config && containerSize.width > 0 && containerSize.height > 0 ? (
              <CircularGraphWrapper
                config={config}
                containerWidth={containerSize.width}
                containerHeight={containerSize.height}
                dataBuffer={circularGraphBuffer}
                targetFps={60}
                displaySeconds={10}
              />
            ) : (
              <div className="flex items-center justify-center h-full text-white">
                <p>Loading Circular Graph or waiting for configuration/size...</p>
              </div>
            )}
          </div>
        ) : activeView === 'appletBrainWaves' ? (
          // Brain Waves Applet View
          <div className="relative h-full p-2" ref={containerRef}> {/* Added padding for aesthetics */}
            {config && containerSize.width > 0 && containerSize.height > 0 ? (
              <BrainWavesDisplay
                containerWidth={containerSize.width}
                containerHeight={containerSize.height} // Use full container height
              />
            ) : (
              <div className="flex items-center justify-center h-full text-white">
                <p>Loading Brain Waves Applet or waiting for configuration/size...</p>
              </div>
            )}
            {/* Optional: Keep a minimal status bar if desired, or let the applet handle its own status */}
            {/* <EegStatusBar
              status={status} // This status is for the main /eeg/data WebSocket
              fps={displayFps}
              config={config}
              latestTimestampRef={latestTimestampRef} // This is for the main /eeg/data WebSocket
              debugInfoRef={debugInfoRef}
            /> */}
          </div>
       ) : null}
     </div>
   </div>
  );
}