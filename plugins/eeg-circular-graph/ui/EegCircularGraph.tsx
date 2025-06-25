'use client';

import React, { useRef, useEffect, useState, useCallback } from 'react';
import { EegCircularRenderer, EegCircularRendererRef } from './EegCircularRenderer';
import { useDataBuffer } from '../../../kiosk/src/hooks/useDataBuffer';
import { SampleChunk } from '../../../kiosk/src/types/eeg';

interface EegCircularGraphProps {
  config: any;
  containerWidth: number;
  containerHeight: number;
  dataBuffer: ReturnType<typeof useDataBuffer<SampleChunk>>;
  targetFps?: number;
  displaySeconds?: number;
  dataVersion: number;
}

export const EegCircularGraph = ({
  config,
  containerWidth,
  containerHeight,
  dataBuffer,
  targetFps = 60,
  displaySeconds = 10,
  dataVersion
}: EegCircularGraphProps) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<EegCircularRendererRef>(null);
  const animationFrameRef = useRef<number>();

  const samplingRate = config?.sampling_rate || 1000;
  const numPoints = samplingRate * displaySeconds;
  const numChannels = config?.channels?.length || 8;

  // New render logic to pull data from the buffer asynchronously
  useEffect(() => {
    const sampleChunks = dataBuffer.getAndClearData();
        
    if (sampleChunks.length > 0 && rendererRef.current) {
      // The data from the buffer is now a flat array of SampleChunk objects.
      // We can iterate through them directly.
      sampleChunks.forEach((chunk: SampleChunk) => {
        chunk.samples.forEach((sample) => {
          const chIndex = sample.channelIndex;
          if (chIndex < numChannels && rendererRef.current) {
            rendererRef.current.addNewSample(chIndex, sample.value);
          }
        });
      });
    }
  }, [dataVersion, dataBuffer, numChannels]);

  return (
    <div className="eeg-circular-graph" style={{ width: containerWidth, height: containerHeight }}>
      <canvas
        ref={canvasRef}
        style={{ width: '100%', height: '100%' }}
      />
      <EegCircularRenderer
        ref={rendererRef}
        canvasRef={canvasRef}
        config={config}
        numPoints={numPoints}
        targetFps={targetFps}
        containerWidth={containerWidth}
        containerHeight={containerHeight}
      />
    </div>
  );
};