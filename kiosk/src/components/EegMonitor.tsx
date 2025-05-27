'use client';

import { useRef, useState, useEffect, useCallback, useLayoutEffect } from 'react';
import { useEegConfig } from './EegConfig';
// EegConfigDisplay is removed as we are inlining and modifying its logic.
// EegChannelConfig is also not used in the settings panel.
import { EegStatusBar } from './EegStatusBar';
import { useEegDataHandler } from './EegDataHandler';
import { EegRenderer } from './EegRenderer';
import { FftRenderer } from './FftRenderer'; // Import the FftRenderer
import EegRecordingControls from './EegRecordingControls'; // Import the actual controls
// import { ScrollingBuffer } from '../utils/ScrollingBuffer'; // Removed - Unused and file doesn't exist
import { GRAPH_HEIGHT, WINDOW_DURATION, TIME_TICKS } from '../utils/eegConstants';
import { useCommandWebSocket } from '../context/CommandWebSocketContext';
/* eslint-disable @typescript-eslint/ban-ts-comment */
// @ts-ignore: WebglStep might be missing from types but exists at runtime
import { WebglStep, ColorRGBA } from 'webgl-plot';
import { getChannelColor } from '../utils/colorUtils';

export default function EegMonitorWebGL() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const windowSizeRef = useRef<number>(500); // Default, will be updated based on config
  const dataRef = useRef<any[]>([]); // Re-added definition
  const [dataReceived, setDataReceived] = useState(false);
  const [driverError, setDriverError] = useState<string | null>(null);
  // Ref to hold the last update timestamp for each channel's data chunk
  const channelTimestampRef = useRef<number[]>([]);
  // Ref to hold the single latest timestamp of any received data packet
  const latestTimestampRef = useRef<number>(performance.now());
  // Removed canvasDimensions state, EegRenderer handles this
  const [activeView, setActiveView] = useState<'signalGraph' | 'fftGraph' | 'settings'>('signalGraph');
  const [lastGraphView, setLastGraphView] = useState<'signalGraph' | 'fftGraph'>('signalGraph');
  const [linesReady, setLinesReady] = useState(false); // State to track line readiness
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 }); // State for container dimensions
  const [dataVersion, setDataVersion] = useState(0); // Version counter for dataRef updates
  const fftDataRef = useRef<Record<number, number[]>>({}); // Ref to store latest FFT data for all channels
  const [fftDataVersion, setFftDataVersion] = useState(0); // Version counter for fftDataRef updates
  const [configWebSocket, setConfigWebSocket] = useState<WebSocket | null>(null);
  const [isConfigWsOpen, setIsConfigWsOpen] = useState(false); // Added to track WebSocket state
  const [configUpdateStatus, setConfigUpdateStatus] = useState<string | null>(null);
  const [uiVoltageScaleFactor, setUiVoltageScaleFactor] = useState<number>(0.5); // Added for UI Voltage Scaling
  const settingsScrollRef = useRef<HTMLDivElement>(null); // Ref for settings scroll container
  const [canScrollSettings, setCanScrollSettings] = useState(false); // True if settings panel has enough content to scroll
  const [isAtSettingsBottom, setIsAtSettingsBottom] = useState(false); // True if scrolled to the bottom of settings

  // Get configuration from context
  const { config, status: configStatus } = useEegConfig(); // refreshConfig removed

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

  // Effect to manage the /config WebSocket connection
  useEffect(() => {
    if (activeView === 'settings') {
      console.log('Attempting to connect to /config WebSocket');
      const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
      const newConfigWs = new WebSocket(`ws://${wsHost}:8080/config`);
      setConfigWebSocket(newConfigWs);
      setIsConfigWsOpen(false); // Initially false until onopen
      setConfigUpdateStatus('Connecting to config service...');

      const connectionTimeout = setTimeout(() => {
        if (newConfigWs.readyState !== WebSocket.OPEN) {
          console.warn('/config WebSocket connection timed out.');
          setConfigUpdateStatus('Connection to config service timed out. Please check daemon.');
          setIsConfigWsOpen(false); // Ensure this is false
          // Optionally, try to close the WebSocket if it's in a connecting state
          if (newConfigWs.readyState === WebSocket.CONNECTING) {
            newConfigWs.close();
          }
        }
      }, 5000); // 5-second timeout

      newConfigWs.onopen = () => {
        clearTimeout(connectionTimeout);
        console.log('/config WebSocket connected');
        setIsConfigWsOpen(true);
        setConfigUpdateStatus('Connected to config service. Ready to send updates.');
      };

      newConfigWs.onmessage = (event) => {
        console.log('/config WebSocket message:', event.data);
        try {
          const response = JSON.parse(event.data as string);
          // Check if it's a CommandResponse (status and message)
          if (response.status && response.message) {
            if (response.status === 'ok') {
              if (response.message === "Configuration unchanged") {
                setConfigUpdateStatus(`Configuration unchanged.`);
              } else {
                setConfigUpdateStatus(`Config update request successful: ${response.message}. Waiting for applied config.`);
              }
              // refreshConfig(); // REFRESH_CONFIG_REMOVED: Global config will update via EegConfigProvider's WebSocket
            } else {
              setConfigUpdateStatus(`Config update error: ${response.message}`);
            }
          } else {
            // It might be the initial full config object, or an updated one
            // For now, we primarily care about command responses here.
            // The EegConfig context handles receiving the main config.
            console.log('Received config object via /config WebSocket:', response);
          }
        } catch (e) {
          console.error('Error parsing /config WebSocket message:', e);
          setConfigUpdateStatus('Error processing message from config service.');
        }
      };

      newConfigWs.onclose = () => {
        clearTimeout(connectionTimeout);
        console.log('/config WebSocket disconnected');
        setConfigWebSocket(null);
        setIsConfigWsOpen(false);
        // Only set to 'Disconnected' if it wasn't already an error or timeout
        setConfigUpdateStatus(prevStatus =>
          prevStatus && (prevStatus.includes('Error') || prevStatus.includes('timed out'))
          ? prevStatus
          : 'Disconnected from config service.'
        );
      };

      newConfigWs.onerror = (error) => {
        clearTimeout(connectionTimeout);
        console.error('/config WebSocket error:', error);
        setConfigWebSocket(null);
        setIsConfigWsOpen(false);
        setConfigUpdateStatus('Error connecting to config service.');
      };

      return () => {
        clearTimeout(connectionTimeout);
        console.log('Closing /config WebSocket');
        if (newConfigWs.readyState === WebSocket.OPEN || newConfigWs.readyState === WebSocket.CONNECTING) {
          newConfigWs.close();
        }
        setConfigWebSocket(null);
        setIsConfigWsOpen(false);
        setConfigUpdateStatus(null);
      };
    } else {
      // If settings are hidden, ensure WebSocket is closed and status cleared
      if (configWebSocket) {
        console.log('Closing /config WebSocket because settings are hidden');
        configWebSocket.close();
        setConfigWebSocket(null);
        setIsConfigWsOpen(false);
      }
      setConfigUpdateStatus(null);
    }
  }, [activeView]); // REFRESH_CONFIG_REMOVED: refreshConfig removed from dependencies


  const handleUpdateConfig = () => {
    if (!configWebSocket || configWebSocket.readyState !== WebSocket.OPEN) {
      console.error('Config WebSocket not connected or not ready.');
      setConfigUpdateStatus('Error: Config service not connected. Cannot send update.');
      return;
    }

    if (!selectedChannelCount && !selectedSampleRate && !selectedPowerlineFilter) {
      setConfigUpdateStatus('No changes selected to update.');
      console.log('No changes to send for config update.');
      return;
    }
    
    const newConfig: { channels?: number[]; sample_rate?: number; powerline_filter_hz?: number | null } = {};

    if (selectedChannelCount !== undefined) {
      const numChannels = parseInt(selectedChannelCount, 10);
      // Create an array of channel indices [0, 1, ..., numChannels-1]
      // The daemon expects actual channel indices, not just the count.
      // However, the UI currently only allows selecting the *number* of channels.
      // For now, let's assume if user selects "N channels", they mean channels 0 to N-1.
      // This might need refinement based on how channel selection is truly intended.
      if (!isNaN(numChannels) && numChannels > 0) {
        newConfig.channels = Array.from({ length: numChannels }, (_, i) => i);
      } else if (numChannels === 0) {
         newConfig.channels = []; // Send empty array if 0 channels selected
      }
    }

    if (selectedSampleRate !== undefined) {
      const rate = parseInt(selectedSampleRate, 10);
      if (!isNaN(rate)) {
        newConfig.sample_rate = rate;
      }
    }
    
    if (selectedPowerlineFilter !== undefined) {
      if (selectedPowerlineFilter === 'off') {
        newConfig.powerline_filter_hz = null;
      } else {
        const filterHz = parseInt(selectedPowerlineFilter, 10);
        if (!isNaN(filterHz) && (filterHz === 50 || filterHz === 60)) {
          newConfig.powerline_filter_hz = filterHz;
        }
      }
    }
    
    console.log('Sending config update:', newConfig);
    setConfigUpdateStatus('Sending configuration update...');
    configWebSocket.send(JSON.stringify(newConfig));
  };

  // Debug info reference (ensure it's defined)
  const debugInfoRef = useRef<{
    lastPacketTime: number;
    packetsReceived: number;
    samplesProcessed: number;
  }>({
    lastPacketTime: 0,
    packetsReceived: 0,
    samplesProcessed: 0
  });

  // Use the command WebSocket context
  const {
    wsConnected,
    startRecording,
    stopRecording,
    recordingStatus,
    recordingFilePath,
    // ws, // ws is used by EegRecordingControls via the context
    // startRecording, // Handled by EegRecordingControls
    // stopRecording, // Handled by EegRecordingControls
    // wsConnected, // Used by EegRecordingControls
  } = useCommandWebSocket();

  // Update canvas dimensions based on container size
  // Effect to update the windowSizeRef based on config and container width
  useEffect(() => {
    // Function to calculate and update windowSizeRef
    const updateWindowSize = () => {
      if (containerRef.current && config?.sample_rate) {
        const { width } = containerRef.current.getBoundingClientRect();
        const sampleRate = config.sample_rate;
        // Calculate samples needed based on container width, sample rate, and window duration
        const samplesNeeded = Math.ceil((width / 800) * (sampleRate * WINDOW_DURATION / 1000));
        windowSizeRef.current = samplesNeeded;
        console.log(`Window size ref updated: ${samplesNeeded} samples (based on width ${width}, rate ${sampleRate})`);
      }
    };
    
    // Update initially when config is available
    updateWindowSize();
    
    // Optional: Add resize listener if window size should adapt dynamically to container resize
    // Note: EegRenderer already uses ResizeObserver, so this might be redundant
    // If needed, consider using the ResizeObserver from EegRenderer or adding one here.
    // For now, we only update based on config load.
  }, [config?.sample_rate]); // Remove containerRef dependency here, handled by ResizeObserver below

  // Handle data updates (memoized with useCallback)
  const handleDataUpdate = useCallback((received: boolean) => {
    setDataReceived(received);
  }, [setDataReceived]); // setDataReceived is stable

  // Handle driver errors (memoized with useCallback)
  const handleDriverError = useCallback((error: string) => {
    console.error("Driver error:", error);
    setDriverError(error);
    // Auto-clear error after 10 seconds
    const timer = setTimeout(() => setDriverError(null), 10000);
    // Optional: Cleanup timeout if component unmounts or error changes before timeout fires
    // return () => clearTimeout(timer); // Note: useCallback doesn't directly support cleanup return like useEffect
  }, [setDriverError]); // setDriverError is stable

  // Handle FFT data updates
  const handleFftData = useCallback((channelIndex: number, fftOutput: number[]) => {
    fftDataRef.current[channelIndex] = fftOutput;
    setFftDataVersion(v => v + 1);
    // Optional: Add logging for FFT data if needed for debugging
    // console.log(`[EegMonitor] Received FFT data for Ch ${channelIndex}:`, fftOutput.slice(0, 5));
  }, [setFftDataVersion]); // setFftDataVersion is stable

  // Effect to initialize/resize channelTimestampRef based on channel count
  useEffect(() => {
    const numChannels = config?.channels?.length || 0;
    if (numChannels > 0 && channelTimestampRef.current.length !== numChannels) {
      // Initialize or resize the array, filling with the current time
      channelTimestampRef.current = Array(numChannels).fill(performance.now());
      console.log(`Initialized/Resized channelTimestampRef for ${numChannels} channels.`);
    }
  }, [config?.channels?.length]); // Depend on the number of channels

  // Get data handler status and FPS
  const { status } = useEegDataHandler({
    config,
    onDataUpdate: handleDataUpdate,
    onError: handleDriverError,
    linesRef: dataRef, // Pass dataRef as linesRef (holds WebglStep instances)
    lastDataChunkTimeRef: channelTimestampRef, // Pass the array ref for per-channel times
    latestTimestampRef: latestTimestampRef,     // Pass the single ref for the overall latest time
    debugInfoRef: debugInfoRef, // Pass debugInfoRef for packet counting
    onFftData: activeView === 'fftGraph' ? handleFftData : undefined // Pass FFT handler only if in FFT view
  });

  // Removed dedicated useLayoutEffect for ResizeObserver

  // Effect to create WebGL lines when config and CONTAINER SIZE are ready
  // Constants for scaling
  const MICROVOLT_CONVERSION_FACTOR = 1e6; // V to uV
  const BASE_VISUAL_AMPLITUDE_SCALE = 0.01; // Renamed from VISUAL_AMPLITUDE_SCALE for UI scaling
  const UI_SCALE_FACTORS = [0.125, 0.25, 0.5, 1, 2, 4, 8]; // Added UI Scale Factors

  useEffect(() => {
    // --- ResizeObserver Setup ---
    let resizeObserver: ResizeObserver | null = null;
    const target = containerRef.current;
    let sizeUpdateTimeoutId: NodeJS.Timeout | null = null;

    if (activeView !== 'settings' && target) {
        console.log("[EegMonitor LineEffect] Setting up ResizeObserver.");
        resizeObserver = new ResizeObserver(entries => {
          for (let entry of entries) {
            const { width, height } = entry.contentRect;
            console.log(`[EegMonitor ResizeObserver] Observed size change: ${width}x${height}. Current activeView: ${activeView}`);
            
            // Clear any existing timeout to avoid multiple rapid updates
            if (sizeUpdateTimeoutId) {
              clearTimeout(sizeUpdateTimeoutId);
            }
            
            // Use a small timeout to ensure the size is stable
            sizeUpdateTimeoutId = setTimeout(() => {
              setContainerSize(prevSize => {
                if (prevSize.width !== width || prevSize.height !== height) {
                  console.log(`[EegMonitor ResizeObserver] Setting container size: ${width}x${height}`);
                  return { width, height };
                }
                return prevSize;
              });
            }, 50); // Small delay to ensure DOM is fully laid out
          }
        });
        resizeObserver.observe(target);

        // Check initial size when observer is set up
        const initialRect = target.getBoundingClientRect();
        if (initialRect.width > 0 && initialRect.height > 0 && (containerSize.width !== initialRect.width || containerSize.height !== initialRect.height)) {
            console.log(`[EegMonitor ResizeObserver] Setting initial size: ${initialRect.width}x${initialRect.height}`);
            setContainerSize({ width: initialRect.width, height: initialRect.height });
        }
    }
    // --- End ResizeObserver Setup ---


    console.log(`[EegMonitor LineEffect RUNS] activeView: ${activeView}, containerSize: ${JSON.stringify(containerSize)}, Config: ${!!config}, Channels: ${config?.channels?.length}, ContainerWidth: ${containerSize.width}`);

    // Handle navigating AWAY from graph
    if (activeView === 'settings') {
        console.log("[EegMonitor LineEffect] In settings view, ensuring lines are cleared.");
        if (dataRef.current.length > 0 || linesReady) {
             console.log("[EegMonitor LineEffect] Clearing lines for settings view (activeView is 'settings').");
             dataRef.current = [];
             setLinesReady(false);
             setDataVersion(v => v + 1);
        }
        // Cleanup observer if it exists from a previous render
        return () => {
            if (resizeObserver && target) {
                console.log("[EegMonitor LineEffect] Cleaning up ResizeObserver (settings view).");
                resizeObserver.unobserve(target);
                resizeObserver.disconnect();
            }
        };
    }

    // Add a minimum width threshold to ensure container is properly measured
    const MIN_VALID_WIDTH = 50; // Minimum width to consider container properly measured
    
    // Depend on config, channels, and the container SIZE state
    if (config && config.channels && containerSize.width > MIN_VALID_WIDTH) {
      const numChannels = config.channels.length;
      const width = containerSize.width; // Use width from state
      const sampleRate = config.sample_rate || 250; // Use default if needed

      // Calculate points needed based on current width
      const initialNumPoints = Math.max(10, Math.ceil((width / 800) * (sampleRate * WINDOW_DURATION / 1000))); // Ensure at least 10 points

      console.log(`[EegMonitor] Measured container width: ${width}`); // Log width
      console.log(`[EegMonitor] Calculated initialNumPoints: ${initialNumPoints}`); // Log points
      console.log(`[EegMonitor] Container size from state:`, containerSize);
      console.log(`[EegMonitor] Container element rect:`, containerRef.current?.getBoundingClientRect());
      console.log(`[EegMonitor] activeView:`, activeView);

      // Width check is now handled by the MIN_VALID_WIDTH check in the if condition

      if (numChannels === 0) {
          console.warn("[EegMonitor LineEffect] Zero channels configured. Clearing lines.");
          if (dataRef.current.length > 0 || linesReady) {
            dataRef.current = [];
            setLinesReady(false);
            setDataVersion(v => v + 1);
          }
          return;
      }

      // Skip condition removed to ensure lines are always reconfigured when config.channels changes.
      // The useEffect dependency on config.channels handles triggering this.
      console.log(`[EegMonitor LineEffect] Proceeding to create/update lines. Current lines: ${dataRef.current?.length}, Required: ${numChannels}. Current points: ${dataRef.current?.[0]?.numPoints}, Required: ${initialNumPoints}`);

      console.log(`[EegMonitor] Creating/Updating ${numChannels} WebGL lines with ${initialNumPoints} points each (Width: ${width}).`);

      const lines: WebglStep[] = [];
      const ySpacing = 2.0 / numChannels; // Total Y range is 2 (-1 to 1)

      for (let i = 0; i < numChannels; i++) {
        // Reuse existing line instance if possible, otherwise create new
        const line = dataRef.current?.[i] instanceof WebglStep
                     ? dataRef.current[i]
                     : new WebglStep(new ColorRGBA(1, 1, 1, 1), initialNumPoints);

        // Ensure numPoints is updated if it changed
        if (line.numPoints !== initialNumPoints) {
            line.numPoints = initialNumPoints;
        }

        // Set color - MOVED TO EegRenderer
        /*
        try {
          const colorTuple = getChannelColor(i);
          // Ensure color values are in 0-1 range for WebglPlot ColorRGBA
          line.color = new ColorRGBA(
            colorTuple[0] / 255,
            colorTuple[1] / 255,
            colorTuple[2] / 255,
            1
          );
        } catch {
          line.color = new ColorRGBA(1, 1, 1, 1); // fallback white
        }
        */

        line.lineWidth = 1;
        // Original Scale Y: Convert to microvolts AND scale for visual spacing/amplitude
        // const calculatedScaleY = (ySpacing * VISUAL_AMPLITUDE_SCALE) * MICROVOLT_CONVERSION_FACTOR;
        // New calculation using uiVoltageScaleFactor:
        const finalVisualAmplitudeScale = BASE_VISUAL_AMPLITUDE_SCALE * uiVoltageScaleFactor;
        const calculatedScaleY = (ySpacing * finalVisualAmplitudeScale) * MICROVOLT_CONVERSION_FACTOR;
        line.scaleY = calculatedScaleY;
        // console.log(`[EegMonitor LineEffect] Ch ${i}: ySpacing=${ySpacing.toFixed(4)}, finalVisualAmplitudeScale=${finalVisualAmplitudeScale}, calculatedScaleY=${calculatedScaleY}`);

        // Center channel i vertically within its allocated space
        line.offsetY = 1 - (i + 0.5) * ySpacing;

        // Set horizontal spacing
        line.lineSpaceX(-1, 2 / initialNumPoints);

        lines.push(line);
      }
      dataRef.current = lines;
      console.log(`[EegMonitor] Assigned ${lines.length} lines to dataRef. Bumping version.`);
      setLinesReady(true); // Mark lines as ready
      setDataVersion(v => v + 1); // Increment version

    } else {
        // This block executes if not in settings view, AND (config is null, or channels are null/empty, or containerSize.width is 0)
        // The specific check for numChannels === 0 inside the 'if' block already handles clearing.
        // This 'else' handles cases where the outer 'if (config && config.channels && containerSize.width > 0)' fails.
        console.log(`[EegMonitor LineEffect SKIPPING] Condition not met: config=${!!config}, config.channels=${!!config?.channels}, containerSize.width=${containerSize.width} > MIN_VALID_WIDTH=${MIN_VALID_WIDTH}`);
        if (linesReady || dataRef.current.length > 0) { // If lines were previously ready or data exists
            dataRef.current = [];
            setLinesReady(false);
            setDataVersion(v => v + 1);
        }
    }

    // Cleanup function for the effect
    return () => {
        if (resizeObserver && target) {
            console.log("[EegMonitor LineEffect] Cleaning up ResizeObserver.");
            resizeObserver.unobserve(target);
            resizeObserver.disconnect();
        }
    };
    // Depend on config, container size state, activeView, AND uiVoltageScaleFactor
  }, [config?.channels, config?.sample_rate, containerSize, activeView, uiVoltageScaleFactor]);
  
  // Use the FPS from config with no fallback
  const displayFps = config?.fps || 0;

  // Handler for the "Brain Waves" / "Signal Graph" button
  const handleToggleGraphView = () => {
    if (activeView === 'signalGraph') {
      setActiveView('fftGraph');
    } else if (activeView === 'fftGraph') {
      setActiveView('signalGraph');
    } else if (activeView === 'settings') {
      // If in settings, switch to the last active graph view
      setActiveView(lastGraphView);
      // Ensure container size is reset for graph view
      console.log("[EegMonitor handleToggleGraphView] Switching from settings to graph view. Resetting containerSize.");
      setContainerSize({ width: 0, height: 0 });
    }
  };

  // Handler for the "Settings" / "Back to [Graph]" button
  const handleToggleSettingsView = () => {
    if (activeView !== 'settings') {
      // Store the current graph view before switching to settings
      if (activeView === 'signalGraph' || activeView === 'fftGraph') {
        setLastGraphView(activeView);
      }
      setActiveView('settings');
      // Settings panel manages its own layout, graph container size reset is handled when switching back.
    } else {
      // Switching back from settings to the last graph view
      setActiveView(lastGraphView);
      // If switching from settings to graph view, ensure container size is reset
      // to trigger proper measurement and line creation for the graph
      console.log("[EegMonitor handleToggleSettingsView] Switching from settings to graph view. Resetting containerSize.");
      setContainerSize({ width: 0, height: 0 });
    }
  };
  
  // Effect to handle transition from settings to graph view
  useEffect(() => {
    if (activeView !== 'settings' && containerRef.current) {
      console.log(`[EegMonitor TransitionEffect RUNS] activeView: ${activeView}, containerRef.current: ${!!containerRef.current}. Scheduling size check.`);
      
      // Use a timeout to ensure the DOM has updated after the view switch
      const transitionTimeoutId = setTimeout(() => {
        const rect = containerRef.current?.getBoundingClientRect();
        console.log(`[EegMonitor TransitionEffect TIMEOUT] Measured rect: ${JSON.stringify(rect)}`);
        if (rect && rect.width > 0) {
          console.log(`[EegMonitor] Post-transition size check: ${rect.width}x${rect.height}`);
          // Update container size if it's not already set correctly
          setContainerSize(prevSize => {
            if (prevSize.width !== rect.width || prevSize.height !== rect.height) {
              console.log(`[EegMonitor TransitionEffect TIMEOUT] Updating container size from ${JSON.stringify(prevSize)} to ${rect.width}x${rect.height}`);
              return { width: rect.width, height: rect.height };
            }
            console.log(`[EegMonitor TransitionEffect TIMEOUT] Container size already correct: ${JSON.stringify(prevSize)}`);
            return prevSize;
          });
        } else {
          console.log(`[EegMonitor TransitionEffect TIMEOUT] Rect not valid or width is 0. rect: ${JSON.stringify(rect)}`);
        }
      }, 100); // Small delay to ensure DOM is updated
      
      return () => clearTimeout(transitionTimeoutId);
    }
  }, [activeView]);

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
          
          {/* Brain Waves / Signal Graph toggle button */}
          <button
            onClick={handleToggleGraphView}
            className="px-4 py-1 rounded-md bg-teal-600 hover:bg-teal-700 text-white"
            // disabled={activeView === 'settings'} // Optionally disable if in settings, or allow switching
          >
            {activeView === 'signalGraph' ? 'Brain Waves' : activeView === 'fftGraph' ? 'Signal Graph' : lastGraphView === 'signalGraph' ? 'Brain Waves' : 'Signal Graph'}
          </button>

          {/* Settings button */}
          <button
            onClick={handleToggleSettingsView}
            className="px-4 py-1 rounded-md bg-blue-600 hover:bg-blue-700 text-white"
          >
            {activeView === 'settings'
              ? `Back to ${lastGraphView === 'signalGraph' ? 'Signal' : 'FFT'} Graph`
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
                      disabled={!isConfigWsOpen}
                      className={`px-6 py-2 rounded-md text-white focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-opacity-50 ${
                        !isConfigWsOpen
                          ? 'bg-gray-500 cursor-not-allowed'
                          : 'bg-blue-600 hover:bg-blue-700'
                      }`}
                    >
                      Update Config
                    </button>
                    {configUpdateStatus && (
                      <p className={`mt-2 text-xs ${configUpdateStatus.includes('error') ? 'text-red-400' : 'text-gray-400'}`}>
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
            {/* Time markers */}
            <div className="relative h-full"> {/* This div is important for positioning */}
              <div className="absolute w-full flex justify-between px-2 -top-6 text-gray-400 text-sm">
                {[...TIME_TICKS].reverse().map(time => (
                  <div key={time}>{time}s</div>
                ))}
              </div>
              
              <div className="relative h-full" ref={containerRef}> {/* EegRenderer and related elements */}
                {/* Channel labels */}
                <div className="absolute -left-8 h-full flex flex-col justify-between">
                  {config?.channels && config.channels.length > 0 ? (
                    config.channels.map((chIdx) => (
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
                  dataVersion={dataVersion}
                  targetFps={displayFps}
                />
                 <EegStatusBar
                   status={status}
                   fps={displayFps}
                   dataReceived={dataReceived}
                   packetsReceived={debugInfoRef.current.packetsReceived}
                 />
             </div>
           </div>
         </div>
       ) : activeView === 'fftGraph' ? (
         <div className="relative h-full" ref={containerRef}>
           {/* Canvas for FFT plot */}
           <canvas
             ref={canvasRef} // Reusing the same canvas, FftRenderer will manage it
             className="w-full h-full border border-gray-700 rounded-lg" // Style consistent with EegRenderer
           />
           <FftRenderer
             canvasRef={canvasRef}
             fftDataRef={fftDataRef}
             fftDataVersion={fftDataVersion}
             config={config}
             containerWidth={containerSize.width}
             containerHeight={containerSize.height}
             linesReady={linesReady} // linesReady indicates container is ready and basic config is loaded
                                     // FftRenderer internally manages its own line setup based on FFT data.
             targetFps={displayFps} // Use the same displayFps for now
           />
           {/* Optional: Add specific FFT axis labels or overlays here if needed */}
           {/* Example: X-axis labels for frequency */}
           {/* <div className="absolute w-full flex justify-between px-2 -bottom-6 text-gray-400 text-sm">
             <span>{FFT_MIN_FREQ_HZ} Hz</span>
             <span>{(FFT_MIN_FREQ_HZ + FFT_MAX_FREQ_HZ) / 2} Hz</span>
             <span>{FFT_MAX_FREQ_HZ} Hz</span>
           </div> */}
           <EegStatusBar
             status={status} // Or a more relevant status for FFT
             fps={displayFps}
             dataReceived={dataReceived} // This reflects raw data; FFT is derived
             packetsReceived={debugInfoRef.current.packetsReceived}
           />
         </div>
       ) : null}
     </div>
   </div>
  );
}