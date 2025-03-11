'use client';

import React, { useState, useEffect, useCallback } from 'react';

interface RecordingStatus {
  isRecording: boolean;
  filePath: string | null;
  message: string;
}

export function EegRecordingControls() {
  const [status, setStatus] = useState<RecordingStatus>({
    isRecording: false,
    filePath: null,
    message: 'Not recording'
  });
  const [connected, setConnected] = useState(false);
  const [wsRef, setWsRef] = useState<WebSocket | null>(null);

  // Connect to the command WebSocket
  useEffect(() => {
    const ws = new WebSocket('ws://localhost:8080/command');
    setWsRef(ws);
    
    ws.onopen = () => {
      console.log('Command WebSocket connected');
      setConnected(true);
    };
    
    ws.onmessage = (event) => {
      try {
        const response = JSON.parse(event.data);
        
        if (response.status === 'ok') {
          // Parse the message to determine recording status
          const isRecording = response.message.startsWith('Currently recording');
          let filePath = null;
          
          if (isRecording) {
            // Extract file path from message
            const match = response.message.match(/Currently recording to (.+)/);
            if (match && match[1]) {
              filePath = match[1];
            }
          }
          
          setStatus({
            isRecording,
            filePath,
            message: response.message
          });
        } else {
          console.error('Command error:', response.message);
        }
      } catch (error) {
        console.error('Error parsing command response:', error);
      }
    };
    
    ws.onclose = () => {
      console.log('Command WebSocket disconnected');
      setConnected(false);
    };
    
    ws.onerror = (error) => {
      console.error('Command WebSocket error:', error);
      setConnected(false);
    };
    
    return () => {
      ws.close();
    };
  }, []);

  // Send command to start recording
  const startRecording = useCallback(() => {
    if (wsRef && wsRef.readyState === WebSocket.OPEN) {
      wsRef.send(JSON.stringify({ command: 'start' }));
    }
  }, [wsRef]);

  // Send command to stop recording
  const stopRecording = useCallback(() => {
    if (wsRef && wsRef.readyState === WebSocket.OPEN) {
      wsRef.send(JSON.stringify({ command: 'stop' }));
    }
  }, [wsRef]);

  // Request status update
  const requestStatus = useCallback(() => {
    if (wsRef && wsRef.readyState === WebSocket.OPEN) {
      wsRef.send(JSON.stringify({ command: 'status' }));
    }
  }, [wsRef]);

  return (
    <div className="p-4 bg-gray-900 text-white rounded-lg mb-4">
      <h2 className="text-xl font-bold mb-2">Recording Controls</h2>
      
      <div className="mb-4">
        <div className="flex items-center mb-2">
          <div className="mr-2">Status:</div>
          <div className="flex items-center">
            <span className={`inline-block w-3 h-3 rounded-full mr-2 ${status.isRecording ? 'bg-red-500 animate-pulse' : 'bg-gray-500'}`}></span>
            <span>{status.message}</span>
          </div>
        </div>
        
        {status.filePath && (
          <div className="text-sm text-gray-400 mb-2 truncate">
            File: {status.filePath}
          </div>
        )}
      </div>
      
      <div className="flex space-x-2">
        <button
          onClick={startRecording}
          disabled={status.isRecording || !connected}
          className={`px-4 py-2 rounded-md ${
            status.isRecording || !connected
              ? 'bg-gray-700 text-gray-500 cursor-not-allowed'
              : 'bg-green-600 hover:bg-green-700 text-white'
          }`}
        >
          Start Recording
        </button>
        
        <button
          onClick={stopRecording}
          disabled={!status.isRecording || !connected}
          className={`px-4 py-2 rounded-md ${
            !status.isRecording || !connected
              ? 'bg-gray-700 text-gray-500 cursor-not-allowed'
              : 'bg-red-600 hover:bg-red-700 text-white'
          }`}
        >
          Stop Recording
        </button>
        
        <button
          onClick={requestStatus}
          disabled={!connected}
          className={`px-4 py-2 rounded-md ${
            !connected
              ? 'bg-gray-700 text-gray-500 cursor-not-allowed'
              : 'bg-blue-600 hover:bg-blue-700 text-white'
          }`}
        >
          Refresh Status
        </button>
      </div>
      
      <div className="mt-2 text-xs text-gray-500">
        {connected ? 'Connected to server' : 'Disconnected from server'}
      </div>
    </div>
  );
}