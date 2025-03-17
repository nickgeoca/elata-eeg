'use client';

/**
 * ScrollingBuffer.ts
 *
 * Optimized fixed-size buffer for real-time scrolling display of EEG data.
 *
 * ## Time-Based Rendering Approach
 *
 * This implementation uses a Time-Based Rendering approach, which:
 * 1. Assigns each sample a specific index position in the buffer
 * 2. Determines x-position based on the sample's index, not time
 * 3. Shifts the graph based on actual elapsed time between frames
 *
 * ## Mathematical Foundation
 *
 * The key equations that drive the smooth scrolling behavior:
 *
 * 1. Render offset shift based on elapsed time:
 *    offset_delta = S * dt
 *    Where:
 *    - S = sample rate (Hz)
 *    - dt = actual time elapsed since last frame (seconds)
 *
 *    Example: With S=250Hz and dt=0.0167s (≈60fps):
 *    offset_delta = 250 * 0.0167 = 4.175 samples per frame
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
 *    - A shift based on actual time ensures accurate scrolling regardless of frame timing
 *
 * ## Render Offset Mechanism
 *
 * The renderOffset is used to create smooth scrolling between data arrivals:
 * - Incremented based on actual elapsed time to create consistent movement
 * - Represents the position offset in percentage of canvas width
 * - Reset to zero when new data arrives to maintain proper alignment
 *
 * IMPORTANT: Using actual elapsed time ensures smooth scrolling even when frame rates
 * fluctuate or don't exactly match the expected rate (e.g., 59.94Hz vs 60Hz).
 */
export class ScrollingBuffer {
  private buffer: Float32Array;
  private size: number = 0;
  private renderOffset: number = 0; // In percentage of canvas width
  private sampleRate: number = 250; // Default sample rate, can be updated
  private pendingReset: boolean = false; // Flag to indicate a pending reset
  
  constructor(private capacity: number, sampleRate?: number) {
    this.buffer = new Float32Array(capacity);
    if (sampleRate) {
      this.sampleRate = sampleRate;
    }
  }
  
  // Add a new value to the buffer
  push(value: number) {
    // Log occasionally when data is pushed (for debugging)
    if (Math.random() < 0.001) {
      console.log(`[ScrollingBuffer] Pushing value: ${value}, current size: ${this.size}, renderOffset: ${this.renderOffset.toFixed(2)}`);
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
  }
  
  // Update the sample rate if needed
  setSampleRate(sampleRate: number) {
    this.sampleRate = sampleRate;
  }
  
  // Get the current sample rate
  getSampleRate(): number {
    return this.sampleRate;
  }
  
  // Update the render offset based on actual elapsed time (in seconds)
  updateRenderOffsetWithTime(elapsedTimeSec: number) {
    // Apply any pending resets
    this.applyPendingReset();
    
    // Calculate samples to shift based on elapsed time and sample rate
    // samples = sampleRate * time
    const samplesShift = this.sampleRate * elapsedTimeSec;
    this.renderOffset += samplesShift;
    
    // Log occasionally for debugging
    if (Math.random() < 0.005) {
      console.log(`[ScrollingBuffer] Time-based update: ${elapsedTimeSec.toFixed(4)}s, shift: ${samplesShift.toFixed(2)} samples, new offset: ${this.renderOffset.toFixed(2)}`);
    }
  }
  
  // Legacy method for compatibility - prefer using updateRenderOffsetWithTime
  updateRenderOffset(delta: number) {
    // Apply any pending reset before updating the offset
    this.applyPendingReset();
    
    this.renderOffset += delta;
  }
  
  // Request a reset of render offset when new data arrives
  // This ensures the graph fits within the canvas when new data comes in
  // but prevents visual jumps by applying the reset at the start of the next render cycle
  maintainRenderOffset() {
    // Instead of immediately resetting, flag for reset on next frame
    this.pendingReset = true;
    
    // Log occasionally for debugging
    if (Math.random() < 0.01) {
      console.log(`[ScrollingBuffer] Requested renderOffset reset (will apply on next frame)`);
    }
  }
  
  // Apply any pending reset before updating the render offset
  private applyPendingReset() {
    if (this.pendingReset) {
      this.renderOffset = 0;
      this.pendingReset = false;
      
      // Log occasionally for debugging
      if (Math.random() < 0.01) {
        console.log(`[ScrollingBuffer] Applied renderOffset reset to zero`);
      }
    }
  }
  
  // Get the current render offset (in percentage of canvas width)
  getRenderOffset(): number {
    return this.renderOffset;
  }
  
  // Get data for rendering without creating new arrays
  getData(points: Float32Array) {
    // Apply any pending reset before rendering
    this.applyPendingReset();
    
    if (this.size === 0) {
      // Log a warning if the buffer is empty (only occasionally to avoid spam)
      if (Math.random() < 0.01) {
        console.warn(`[ScrollingBuffer] Buffer is empty! renderOffset=${this.renderOffset.toFixed(2)}`);
      }
      return 0;
    }
    
    // Check if we might exceed buffer bounds and log warning
    if (this.size * 2 > points.length) {
      console.warn(`[ScrollingBuffer] Buffer overflow risk: size=${this.size}, points.length=${points.length}, capacity=${this.capacity}`);
      // Limit size to prevent buffer overflow
      this.size = Math.floor(points.length / 2);
    }
    
    // Fill the points array with x,y pairs using index-based approach
    for (let i = 0; i < this.size; i++) {
      // For traditional EEG style (right-to-left):
      // Map newest points (higher indices) to higher x values (right side)
      // This ensures consistent behavior between initial fill and steady state
      const relativeIndex = this.size - 1 - i;
      
      // Apply renderOffset for smooth scrolling (in percentage of canvas width)
      // Use the full render offset value to ensure continuous scrolling between data arrivals
      // This creates a smooth leftward movement at the target frame rate
      const adjustedIndex = relativeIndex + this.renderOffset;
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
}