'use client';

import { useRef, useState, useEffect } from 'react';
import { useEegConfig } from './EegConfig';
import { EegStatusBar } from './EegStatusBar';
import { useEegDataHandler } from './EegDataHandler';
import { EegRenderer } from './EegRenderer';
import { ScrollingBuffer } from '../utils/ScrollingBuffer';
import { GRAPH_HEIGHT, GRAPH_WIDTH, TIME_TICKS, VOLTAGE_TICKS } from '../utils/eegConstants';

export default function EegMonitorWebGL() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const windowSizeRef = useRef<number>(500); // Default, will be updated based on config
  const dataRef = useRef<ScrollingBuffer[]>([]);
  const [dataReceived, setDataReceived] = useState(false);
  const latestTimestampRef = useRef<number>(Date.now());
  const renderNeededRef = useRef<boolean>(false);
  
  // Get configuration from context
  const { config } = useEegConfig();

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
    renderNeededRef,
    latestTimestampRef
  });

  return (
    <div className="p-4 bg-gray-900">
      <h1 className="text-2xl font-bold mb-4 text-white">EEG Monitor (WebGL)</h1>
      
      {/* Status Bar */}
      <EegStatusBar 
        status={status}
        dataReceived={dataReceived}
        fps={fps}
        packetsReceived={debugInfoRef.current.packetsReceived}
      />
      
      {/* Time markers */}
      <div className="relative">
        <div className="absolute w-full flex justify-between px-2 -top-6 text-gray-400 text-sm">
          {/* Use a reversed copy instead of mutating the array in place */}
          {[...TIME_TICKS].reverse().map(time => (
            <div key={time}>{time}s</div>
          ))}
        </div>
        
        <div className="relative">
          {/* Channel labels */}
          <div className="absolute -left-8 h-full flex flex-col justify-between">
            {Array.from({ length: config?.channels?.length || 4 }, (_, i) => i + 1).map(ch => (
              <div key={ch} className="text-gray-400 font-medium">Ch{ch}</div>
            ))}
          </div>
          
          
          {/* WebGL Canvas */}
          <canvas
            ref={canvasRef}
            width={GRAPH_WIDTH}
            height={GRAPH_HEIGHT * (config?.channels?.length || 4)}
            className={`w-full border border-gray-700 rounded-lg ${
              (config?.channels?.length || 4) > 6
                ? 'h-[80vh]'
                : (config?.channels?.length || 4) > 4
                  ? 'h-[70vh]'
                  : 'h-[60vh]'
            }`}
          />
          
          {/* WebGL Renderer (doesn't render anything directly, handles WebGL setup) */}
          <EegRenderer
            canvasRef={canvasRef}
            dataRef={dataRef}
            config={config}
            renderNeededRef={renderNeededRef}
            latestTimestampRef={latestTimestampRef}
            debugInfoRef={debugInfoRef}
          />
        </div>
      </div>
    </div>
  );
}