'use client'; // Required for useState

import EegMonitor from '@/components/EegMonitor';
import { EegConfigProvider } from '@/components/EegConfig';
import { CommandWebSocketProvider } from '@/context/CommandWebSocketContext';

export default function Home() {
  return (
    <div className="min-h-screen bg-gray-900 text-white">
      <EegConfigProvider>
        <CommandWebSocketProvider>
          <div className="container mx-auto p-4">
            {/* EEG Monitor is now the main and only view */}
            <EegMonitor />
          </div>
        </CommandWebSocketProvider>
      </EegConfigProvider>
    </div>
  );
}
