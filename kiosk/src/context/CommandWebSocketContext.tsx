'use client';

import { createContext, useState, useEffect, useContext, useMemo, useCallback, useRef } from 'react';

interface CommandWebSocketContextType {
  ws: WebSocket | null;
  wsConnected: boolean;
  startRecording: () => void;
  stopRecording: () => void;
  sendPowerlineFilterCommand: (value: number | null) => void; // Added
  recordingStatus: string;
  recordingFilePath: string | null;
  isStartRecordingPending: boolean;
  recordingError: string | null;
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
  const [isStartRecordingPending, setIsStartRecordingPending] = useState(false);
  const [recordingError, setRecordingError] = useState<string | null>(null);
  const timeoutRef = useRef<NodeJS.Timeout | null>(null);

  useEffect(() => {
    const wsHost = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
    const wsProtocol = typeof window !== 'undefined' && window.location.protocol === 'https:' ? 'wss' : 'ws';
    const newWs = new WebSocket(`${wsProtocol}://${wsHost}:8080/command`);
 
    newWs.onopen = () => {
      console.log('[Command] WebSocket connected.');
      setWsConnected(true);
    };

    newWs.onmessage = (event) => {
      console.log(`[Command] Received message:`, event.data);
      try {
        const response = JSON.parse(event.data);

        if (response.status === 'ok') {
          const isRecording = response.message.startsWith('Currently recording');
          const isStopped = response.message.includes('Recording stopped');
          const failedToStart = response.message.includes('Failed to start recording');
          const isCommandSent = response.message.includes('command sent');
          let filePath = null;

          console.log(`[Command] Processing message: "${response.message}", isRecording: ${isRecording}, pending: ${isStartRecordingPending}`);

          if (isRecording) {
            const match = response.message.match(/Currently recording to (.+)/);
            if (match && match[1]) {
              filePath = match[1];
              console.log(`[Command] Recording started. File path: ${filePath}`);
            }
          } else if (isStopped) {
            console.log(`[Command] Recording stopped.`);
          }

          // Only update status for non-command-sent messages
          if (!isCommandSent) {
            setRecordingStatus(response.message);
            setRecordingFilePath(filePath);
          }
          setRecordingError(null); // Clear any previous errors

          // Clear pending state ONLY for definitive recording status (not command acknowledgments)
          if (isRecording || failedToStart || isStopped) {
            console.log(`[Command] Clearing pending state due to: isRecording=${isRecording}, failedToStart=${failedToStart}, isStopped=${isStopped}`);
            setIsStartRecordingPending(prev => {
              if (prev) {
                console.timeEnd('startRecordingCommand');
                // Clear timeout when recording successfully starts
                if (timeoutRef.current) {
                  clearTimeout(timeoutRef.current);
                  timeoutRef.current = null;
                }
                return false;
              }
              return prev;
            });
          }
        } else {
          console.error('[Command] Received error:', response.message);
          setRecordingError(response.message);
          setIsStartRecordingPending(prev => {
            if (prev) {
              console.timeEnd('startRecordingCommand');
              // Clear timeout on error
              if (timeoutRef.current) {
                clearTimeout(timeoutRef.current);
                timeoutRef.current = null;
              }
              return false;
            }
            return prev;
          });
        }
      } catch (error) {
        console.error('[Command] Error parsing response JSON:', error);
      }
    };

    newWs.onclose = () => {
      console.log('[Command] WebSocket disconnected.');
      setWsConnected(false);
    };

    newWs.onerror = (error) => {
      console.error('[Command] WebSocket error:', error);
      setWsConnected(false);
    };

    setWs(newWs);

    return () => {
      newWs.close();
      // Clean up timeout on unmount
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
        timeoutRef.current = null;
      }
    };
  }, []);

  const startRecording = useCallback(() => {
    if (ws && ws.readyState === WebSocket.OPEN && !isStartRecordingPending) {
      setIsStartRecordingPending(true);
      console.log('[Command] Sending "start" command...');
      console.time('startRecordingCommand');
      ws.send(JSON.stringify({ command: 'start' }));
      
      // Clear any existing timeout
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
      
      // Add timeout protection - clear pending state after 10 seconds if no response
      timeoutRef.current = setTimeout(() => {
        setIsStartRecordingPending(prev => {
          if (prev) {
            console.warn('[Command] Start recording timeout - clearing pending state');
            setRecordingError('Recording start timeout - please try again');
            timeoutRef.current = null;
            return false;
          }
          return prev;
        });
      }, 10000);
    }
  }, [ws, isStartRecordingPending]);

  const stopRecording = useCallback(() => {
    if (ws && ws.readyState === WebSocket.OPEN) {
      console.log('[Command] Sending "stop" command...');
      ws.send(JSON.stringify({ command: 'stop' }));
    }
  }, [ws]);

  const sendPowerlineFilterCommand = useCallback((value: number | null) => {
    if (ws && ws.readyState === WebSocket.OPEN) {
      console.log(`[Command] Sending "set_powerline_filter" command with value: ${value}`);
      ws.send(JSON.stringify({ command: 'set_powerline_filter', value: value }));
    } else {
      console.warn('[Command] WebSocket not open, cannot send "set_powerline_filter" command.');
    }
  }, [ws]);

  const value = useMemo(() => ({
    ws,
    wsConnected,
    startRecording,
    stopRecording,
    sendPowerlineFilterCommand, // Added
    recordingStatus,
    recordingFilePath,
    isStartRecordingPending,
    recordingError,
  }), [ws, wsConnected, startRecording, stopRecording, sendPowerlineFilterCommand, recordingStatus, recordingFilePath, isStartRecordingPending, recordingError]);

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