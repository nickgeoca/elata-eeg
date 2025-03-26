'use client';

import { useRef, useState, useEffect, useCallback } from 'react';
import { useEegConfig } from './EegConfig';
import EegConfigDisplay from './EegConfig';
import { EegStatusBar } from './EegStatusBar';
import { useEegDataHandler } from './EegDataHandler';
import { EegRenderer } from './EegRenderer';
import { ScrollingBuffer } from '../utils/ScrollingBuffer';
import { GRAPH_HEIGHT, WINDOW_DURATION, TIME_TICKS } from '../utils/eegConstants';

export default function EegMonitorWebGL() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const windowSizeRef = useRef<number>(500); // Default, will be updated based on config
  const dataRef = useRef<ScrollingBuffer[]>([]);
  const [dataReceived, setDataReceived] = useState(false);
  const latestTimestampRef = useRef<number>(Date.now());
  const [canvasDimensions, setCanvasDimensions] = useState({ width: 800, height: 480 });
  const [showSettings, setShowSettings] = useState(false);
  
  // Recording state
  const [isRecording, setIsRecording] = useState(false);
  const [recordingStatus, setRecordingStatus] = useState('Not recording');
  const [recordingFilePath, setRecordingFilePath] = useState<string | null>(null);
  const [wsConnected, setWsConnected] = useState(false);
  const [wsRef, setWsRef] = useState<WebSocket | null>(null);
  
  // Get configuration from context
  const { config, status: configStatus } = useEegConfig();

  // Debug info reference
  const debugInfoRef = useRef<{
    lastPacketTime: number;
    packetsReceived: number;
    samplesProcessed: number;
  }>({
    lastPacketTime: 0,
    packetsReceived: 0,
    samplesProcessed: 0
  });

  // Connect to the command WebSocket for recording
  useEffect(() => {
    const ws = new WebSocket('ws://localhost:8080/command');
    setWsRef(ws);
    
    ws.onopen = () => {
      console.log('Command WebSocket connected');
      setWsConnected(true);
    };
    
    ws.onmessage = (event) => {
      try {
        const response = JSON.parse(event.data);
        
        if (response.status === 'ok') {
          // Parse the message to determine recording status
          const recording = response.message.startsWith('Currently recording');
          let filePath = null;
          
          if (recording) {
            // Extract file path from message
            const match = response.message.match(/Currently recording to (.+)/);
            if (match && match[1]) {
              filePath = match[1];
            }
          }
          
          setIsRecording(recording);
          setRecordingStatus(response.message);
          setRecordingFilePath(filePath);
        } else {
          console.error('Command error:', response.message);
        }
      } catch (error) {
        console.error('Error parsing command response:', error);
      }
    };
    
    ws.onclose = () => {
      console.log('Command WebSocket disconnected');
      setWsConnected(false);
    };
    
    ws.onerror = (error) => {
      console.error('Command WebSocket error:', error);
      setWsConnected(false);
    };
    
    return () => {
      ws.close();
    };
  }, []);

  // Send command to start recording
  const startRecording = useCallback(() => {
    if (wsRef && wsRef.readyState === WebSocket.OPEN) {
      wsRef.send(JSON.stringify({ command: 'start' }));
    }
  }, [wsRef]);

  // Send command to stop recording
  const stopRecording = useCallback(() => {
    if (wsRef && wsRef.readyState === WebSocket.OPEN) {
      wsRef.send(JSON.stringify({ command: 'stop' }));
    }
  }, [wsRef]);

  // Update canvas dimensions based on container size
  useEffect(() => {
    const updateDimensions = () => {
      if (containerRef.current) {
        const { width } = containerRef.current.getBoundingClientRect();
        const channelCount = config?.channels?.length || 4;
        const height = GRAPH_HEIGHT * channelCount;
        
        // Update canvas dimensions
        setCanvasDimensions({ width, height });
        
        // Update window size for ScrollingBuffer based on screen width and sample rate
        const sampleRate = config?.sample_rate || 250;
        const samplesNeeded = Math.ceil((width / 800) * (sampleRate * WINDOW_DURATION / 1000));
        windowSizeRef.current = samplesNeeded;
        
        console.log(`Canvas dimensions updated: ${width}x${height}, samples needed: ${samplesNeeded}`);
      }
    };

    // Initial update
    updateDimensions();

    // Add resize listener
    window.addEventListener('resize', updateDimensions);
    
    // Clean up
    return () => {
      window.removeEventListener('resize', updateDimensions);
    };
  }, [config]);

  // Handle data updates
  const handleDataUpdate = (received: boolean) => {
    setDataReceived(received);
  };

  // Get data handler status and FPS
  const { status, fps } = useEegDataHandler({
    config,
    onDataUpdate: handleDataUpdate,
    dataRef,
    windowSizeRef,
    debugInfoRef,
    latestTimestampRef
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
          <EegStatusBar
            status={status}
            dataReceived={dataReceived}
            fps={displayFps}
            packetsReceived={debugInfoRef.current.packetsReceived}
          />
        </div>
        <div className="flex space-x-2">
          {/* Recording button */}
          <button
            onClick={isRecording ? stopRecording : startRecording}
            disabled={!wsConnected}
            className={`px-4 py-1 rounded-md flex items-center ${
              !wsConnected
                ? 'bg-gray-700 text-gray-500 cursor-not-allowed'
                : isRecording
                  ? 'bg-red-600 hover:bg-red-700 text-white'
                  : 'bg-green-600 hover:bg-green-700 text-white'
            }`}
          >
            {isRecording ? (
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
      {isRecording && (
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
                  {Array.from({ length: config?.channels?.length || 4 }, (_, i) => i + 1).map(ch => (
                    <div key={ch} className="text-gray-400 font-medium">Ch{ch}</div>
                  ))}
                </div>
                
                {/* WebGL Canvas - Now using dynamic dimensions and full height */}
                <canvas
                  ref={canvasRef}
                  width={canvasDimensions.width}
                  height={canvasDimensions.height}
                  className="w-full h-full border border-gray-700 rounded-lg"
                />
                
                {/* WebGL Renderer (doesn't render anything directly, handles WebGL setup) */}
                <EegRenderer
                  canvasRef={canvasRef}
                  dataRef={dataRef}
                  config={config}
                  latestTimestampRef={latestTimestampRef}
                  debugInfoRef={debugInfoRef}
                />
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}