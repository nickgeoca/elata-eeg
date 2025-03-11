import Image from "next/image";
import EegMonitor from '@/components/EegMonitor';
import EegConfigDisplay, { EegConfigProvider } from '@/components/EegConfig';

export default function Home() {
  return (
    <div className="min-h-screen p-8">
      <h1 className="text-3xl font-bold mb-6">EEG Kiosk</h1>
      
      {/* Wrap components with the provider to share configuration */}
      <EegConfigProvider>
        <EegConfigDisplay />
        <EegMonitor />
      </EegConfigProvider>
    </div>
  );
}
