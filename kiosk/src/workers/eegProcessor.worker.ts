import { ResizableRingBuffer } from '../utils/ringBuffer';

let ringBuffer: ResizableRingBuffer | null = null;

self.onmessage = (event) => {
  const { type, payload } = event.data;

  switch (type) {
    case 'init':
      ringBuffer = new ResizableRingBuffer(payload.buffer);
      break;
    case 'data':
      if (ringBuffer) {
        const { samples, meta } = payload;
        const numMetaChannels = meta.channel_names.length;
        if (numMetaChannels === 0) return;

        const NCH = 1; // Assuming single channel for now
        if (NCH === 1) {
          const batch = new Float32Array(samples);
          ringBuffer.write(batch);
        } else {
          // De-interleaving logic for multi-channel data
          const batches: number[][] = Array.from({ length: NCH }, () => []);
          for (let i = 0; i < samples.length; i++) {
            const channelIndex = i % numMetaChannels;
            if (channelIndex < NCH) {
              batches[channelIndex].push(samples[i]);
            }
          }
          const batch = new Float32Array(batches[0]);
          ringBuffer.write(batch);
        }
      }
      break;
  }
};