import Image from "next/image";
import EegMonitor from '@/components/EegMonitor';
import EegConfigDisplay, { EegConfigProvider } from '@/components/EegConfig';
import { EegRecordingControls } from '@/components/EegRecordingControls';

export default function Home() {
  return (
    <div className="min-h-screen p-8">
      <h1 className="text-3xl font-bold mb-6">EEG Kiosk</h1>
      
      {/* Wrap components with the provider to share configuration */}
      <EegConfigProvider>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <EegConfigDisplay />
          <EegRecordingControls />
        </div>
        <EegMonitor />
      </EegConfigProvider>
    </div>
  );
}
