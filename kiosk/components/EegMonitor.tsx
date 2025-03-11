'use client';

import { useEffect, useState, useMemo } from 'react';
import { LinePath } from '@visx/shape';
import { scaleLinear, scaleTime } from '@visx/scale';
import { extent } from '@visx/vendor/d3-array';
import { Group } from '@visx/group';
import { AxisLeft, AxisBottom } from '@visx/axis';

interface EegData {
  channels: number[];
  timestamp: number;
}

interface DataPoint {
  timestamp: number;
  value: number;
  channel: number;
}

export default function EegMonitor() {
  const [eegHistory, setEegHistory] = useState<DataPoint[]>([]);
  const [status, setStatus] = useState('Connecting...');
  
  // Graph dimensions
  const width = 800;
  const height = 400;
  const margin = { top: 20, right: 20, bottom: 40, left: 40 };

  // Update the WebSocket handler to maintain history
  useEffect(() => {
    const ws = new WebSocket('ws://localhost:8080/eeg');

    ws.onopen = () => {
      setStatus('Connected');
    };

    ws.onmessage = (event) => {
      const data: EegData = JSON.parse(event.data);
      // Convert incoming data to DataPoints and add to history
      const newPoints = data.channels.map((value, idx) => ({
        timestamp: data.timestamp,
        value,
        channel: idx,
      }));
      
      setEegHistory(prev => [...prev, ...newPoints].slice(-1000)); // Keep last 1000 points
    };

    // ... existing error and close handlers ...

    return () => {
      ws.close();
    };
  }, []);

  // Create scales for the graph
  const xScale = useMemo(() => {
    const domain = extent(eegHistory, d => d.timestamp) as [number, number];
    return scaleTime({
      domain,
      range: [margin.left, width - margin.right],
    });
  }, [eegHistory, width]);

  const yScale = useMemo(() => {
    const domain = extent(eegHistory, d => d.value) as [number, number];
    return scaleLinear({
      domain,
      range: [height - margin.bottom, margin.top],
    });
  }, [eegHistory, height]);

  return (
    <div className="p-4">
      <h1 className="text-2xl font-bold mb-4">EEG Monitor</h1>
      <div className="mb-2">Status: {status}</div>
      
      <svg width={width} height={height}>
        <Group>
          {/* Draw a line for each channel */}
          {Array.from(new Set(eegHistory.map(d => d.channel))).map(channel => (
            <LinePath
              key={channel}
              data={eegHistory.filter(d => d.channel === channel)}
              x={d => xScale(d.timestamp) ?? 0}
              y={d => yScale(d.value) ?? 0}
              stroke={`hsl(${channel * 30}, 70%, 50%)`}
              strokeWidth={2}
            />
          ))}
          
          <AxisBottom
            scale={xScale}
            top={height - margin.bottom}
            label="Time"
          />
          
          <AxisLeft
            scale={yScale}
            left={margin.left}
            label="Amplitude"
          />
        </Group>
      </svg>
    </div>
  );
} 