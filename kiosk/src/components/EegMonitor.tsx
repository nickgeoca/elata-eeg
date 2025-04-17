'use client';

import { useRef, useState, useEffect, useCallback, useLayoutEffect } from 'react';
import { useEegConfig } from './EegConfig';
import EegConfigDisplay from './EegConfig';
import { EegStatusBar } from './EegStatusBar';
import { useEegDataHandler } from './EegDataHandler';
import { EegRenderer } from './EegRenderer';
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
  const [showSettings, setShowSettings] = useState(false);
  const [linesReady, setLinesReady] = useState(false); // State to track line readiness
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 }); // State for container dimensions
  const [dataVersion, setDataVersion] = useState(0); // Version counter for dataRef updates
  
  // Get configuration from context
  const { config, status: configStatus } = useEegConfig();

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
    ws,
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
    // debugInfoRef is not needed by useEegDataHandler
    // latestTimestampRef was renamed to channelTimestampRef and is passed as lastDataChunkTimeRef
  });

  // Removed dedicated useLayoutEffect for ResizeObserver

  // Effect to create WebGL lines when config and CONTAINER SIZE are ready
  // Constants for scaling
  const MICROVOLT_CONVERSION_FACTOR = 1e6; // V to uV
  const VISUAL_AMPLITUDE_SCALE = 0.001; // Visual height scale (0-1) - Start with 0.1

  useEffect(() => {
    // --- ResizeObserver Setup ---
    let resizeObserver: ResizeObserver | null = null;
    const target = containerRef.current;

    if (!showSettings && target) {
        console.log("[EegMonitor LineEffect] Setting up ResizeObserver.");
        resizeObserver = new ResizeObserver(entries => {
          for (let entry of entries) {
            const { width, height } = entry.contentRect;
            setContainerSize(prevSize => {
              if (prevSize.width !== width || prevSize.height !== height) {
                console.log(`[EegMonitor ResizeObserver] Container size changed: ${width}x${height}`);
                return { width, height };
              }
              return prevSize;
            });
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


    console.log(`[EegMonitor LineEffect] Running. showSettings: ${showSettings}, Config: ${!!config}, Channels: ${config?.channels?.length}, ContainerWidth: ${containerSize.width}`);

    // Handle navigating AWAY from graph
    if (showSettings) {
        console.log("[EegMonitor LineEffect] In settings view, ensuring lines are cleared.");
        if (dataRef.current.length > 0 || linesReady) {
             console.log("[EegMonitor LineEffect] Clearing lines for settings view.");
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

    // Depend on config, channels, and the container SIZE state
    if (config && config.channels && containerSize.width > 0) {
      const numChannels = config.channels.length;
      const width = containerSize.width; // Use width from state
      const sampleRate = config.sample_rate || 250; // Use default if needed

      // Calculate points needed based on current width
      const initialNumPoints = Math.max(10, Math.ceil((width / 800) * (sampleRate * WINDOW_DURATION / 1000))); // Ensure at least 10 points

      console.log(`[EegMonitor] Measured container width: ${width}`); // Log width
      console.log(`[EegMonitor] Calculated initialNumPoints: ${initialNumPoints}`); // Log points

      // This check might be redundant now as the effect depends on containerSize.width > 0
      // if (width <= 0) {
      //     console.warn("[EegMonitor] Container width is zero (from state). Skipping line creation/update.");
      //     setLinesReady(false); // Ensure lines are not marked ready
      //     return;
      // }

      if (numChannels === 0) {
          console.warn("[EegMonitor] Skipping line creation - zero channels.");
          return;
      }

      // Avoid recreating if lines seem to match current config
      // Avoid recreating if lines seem to match current config AND point count
      console.log(`[EegMonitor LineEffect] Checking skip condition. Current lines: ${dataRef.current?.length}, Required: ${numChannels}. Current points: ${dataRef.current?.[0]?.numPoints}, Required: ${initialNumPoints}`);
      if (dataRef.current?.length === numChannels && dataRef.current[0]?.numPoints === initialNumPoints) {
          console.log("[EegMonitor LineEffect] Skipping line update - config and points match.");
          // Ensure lines are marked ready even if we skip update
          if (!linesReady) {
              console.log("[EegMonitor LineEffect] Marking linesReady=true because skip condition met.");
              setLinesReady(true);
          }
          return; // Skip if everything matches
      }

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
        const calculatedScaleY = (ySpacing * VISUAL_AMPLITUDE_SCALE) * MICROVOLT_CONVERSION_FACTOR;
        line.scaleY = calculatedScaleY;
        // console.log(`[EegMonitor LineEffect] Ch ${i}: ySpacing=${ySpacing.toFixed(4)}, VISUAL_AMPLITUDE_SCALE=${VISUAL_AMPLITUDE_SCALE}, calculatedScaleY=${calculatedScaleY}`); // Keep commented for now

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
        console.log(`[EegMonitor LineEffect] Condition NOT met (or in settings view). Config: ${!!config}, Channels: ${config?.channels?.length}, ContainerWidth: ${containerSize.width}`);
        // No need to clear lines here, handled by the showSettings check above
    }

    // Cleanup function for the effect
    return () => {
        if (resizeObserver && target) {
            console.log("[EegMonitor LineEffect] Cleaning up ResizeObserver.");
            resizeObserver.unobserve(target);
            resizeObserver.disconnect();
        }
    };
    // Depend on config, container size state, AND showSettings
  }, [config?.channels, containerSize, showSettings]);
  
  // Use the FPS from config with no fallback
  const displayFps = config?.fps || 0;

  // Toggle between settings and graph view
  const toggleSettings = () => {
    setShowSettings(!showSettings);
  };

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
        <div className="flex space-x-2">
          {/* Recording button */}
          <button
            onClick={recordingStatus.startsWith('Currently recording') ? stopRecording : startRecording}
            disabled={!wsConnected}
            className={`px-4 py-1 rounded-md flex items-center ${
              !wsConnected
                ? 'bg-gray-700 text-gray-500 cursor-not-allowed'
                : recordingStatus.startsWith('Currently recording')
                  ? 'bg-red-600 hover:bg-red-700 text-white'
                  : 'bg-green-600 hover:bg-green-700 text-white'
            }`}
          >
            {recordingStatus.startsWith('Currently recording') ? (
              <>
                <span className="inline-block w-2 h-2 rounded-full bg-white mr-2"></span>
                Stop Recording
              </>
            ) : (
              <>
                <span className="inline-block w-2 h-2 rounded-full bg-white mr-2"></span>
                Start Recording
              </>
            )}
          </button>
          
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
          
          {/* Settings button */}
          <button
            onClick={toggleSettings}
            className="px-4 py-1 rounded-md bg-blue-600 hover:bg-blue-700 text-white"
          >
            {showSettings ? 'Show Graph' : 'Settings'}
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
        {showSettings ? (
          <div className="h-full p-4 overflow-auto">
            <EegConfigDisplay />
          </div>
        ) : (
          <div className="h-full p-4">
            {/* Time markers */}
            <div className="relative h-full">
              <div className="absolute w-full flex justify-between px-2 -top-6 text-gray-400 text-sm">
                {/* Use a reversed copy instead of mutating the array in place */}
                {[...TIME_TICKS].reverse().map(time => (
                  <div key={time}>{time}s</div>
                ))}
              </div>
              
              <div className="relative h-full" ref={containerRef}>
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
                
                {/* WebGL Canvas - Now using dynamic dimensions and full height */}
                <canvas
                  ref={canvasRef} // EegRenderer will set width/height attributes
                  className="w-full h-full border border-gray-700 rounded-lg" // Style remains
                />
                
                {/* WebGL Renderer (doesn't render anything directly, handles WebGL setup) */}
                <EegRenderer
                  canvasRef={canvasRef}
                  dataRef={dataRef} // Restore prop pass
                  config={config}
                  latestTimestampRef={latestTimestampRef} // Pass the single timestamp ref
                  debugInfoRef={debugInfoRef} // Pass debugInfoRef
                  containerRef={containerRef}
                  linesReady={linesReady} // Pass down the readiness flag
                  dataVersion={dataVersion} // Pass down the data version
                />
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}