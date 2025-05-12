import EegMonitor from '@/components/EegMonitor';
import { EegConfigProvider } from '@/components/EegConfig';
import EegChannelConfig from '@/components/EegChannelConfig';
import EegRecordingControls from '@/components/EegRecordingControls';
import { CommandWebSocketProvider } from '@/context/CommandWebSocketContext';

export default function Home() {
  return (
    <div className="min-h-screen">
      {/* Wrap components with the providers to share configuration and command state */}
      <EegConfigProvider>
        <CommandWebSocketProvider>
          <div className="container mx-auto p-4">
            <h1 className="text-2xl font-bold mb-4">EEG Monitoring System</h1>
            
            <div className="grid grid-cols-1 md:grid-cols-3 gap-6 mb-6">
              {/* Channel configuration component */}
              <div className="md:col-span-2">
                <EegChannelConfig />
              </div>
              
              {/* Recording controls component */}
              <div>
                <EegRecordingControls />
              </div>
            </div>
            
            {/* EEG monitor component */}
            <EegMonitor />
          </div>
        </CommandWebSocketProvider>
      </EegConfigProvider>
    </div>
  );
}
