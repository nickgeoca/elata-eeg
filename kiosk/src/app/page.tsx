'use client'; // Required for useState

import EegMonitor from '@/components/EegMonitor';
import { EegConfigProvider } from '@/components/EegConfig';
import { CommandWebSocketProvider } from '@/context/CommandWebSocketContext';

export default function Home() {
  return (
    <div className="flex flex-col min-h-screen bg-gray-900 text-white">
      <EegConfigProvider>
        <CommandWebSocketProvider>
          <div className="flex-grow">
            {/* EEG Monitor is now the main and only view */}
            <EegMonitor />
          </div>
        </CommandWebSocketProvider>
      </EegConfigProvider>
    </div>
  );
}
