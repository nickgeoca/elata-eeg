'use client';

/**
 * ScrollingBuffer.ts
 *
 * Optimized dynamic-size buffer for real-time scrolling display of EEG data.
 *
 * ## Global Time Approach
 *
 * This implementation uses a Global Time Approach, which:
 * 1. Records a timestamp when each data chunk arrives
 * 2. Calculates the render offset based on the time elapsed since that timestamp
 * 3. Eliminates the need for resets by always computing the offset fresh from a stable reference point
 *
 * ## Mathematical Foundation
 *
 * The key equations that drive the smooth scrolling behavior:
 *
 * 1. Render offset calculation based on time since last data chunk:
 *    offset = S * dt
 *    Where:
 *    - S = sample rate (Hz)
 *    - dt = time elapsed since last data chunk (seconds)
 *
 *    Example: With S=250Hz and dt=0.050s (50ms since last chunk):
 *    offset = 250 * 0.050 = 12.5 samples
 *
 * 2. Sample position calculation:
 *    x_i = x_right - i * (W / N)
 *    Where:
 *    - x_i = position of sample i
 *    - x_right = right edge position (1.0 in normalized coordinates)
 *    - i = sample index
 *    - W = screen width (normalized to 1.0)
 *    - N = total samples in buffer
 *
 *    Example: With N=500 samples:
 *    - Each sample occupies 0.2% of screen width (100%/500)
 *    - A shift based on global time ensures consistent behavior without resets
 *
 * ## Advantages of Global Time Approach
 *
 * - Eliminates flickering by avoiding abrupt resets
 * - Avoids floating-point accumulation errors
 * - Provides more consistent and predictable behavior
 * - Scales well with different sample rates or chunk sizes
 * 
 * ## Dynamic Buffer Size
 * 
 * The buffer size is now dynamically adjusted based on:
 * - Screen width (pixels)
 * - Sample rate (Hz)
 * - Window duration (ms)
 * 
 * This ensures optimal memory usage and rendering performance across different screen sizes.
 */
export class ScrollingBuffer {
  private buffer: Float32Array;
  private size: number = 0;
  private sampleRate: number = 250; // Default sample rate, can be updated
  
  // New properties for Global Time Approach
  private lastDataChunkTime: number = performance.now();
  private chunkSize: number = 32; // Number of samples per chunk
  
  // We'll keep renderOffset as a computed value rather than a stored property
  
  constructor(private capacity: number, sampleRate?: number) {
    this.buffer = new Float32Array(capacity);
    if (sampleRate) {
      this.sampleRate = sampleRate;
    }
    
    // Log buffer creation
    console.log(`[ScrollingBuffer] Created new buffer with capacity: ${capacity}, sample rate: ${this.sampleRate}Hz`);
  }
  
  // Add a new value to the buffer
  push(value: number) {
    // Log occasionally when data is pushed (for debugging)
    if (Math.random() < 0.001) {
      console.log(`[ScrollingBuffer] Pushing value: ${value}, current size: ${this.size}`);
    }
    
    // If buffer is full, shift everything left
    if (this.size === this.capacity) {
      // Shift all values left by one position
      this.buffer.copyWithin(0, 1);
      // Add new value at the end
      this.buffer[this.capacity - 1] = value;
    } else {
      // Buffer not full yet, just add to the end
      this.buffer[this.size] = value;
      this.size++;
    }
    
    // If this is the first sample in a new chunk, update the timestamp
    // This assumes push() is called sequentially for each sample in the chunk
    if (this.size % this.chunkSize === 1) {
      this.updateDataChunkTime();
    }
  }
  
  // Update the sample rate if needed
  setSampleRate(sampleRate: number) {
    this.sampleRate = sampleRate;
  }
  
  // Get the current sample rate
  getSampleRate(): number {
    return this.sampleRate;
  }
  
  // Set the chunk size (number of samples per data chunk)
  setChunkSize(size: number) {
    this.chunkSize = size;
  }
  
