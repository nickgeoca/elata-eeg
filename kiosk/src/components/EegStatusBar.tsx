'use client';

import React from 'react';

interface EegStatusBarProps {
  status: string;
  dataReceived: boolean;
  fps: number;
  packetsReceived: number;
}

export function EegStatusBar({ status, dataReceived, fps, packetsReceived }: EegStatusBarProps) {
  return (
    <div className="mb-2 text-gray-300 flex items-center">
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
    </div>
  );
}