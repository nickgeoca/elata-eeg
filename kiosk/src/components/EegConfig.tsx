'use client';

import { useEffect, useState, createContext, useContext, useRef, useCallback } from 'react';

// Define the EEG configuration interface
export interface EegConfig {
  // Defined from daemon
  sample_rate: number;
  channels: number[];
  gain: number;
  board_driver: string;
  batch_size: number;
  // Defined from config file
  fps: number;
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
  const [status, setStatus] = useState('Initializing...'); // Start as Initializing
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const reconnectAttemptsRef = useRef<number>(0);
  const isProduction = process.env.NODE_ENV === 'production';

  const connectWebSocket = useCallback(() => {
    // Clear any existing reconnect timeout
    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current);
      reconnectTimeoutRef.current = null;
    }

    // Close existing connection if any
    if (wsRef.current) {
      try {
        // Remove listeners before closing to prevent triggering reconnect on manual close
        wsRef.current.onopen = null;
        wsRef.current.onmessage = null;
        wsRef.current.onerror = null;
        wsRef.current.onclose = null;
        wsRef.current.close();
      } catch (e) { /* Ignore */ }
      wsRef.current = null;
    }

    setStatus('Connecting...');
    const ws = new WebSocket('ws://localhost:8080/config');
    wsRef.current = ws;

    ws.onopen = () => {
      if (!isProduction) console.log('Config WebSocket: Connection opened');
      setStatus('Connected');
      reconnectAttemptsRef.current = 0; // Reset attempts on success
    };

    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        const FPS = 60.0; // Keep client-side FPS calculation for now
        const configWithFps = { ...data, fps: FPS };
        setConfig(configWithFps);
        if (!isProduction) console.log('Received EEG configuration:', configWithFps);
        // Consider closing the socket here if config is only needed once?
        // ws.close(); // Optional: close after receiving config
      } catch (error) {
        console.error('Error parsing config data:', error);
        setStatus('Error parsing data');
      }
    };

    ws.onclose = (event) => {
      if (!isProduction) console.log(`Config WebSocket: Connection closed (Code: ${event.code}, Reason: ${event.reason})`);
      // Only attempt reconnect if the closure was unexpected
      if (wsRef.current === ws) { // Check if this is still the active WebSocket instance
          setStatus('Disconnected');
          wsRef.current = null; // Clear the ref

          // Exponential backoff reconnection logic
          const maxReconnectDelay = 5000; // 5 seconds max
          const baseDelay = 500; // 0.5 seconds base
          const reconnectDelay = Math.min(
              maxReconnectDelay,
              baseDelay * Math.pow(1.5, reconnectAttemptsRef.current)
          );

          reconnectAttemptsRef.current++;
          if (!isProduction) console.log(`Config WebSocket: Attempting reconnect in ${reconnectDelay}ms (Attempt ${reconnectAttemptsRef.current})`);

          reconnectTimeoutRef.current = setTimeout(connectWebSocket, reconnectDelay);
      }
    };

    ws.onerror = (event) => {
      console.error('Config WebSocket: Error occurred:', event);
      setStatus('Error');
      // The onclose event will usually fire after an error, triggering reconnect logic there.
    };

  }, [isProduction]); // useCallback dependencies

  useEffect(() => {
    connectWebSocket(); // Initial connection attempt

    // Cleanup function
    return () => {
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      if (wsRef.current) {
        try {
          // Prevent reconnect logic during cleanup
          wsRef.current.onclose = null;
          wsRef.current.onerror = null;
          wsRef.current.close();
        } catch (e) { /* Ignore */ }
        wsRef.current = null;
      }
    };
  }, [connectWebSocket]); // useEffect depends on the stable connectWebSocket callback

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