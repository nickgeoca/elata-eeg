'use client';

import React from 'react';
import { EegCircularGraph } from '../../../plugins/eeg-circular-graph/ui/EegCircularGraph';
import { useDataBuffer } from '../hooks/useDataBuffer';
import { SampleChunk } from '../types/eeg';

interface CircularGraphWrapperProps {
  isActive: boolean;
  config: any;
  containerWidth: number;
  containerHeight: number;
  dataBuffer: ReturnType<typeof useDataBuffer<SampleChunk>>;
  targetFps?: number;
  displaySeconds?: number;
  dataVersion: number;
}

export const CircularGraphWrapper = ({
  isActive,
  config,
  containerWidth,
  containerHeight,
  dataBuffer,
  targetFps = 60,
  displaySeconds = 10,
  dataVersion
}: CircularGraphWrapperProps) => {

  // The wrapper now directly uses the EegCircularGraph and passes the buffer.
  return (
    <EegCircularGraph
      isActive={isActive}
      config={config}
      containerWidth={containerWidth}
      containerHeight={containerHeight}
      dataBuffer={dataBuffer}
      targetFps={targetFps}
      displaySeconds={displaySeconds}
      dataVersion={dataVersion}
    />
  );
};