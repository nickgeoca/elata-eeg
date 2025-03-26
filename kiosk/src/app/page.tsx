import EegMonitor from '@/components/EegMonitor';
import { EegConfigProvider } from '@/components/EegConfig';

export default function Home() {
  return (
    <div className="min-h-screen">
      {/* Wrap components with the provider to share configuration */}
      <EegConfigProvider>
        <EegMonitor />
      </EegConfigProvider>
    </div>
  );
}
