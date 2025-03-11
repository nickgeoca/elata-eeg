'use client';

import { useEffect, useState, createContext, useContext } from 'react';

// Define the EEG configuration interface
export interface EegConfig {
  sample_rate: number;
  channels: number[];
  gain: number;
  board_driver: string;
  batch_size: number;
  fps?: number; // Optional as it might be calculated client-side
}

// Create a context to share the configuration across components
interface EegConfigContextType {
  config: EegConfig | null;
  status: string;
}

export const EegConfigContext = createContext<EegConfigContextType>({
  config: null,
  status: 'Initializing...'
});

// Hook to use the EEG configuration
export const useEegConfig = () => useContext(EegConfigContext);

// Provider component
export function EegConfigProvider({ children }: { children: React.ReactNode }) {
  const [config, setConfig] = useState<EegConfig | null>(null);
  const [status, setStatus] = useState('Connecting...');

  useEffect(() => {
    const ws = new WebSocket('ws://localhost:8080/config');
    
    ws.onopen = () => setStatus('Connected');
    
    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        
        // Calculate FPS client-side based on sample rate and batch size
        const calculatedFps = data.sample_rate / data.batch_size;
        
        // Add the calculated FPS to the config
        const configWithFps = {
          ...data,
          fps: calculatedFps
        };
        
        setConfig(configWithFps);
        console.log('Received EEG configuration:', configWithFps);
      } catch (error) {
        console.error('Error parsing config data:', error);
      }
    };
    
    ws.onclose = () => setStatus('Disconnected');
    ws.onerror = () => setStatus('Error');
    
    return () => ws.close();
  }, []);

  return (
    <EegConfigContext.Provider value={{ config, status }}>
      {children}
    </EegConfigContext.Provider>
  );
}

// Display component
export default function EegConfigDisplay() {
  const { config, status } = useEegConfig();

  return (
    <div className="p-4 bg-gray-900 text-white rounded-lg mb-4">
      <h2 className="text-xl font-bold mb-2">EEG Configuration</h2>
      <div className="mb-2">Status: {status}</div>
      
      {config ? (
        <div className="grid grid-cols-2 gap-2">
          <div className="col-span-2 font-semibold text-blue-400">System Parameters</div>
          
          <div>Sample Rate:</div>
          <div>{config.sample_rate} Hz</div>
          
          <div>Channels:</div>
          <div>{config.channels.join(', ')}</div>
          
          <div>Gain:</div>
          <div>{config.gain}</div>
          
          <div>Board Driver:</div>
          <div>{config.board_driver}</div>
          
          <div>Batch Size:</div>
          <div>{config.batch_size} samples</div>
          
          <div>Effective FPS:</div>
          <div>{config.fps?.toFixed(2)} frames/sec</div>
        </div>
      ) : (
        <div className="text-gray-400">Waiting for configuration data...</div>
      )}
    </div>
  );
}