  // Update the timestamp when a new data chunk arrives
  updateDataChunkTime() {
    this.lastDataChunkTime = performance.now();
    
    // Log occasionally for debugging
    if (Math.random() < 0.01) {
      console.log(`[ScrollingBuffer] Updated data chunk time: ${this.lastDataChunkTime}`);
    }
  }
  
  // This method replaces the previous maintainRenderOffset method
  // It should be called when a new data chunk arrives
  notifyNewDataChunk() {
    this.updateDataChunkTime();
  }
  
  // Calculate the current render offset based on time since last data chunk
  // This replaces the previous updateRenderOffsetWithTime method
  calculateRenderOffset(): number {
    const now = performance.now();
    const timeSinceLastChunk = (now - this.lastDataChunkTime) / 1000; // in seconds
    
    // Calculate offset based on time since last chunk and sample rate
    // This gives a smooth, continuous movement
    const samplesElapsed = timeSinceLastChunk * this.sampleRate;
    
    // Log occasionally for debugging
    if (Math.random() < 0.005) {
      console.log(`[ScrollingBuffer] Time since last chunk: ${timeSinceLastChunk.toFixed(4)}s, samples elapsed: ${samplesElapsed.toFixed(2)}`);
    }
    
    return samplesElapsed;
  }
  
  // Get the current render offset (in samples)
  getRenderOffset(): number {
    return this.calculateRenderOffset();
  }
  
  // Get data for rendering without creating new arrays
  getData(points: Float32Array) {
    if (this.size === 0) {
      return 0;
    }
    
    // Check if we might exceed buffer bounds and log warning
    if (this.size * 2 > points.length) {
      console.warn(`[ScrollingBuffer] Buffer overflow risk: size=${this.size}, points.length=${points.length}, capacity=${this.capacity}`);
      // Limit size to prevent buffer overflow
      this.size = Math.floor(points.length / 2);
    }
    
    // Calculate the current render offset
    const renderOffset = this.calculateRenderOffset();
    
    // Fill the points array with x,y pairs using index-based approach
    for (let i = 0; i < this.size; i++) {
      // For traditional EEG style (right-to-left):
      // Map newest points (higher indices) to higher x values (right side)
      const relativeIndex = this.size - 1 - i;
      
      // Apply renderOffset for smooth scrolling (in samples)
      const adjustedIndex = relativeIndex + renderOffset;
      const normalizedX = adjustedIndex / (this.capacity - 1);
      
      points[i * 2] = normalizedX;
      
      // y = normalized value
      points[i * 2 + 1] = this.buffer[i];
    }
    
    return this.size; // Return the number of points added
  }
  
  // Get the capacity of this buffer
  getCapacity(): number {
    return this.capacity;
  }
  
  // Get the current size of the buffer (number of data points)
  getSize(): number {
    return this.size;
  }
  
  // NEW: Update the capacity of the buffer
  // This allows dynamic resizing based on screen width
  updateCapacity(newCapacity: number): void {
    if (newCapacity === this.capacity) {
      return; // No change needed
    }
    
    // Log capacity change
    console.log(`[ScrollingBuffer] Updating capacity: ${this.capacity} -> ${newCapacity}`);
    
    // Create new buffer with updated capacity
    const newBuffer = new Float32Array(newCapacity);
    
    // Copy existing data to new buffer
    if (this.size > 0) {
      // If new buffer is smaller, only copy what fits
      const copySize = Math.min(this.size, newCapacity);
      
      if (this.size <= newCapacity) {
        // If new buffer is larger or same size, copy all data
        newBuffer.set(this.buffer.subarray(0, copySize));
      } else {
        // If new buffer is smaller, copy the most recent data
        // (last 'newCapacity' elements from the current buffer)
        newBuffer.set(this.buffer.subarray(this.size - newCapacity, this.size));
      }
      
      // Update size
      this.size = copySize;
    }
    
    // Update buffer and capacity
    this.buffer = newBuffer;
    this.capacity = newCapacity;
  }
}