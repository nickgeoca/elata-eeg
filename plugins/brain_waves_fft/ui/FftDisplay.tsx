import React from 'react';

interface FftData {
  // Define the structure of the FFT data packet
  // This should match the FftPacket in the backend
  psd_packets: { channel: number; psd: number[] }[];
  fft_config: {
    fft_size: number;
    sample_rate: number;
    window_function: string;
  };
}

interface FftDisplayProps {
  isActive: boolean;
  data: FftData | null;
}

const FftDisplay: React.FC<FftDisplayProps> = ({ isActive, data }) => {
  if (!isActive || !data) {
    return <div>No FFT data available</div>;
  }

  // Basic rendering of the FFT data
  // This will be replaced with a proper visualization
  return (
    <div>
      <h2>FFT Power Spectral Density</h2>
      <p>Window: {data.fft_config.window_function}</p>
      <p>FFT Size: {data.fft_config.fft_size}</p>
      <p>Sample Rate: {data.fft_config.sample_rate} Hz</p>
      {data.psd_packets.map((packet) => (
        <div key={packet.channel}>
          <h3>Channel {packet.channel}</h3>
          <pre>{JSON.stringify(packet.psd, null, 2)}</pre>
        </div>
      ))}
    </div>
  );
};

export default FftDisplay;