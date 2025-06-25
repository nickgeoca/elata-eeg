'use client';

import React from 'react';
import { EegCircularGraph } from '../../../plugins/eeg-circular-graph/ui/EegCircularGraph';
import { useDataBuffer } from '../hooks/useDataBuffer';

interface CircularGraphWrapperProps {
  config: any;
  containerWidth: number;
  containerHeight: number;
  dataBuffer: ReturnType<typeof useDataBuffer>;
  targetFps?: number;
  displaySeconds?: number;
}

export const CircularGraphWrapper = ({
  config,
  containerWidth,
  containerHeight,
  dataBuffer,
  targetFps = 60,
  displaySeconds = 10
}: CircularGraphWrapperProps) => {

  // The wrapper now directly uses the EegCircularGraph and passes the buffer.
  return (
    <EegCircularGraph
      config={config}
      containerWidth={containerWidth}
      containerHeight={containerHeight}
      dataBuffer={dataBuffer}
      targetFps={targetFps}
      displaySeconds={displaySeconds}
    />
  );
};