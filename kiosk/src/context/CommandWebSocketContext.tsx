'use client';

import { createContext, useState, useEffect, useContext } from 'react';

interface CommandWebSocketContextType {
  ws: WebSocket | null;
  wsConnected: boolean;
  startRecording: () => void;
  stopRecording: () => void;
  recordingStatus: string;
  recordingFilePath: string | null;
}

const CommandWebSocketContext = createContext<CommandWebSocketContextType | undefined>(
  undefined
);

export const CommandWebSocketProvider = ({
  children,
}: {
  children: React.ReactNode;
}) => {
  const [ws, setWs] = useState<WebSocket | null>(null);
  const [wsConnected, setWsConnected] = useState(false);
  const [recordingStatus, setRecordingStatus] = useState('Not recording');
  const [recordingFilePath, setRecordingFilePath] = useState<string | null>(null);

  useEffect(() => {
    const wsHost = window.location.hostname;
    const newWs = new WebSocket(`wss://${wsHost}:8080/command`); // Use wss:// for secure connection

    newWs.onopen = () => {
      console.log('Command WebSocket connected');
      setWsConnected(true);
    };

    newWs.onmessage = (event) => {
      try {
        const response = JSON.parse(event.data);

        if (response.status === 'ok') {
          const recording = response.message.startsWith('Currently recording');
          let filePath = null;

          if (recording) {
            const match = response.message.match(/Currently recording to (.+)/);
            if (match && match[1]) {
              filePath = match[1];
            }
          }

          setRecordingStatus(response.message);
          setRecordingFilePath(filePath);
        } else {
          console.error('Command error:', response.message);
        }
      } catch (error) {
        console.error('Error parsing command response:', error);
      }
    };

    newWs.onclose = () => {
      console.log('Command WebSocket disconnected');
      setWsConnected(false);
    };

    newWs.onerror = (error) => {
      console.error('Command WebSocket error:', error);
      setWsConnected(false);
    };

    setWs(newWs);

    return () => {
      newWs.close();
    };
  }, []);

  const startRecording = () => {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ command: 'start' }));
    }
  };

  const stopRecording = () => {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ command: 'stop' }));
    }
  };

  const value: CommandWebSocketContextType = {
    ws,
    wsConnected,
    startRecording,
    stopRecording,
    recordingStatus,
    recordingFilePath,
  };

  return (
    <CommandWebSocketContext.Provider value={value}>
      {children}
    </CommandWebSocketContext.Provider>
  );
};

export const useCommandWebSocket = () => {
  const context = useContext(CommandWebSocketContext);
  if (!context) {
    throw new Error(
      'useCommandWebSocket must be used within a CommandWebSocketProvider'
    );
  }
  return context;
};