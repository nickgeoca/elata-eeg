'use client';

import { useRef, useState, useEffect, useCallback } from 'react';
import { useEegConfig } from './EegConfig';
import EegConfigDisplay from './EegConfig';
import { EegStatusBar } from './EegStatusBar';
import { useEegDataHandler } from './EegDataHandler';
import { EegRenderer } from './EegRenderer';
import { ScrollingBuffer } from '../utils/ScrollingBuffer';
import { GRAPH_HEIGHT, WINDOW_DURATION, TIME_TICKS } from '../utils/eegConstants';
import { useCommandWebSocket } from '../context/CommandWebSocketContext';

export default function EegMonitorWebGL() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const windowSizeRef = useRef<number>(500); // Default, will be updated based on config
  const dataRef = useRef<ScrollingBuffer[]>([]);
  const [dataReceived, setDataReceived] = useState(false);
  const [driverError, setDriverError] = useState<string | null>(null);
  // Ref to hold the last update timestamp for each channel's data chunk
  const channelTimestampRef = useRef<number[]>([]);
  // Ref to hold the single latest timestamp of any received data packet
  const latestTimestampRef = useRef<number>(performance.now());
  // Removed canvasDimensions state, EegRenderer handles this
  const [showSettings, setShowSettings] = useState(false);
  
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
  }, [config?.sample_rate]); // Depend only on sample rate for window size calculation

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
  const { status, fps } = useEegDataHandler({
    config,
    onDataUpdate: handleDataUpdate,
    onError: handleDriverError,
    linesRef: dataRef, // Pass dataRef as linesRef (assuming dataRef holds the WebglStep instances)
    lastDataChunkTimeRef: channelTimestampRef, // Pass the array ref for per-channel times
    latestTimestampRef: latestTimestampRef,     // Pass the single ref for the overall latest time
    // debugInfoRef is not needed by useEegDataHandler
    // latestTimestampRef was renamed to channelTimestampRef and is passed as lastDataChunkTimeRef
  });
  
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
                  dataRef={dataRef}
                  config={config}
                  latestTimestampRef={latestTimestampRef} // Pass the single timestamp ref
                  debugInfoRef={debugInfoRef} // Pass debugInfoRef
                  containerRef={containerRef}
                />
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}