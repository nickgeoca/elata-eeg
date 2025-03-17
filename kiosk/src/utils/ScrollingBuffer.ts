'use client';

/**
 * ScrollingBuffer.ts
 *
 * Optimized fixed-size buffer for real-time scrolling display of EEG data.
 *
 * This implementation uses an Index-Based Rendering approach, which:
 * 1. Assigns each sample a specific index position
 * 2. Determines x-position based on the sample's index, not time
 * 3. Shifts the graph by a consistent amount each frame
 *
 * Key equations:
 * - Render offset shift per frame: offset_delta = S / F (where S = sample rate, F = frame rate)
 * - Sample position: x_i = x_right - i * (W / N) (where W = screen width, N = total samples)
 *
 * The renderOffset is used to create smooth scrolling between data arrivals:
 * - Incremented by offset_delta each frame to create consistent movement
 * - Represents the position offset in percentage of canvas width
 */
export class ScrollingBuffer {
  private buffer: Float32Array;
  private size: number = 0;
  private renderOffset: number = 0; // In percentage of canvas width
  
  constructor(private capacity: number) {
    this.buffer = new Float32Array(capacity);
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
  
  // Update the render offset by the specified delta
  updateRenderOffset(delta: number) {
    this.renderOffset += delta;
  }
  
  // Maintain render offset when new data arrives
  // This ensures smooth scrolling by not resetting the offset
  maintainRenderOffset() {
    // No reset, just continue with current renderOffset
    // This method exists for API compatibility with the old reset behavior
  }
  
  // Get the current render offset (in percentage of canvas width)
  getRenderOffset(): number {
    return this.renderOffset;
  }
  
  // Get data for rendering without creating new arrays
  getData(points: Float32Array) {
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
      // Use a fractional approach to ensure smooth leftward movement
      // This prevents jumps when new data arrives since we maintain consistent renderOffset
      const adjustedIndex = relativeIndex - (this.renderOffset % 1);
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