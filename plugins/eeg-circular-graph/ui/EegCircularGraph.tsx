'use client';

import React, { useRef, useEffect, useState, useCallback } from 'react';
import { EegCircularRenderer } from './EegCircularRenderer';

interface EegCircularGraphProps {
  config: any;
  containerWidth: number;
  containerHeight: number;
  data?: number[][]; // New prop for real-time data
  targetFps?: number;
  displaySeconds?: number;
}

export const EegCircularGraph = ({
  config,
  containerWidth,
  containerHeight,
  data,
  targetFps = 60,
  displaySeconds = 10
}: EegCircularGraphProps) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dataRef = useRef<Float32Array[]>([]);
  const latestTimestampRef = useRef<number>(0);
  const [dataBuffers, setDataBuffers] = useState<Float32Array[]>([]);
  
  const samplingRate = config?.sampling_rate || 1000;
  const numPoints = samplingRate * displaySeconds;
  const numChannels = config?.channels?.length || 8;

  // Initialize data buffers
  useEffect(() => {
    if (numChannels === 0) return;
    
    dataRef.current = [];
    for (let i = 0; i < numChannels; i++) {
      dataRef.current.push(new Float32Array(numPoints));
    }
  }, [numChannels, numPoints]);

  // Update data buffers with new EEG data
  const updateData = useCallback((newData: number[][]) => {
    const now = Date.now();
    if (now - latestTimestampRef.current < 1000 / samplingRate) return;
    
    latestTimestampRef.current = now;
    
    const newBuffers = [...dataBuffers];
    for (let ch = 0; ch < numChannels; ch++) {
      if (!newBuffers[ch]) continue;
      
      // Get latest sample for this channel
      const sample = newData[ch]?.[newData[ch].length - 1] || 0;
      newBuffers[ch][0] = sample; // Simplified - actual circular buffer logic in renderer
    }
    setDataBuffers(newBuffers);
  }, [dataBuffers, numChannels, samplingRate]);

  // Effect to handle incoming data from WebSocket
  useEffect(() => {
    if (data && data.length > 0) {
      updateData(data);
    }
  }, [data, updateData]);

  return (
    <div className="eeg-circular-graph" style={{ width: containerWidth, height: containerHeight }}>
      <canvas 
        ref={canvasRef} 
        style={{ width: '100%', height: '100%' }}
      />
      <EegCircularRenderer
        canvasRef={canvasRef}
        dataRef={{ current: dataBuffers }}
        config={config}
        latestTimestampRef={latestTimestampRef}
        numPoints={numPoints}
        targetFps={targetFps}
        containerWidth={containerWidth}
        containerHeight={containerHeight}
      />
    </div>
  );
};