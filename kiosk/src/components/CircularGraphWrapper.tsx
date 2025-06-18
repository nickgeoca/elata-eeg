'use client';

import React, { useState, useEffect, useCallback } from 'react';

interface CircularGraphWrapperProps {
  config: any;
  containerWidth: number;
  containerHeight: number;
  data?: number[][];
  targetFps?: number;
  displaySeconds?: number;
}

export const CircularGraphWrapper = ({
  config,
  containerWidth,
  containerHeight,
  data,
  targetFps = 60,
  displaySeconds = 10
}: CircularGraphWrapperProps) => {
  const [processedData, setProcessedData] = useState<number[][]>([]);
  
  // Process incoming data for circular visualization
  useEffect(() => {
    if (data && data.length > 0) {
      // For now, just pass through the data
      // In the future, this could apply circular buffer logic
      setProcessedData(data);
    }
  }, [data]);

  return (
    <div 
      className="circular-graph-container bg-gray-800 rounded-lg border border-gray-600"
      style={{ width: containerWidth, height: containerHeight }}
    >
      <div className="flex items-center justify-center h-full text-white">
        <div className="text-center">
          <div className="text-2xl mb-4">🔄 Circular EEG Graph</div>
          <div className="text-lg mb-2">
            Channels: {config?.channels?.length || 0}
          </div>
          <div className="text-sm text-gray-400">
            Sample Rate: {config?.sample_rate || 'Unknown'} Hz
          </div>
          <div className="text-sm text-gray-400 mt-2">
            Data Points: {processedData.length > 0 ? processedData[0]?.length || 0 : 0}
          </div>
          <div className="text-xs text-gray-500 mt-4">
            Circular graph visualization will be rendered here
          </div>
        </div>
      </div>
    </div>
  );
};