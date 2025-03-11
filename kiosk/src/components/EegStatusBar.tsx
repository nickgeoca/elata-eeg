'use client';

import React, { useState, useEffect } from 'react';

interface EegStatusBarProps {
  status: string;
  dataReceived: boolean;
  fps: number;
  packetsReceived: number;
}

export function EegStatusBar({ status, dataReceived, fps, packetsReceived }: EegStatusBarProps) {
  const [recordingStatus, setRecordingStatus] = useState<{isRecording: boolean, message: string}>({
    isRecording: false,
    message: 'Not recording'
  });
  const [wsConnected, setWsConnected] = useState(false);

  // Connect to the command WebSocket to get recording status
  useEffect(() => {
    const ws = new WebSocket('ws://localhost:8080/command');
    
    ws.onopen = () => {
      setWsConnected(true);
      // Request initial status
      ws.send(JSON.stringify({ command: 'status' }));
    };
    
    ws.onmessage = (event) => {
      try {
        const response = JSON.parse(event.data);
        
        if (response.status === 'ok') {
          const isRecording = response.message.startsWith('Currently recording');
          setRecordingStatus({
            isRecording,
            message: response.message
          });
        }
      } catch (error) {
        console.error('Error parsing status response:', error);
      }
    };
    
    ws.onclose = () => {
      setWsConnected(false);
    };
    
    return () => {
      ws.close();
    };
  }, []);

  return (
    <div className="mb-2 text-gray-300 flex items-center flex-wrap">
      <div>Status: {status}</div>
      <span className="ml-4">
        FPS: {fps.toFixed(2)}
      </span>
      <div className="ml-4 flex items-center">
        Data:
        <span className={`ml-2 inline-block w-3 h-3 rounded-full ${dataReceived ? 'bg-green-500' : 'bg-red-500'}`}></span>
        <span className="ml-1">{dataReceived ? 'Receiving' : 'No data'}</span>
      </div>
      <div className="ml-4">
        Packets: {packetsReceived}
      </div>
      <div className="ml-4 flex items-center">
        Recording:
        <span className={`ml-2 inline-block w-3 h-3 rounded-full ${recordingStatus.isRecording ? 'bg-red-500 animate-pulse' : 'bg-gray-500'}`}></span>
        <span className="ml-1">{recordingStatus.isRecording ? 'Active' : 'Inactive'}</span>
      </div>
    </div>
  );
}