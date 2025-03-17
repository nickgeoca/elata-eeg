'use client';

/**
 * EegRenderer.tsx
 *
 * This component handles rendering EEG data using WebGL for efficient visualization.
 *
 * This implementation uses a Time-Based Rendering approach, which:
 * 1. Assigns each sample a specific index position
 * 2. Determines x-position based on the sample's index, not time
 * 3. Shifts the graph based on actual elapsed time between frames
 * 4. Eliminates drift by accounting for actual frame timing
 *
 * The render offset is expressed as a percentage of canvas width, allowing
 * for smooth scrolling that's consistent regardless of screen dimensions or
 * frame rate fluctuations.
 */

import { useEffect, useRef } from 'react';
import REGL from 'regl';
import { ScrollingBuffer } from '../utils/ScrollingBuffer';
import { getChannelColor } from '../utils/colorUtils';
import { VOLTAGE_TICKS, TIME_TICKS, WINDOW_DURATION } from '../utils/eegConstants';
import { FIXED_SAMPLE_RATE, TARGET_FRAME_RATE, RENDER_OFFSET_SHIFT_PER_FRAME } from '../components/EegConfig';

interface EegRendererProps {
  canvasRef: React.RefObject<HTMLCanvasElement | null>;
  dataRef: React.MutableRefObject<ScrollingBuffer[]>;
  config: any;
  latestTimestampRef: React.MutableRefObject<number>;
  debugInfoRef: React.MutableRefObject<{
    lastPacketTime: number;
    packetsReceived: number;
    samplesProcessed: number;
  }>;
  voltageScaleFactor?: number; // Default to 1.0 if not provided
}

export function EegRenderer({
  canvasRef,
  dataRef,
  config,
  latestTimestampRef,
  debugInfoRef,
  voltageScaleFactor = 5.0
}: EegRendererProps) {
  const reglRef = useRef<any>(null);
  const pointsArraysRef = useRef<Float32Array[]>([]);
  const lastFrameTimeRef = useRef(Date.now());
  const frameCountRef = useRef(0);
  const lastFpsLogTimeRef = useRef(Date.now());
  const isProduction = process.env.NODE_ENV === 'production';

  // Pre-allocate point arrays for each channel to avoid GC
  useEffect(() => {
    // Use channel count from config or default to 4
    const channelCount = config?.channels?.length || 4;
    const windowSize = Math.ceil(((config?.sample_rate || 250) * WINDOW_DURATION) / 1000);
    
    // Only recreate arrays if needed (channel count changed or not initialized)
    if (pointsArraysRef.current.length !== channelCount) {
      pointsArraysRef.current = Array(channelCount).fill(null).map(() =>
        new Float32Array(windowSize * 2)
      );
      
      if (!isProduction) {
        console.log(`Initialized ${channelCount} point arrays with size ${windowSize}`);
      }
    }
  }, [config, isProduction]);

  // WebGL setup
  useEffect(() => {
    if (!canvasRef.current) return;
    
    if (!isProduction) {
      console.log("Initializing WebGL renderer");
    }
    
    // Initialize regl
    const regl = REGL({
      canvas: canvasRef.current,
      attributes: {
        antialias: false,
        depth: false,
        preserveDrawingBuffer: true
      }
    });
    
    reglRef.current = regl;
    
    // Get FPS from config or use default
    const renderFps = config?.fps ?? (config?.sample_rate / config?.batch_size) ?? 60;
    
    if (!isProduction) {
      console.log(`Setting render FPS to ${renderFps}`);
    }
    
    // Create WebGL command for drawing the grid
    const drawGrid = regl({
      frag: `
        precision mediump float;
        uniform vec4 color;
        void main() {
          gl_FragColor = color;
        }
      `,
      vert: `
        precision mediump float;
        attribute vec2 position;
        void main() {
          gl_Position = vec4(position, 0, 1);
        }
      `,
      attributes: {
        position: regl.prop('points')
      },
      uniforms: {
        color: regl.prop('color')
      },
      primitive: 'lines',
      count: regl.prop('count'), // Add count property
      blend: {
        enable: true,
        func: {
          srcRGB: 'src alpha',
          srcAlpha: 1,
          dstRGB: 'one minus src alpha',
          dstAlpha: 1
        }
      },
      depth: { enable: false }
    });
    
    // Create WebGL command for drawing the EEG lines
    const drawLines = regl({
      frag: `
        precision mediump float;
        uniform vec4 color;
        void main() {
          gl_FragColor = color;
        }
      `,
      vert: `
        precision mediump float;
        attribute vec2 position;
        uniform float yOffset;
        uniform float yScale;
        void main() {
          // Convert x from [0,1] to [-1,1] (right to left, traditional EEG style)
          // Map 0 to 1 (left of screen) and 1 to -1 (right of screen)
          float x = 1.0 - position.x * 2.0;
          
          // Scale y value and apply offset
          // Convert from [min,max] to [-1,1] with channel offset
          float y = position.y * yScale + yOffset;
          
          gl_Position = vec4(x, y, 0, 1);
        }
      `,
      attributes: {
        position: regl.prop('points')
      },
      uniforms: {
        color: regl.prop('color'),
        yOffset: regl.prop('yOffset'),
        yScale: regl.prop('yScale')
      },
      primitive: 'line strip',
      lineWidth: 1.0, // Minimum allowed line width in REGL (must be between 1 and 32)
      count: regl.prop('count'),
      blend: {
        enable: true,
        func: {
          srcRGB: 'src alpha',
          srcAlpha: 1,
          dstRGB: 'one minus src alpha',
          dstAlpha: 1
        }
      },
      depth: { enable: false }
    });
    
    // Create grid lines
    const gridLines: number[][] = [];
    
    // Vertical time lines
    TIME_TICKS.forEach(time => {
      const x = 1.0 - (time / (WINDOW_DURATION / 1000));
      gridLines.push(
        [x * 2 - 1, -1], // Bottom
        [x * 2 - 1, 1]   // Top
      );
    });
    
    // Horizontal voltage lines for each channel
    const channelCount = config?.channels?.length || 4;
    for (let ch = 0; ch < channelCount; ch++) {
      // Use a linear distribution from top to bottom
      let chOffset = channelCount <= 1
        ? 0
        : -0.9 + (ch / (channelCount - 1)) * 1.8;
      
      VOLTAGE_TICKS.forEach(voltage => {
        // Normalize voltage to [-1, 1] range within channel space
        // Scale based on channel count to prevent overlap with many channels
        const baseScaleFactor = Math.min(0.1, 0.3 / channelCount);
        const scaleFactor = baseScaleFactor * voltageScaleFactor;
        const normalizedVoltage = (voltage / 3) * scaleFactor;
        const y = chOffset + normalizedVoltage;
        
        gridLines.push(
          [-1, y], // Left
          [1, y]   // Right
        );
      });
    }
    
    // Render function with time-based scrolling
    const render = () => {
      // Get current time for logging and other operations
      const now = Date.now();
      
      // Calculate delta time for time-based scrolling
      // This is the key to smooth scrolling - we use the actual elapsed time
      // rather than assuming a fixed frame rate
      const deltaTime = (now - lastFrameTimeRef.current) / 1000; // in seconds
      lastFrameTimeRef.current = now;
      
      // Increment frame counter for FPS calculation
      frameCountRef.current++;
      
      // Calculate and log actual FPS every second
      if (now - lastFpsLogTimeRef.current > 1000) {
        const elapsedSec = (now - lastFpsLogTimeRef.current) / 1000;
        const actualFps = frameCountRef.current / elapsedSec;
        console.log(`Actual render FPS: ${actualFps.toFixed(2)} (${frameCountRef.current} frames in ${elapsedSec.toFixed(2)}s)`);
        frameCountRef.current = 0;
        lastFpsLogTimeRef.current = now;
      }
      
      // Debug: Log render call more frequently during development
      if (!isProduction && Math.random() < 0.05) {
        console.log(`Render function called at ${new Date(now).toISOString()}, dt=${deltaTime.toFixed(4)}s`);
      }
      
      // Get sample rate from config or use default
      const sampleRate = config?.sample_rate || FIXED_SAMPLE_RATE;
      
      // Update render offsets in all buffers using time-based approach
      // This shifts the graph left based on actual elapsed time
      // The renderOffset is maintained when new data arrives for smooth animation
      const channelCount = config?.channels?.length || 4;
      
      // Log the time-based approach more frequently during development
      if (!isProduction && Math.random() < 0.1) {
        console.log(`Time-based scrolling: dt=${deltaTime.toFixed(4)}s, expected samples shift: ${(sampleRate * deltaTime).toFixed(2)}`);
      }
      
      for (let ch = 0; ch < channelCount; ch++) {
        if (dataRef.current[ch]) {
          // Update sample rate in buffer if needed
          if (dataRef.current[ch].getSampleRate() !== sampleRate) {
            dataRef.current[ch].setSampleRate(sampleRate);
          }
          
          const oldRenderOffset = dataRef.current[ch].getRenderOffset();
          
          // Use time-based update instead of fixed frame rate
          dataRef.current[ch].updateRenderOffsetWithTime(deltaTime);
          
          // Log renderOffset updates more frequently for the first channel
          if (!isProduction && ch === 0 && Math.random() < 0.05) {
            console.log(`Updated renderOffset for channel ${ch}: ${oldRenderOffset.toFixed(2)} -> ${dataRef.current[ch].getRenderOffset().toFixed(2)}, dt=${deltaTime.toFixed(4)}s`);
          }
        }
      }
      
      // Use a relative time window based on the latest data timestamp
      const latestTimestamp = latestTimestampRef.current;
      const startTime = latestTimestamp - WINDOW_DURATION;
      const endTime = latestTimestamp;
      const debugInfo = debugInfoRef.current;
      
      // Only log in development mode and very infrequently
      if (!isProduction && Math.random() < 0.01) {
        console.log(`Time window: ${new Date(startTime).toISOString()} to ${new Date(endTime).toISOString()}`);
        console.log(`Sample rate: ${sampleRate} Hz, time-based scrolling active`);
        
        // Log buffer and renderOffset status for each channel
        for (let ch = 0; ch < channelCount; ch++) {
          if (dataRef.current[ch]) {
            const buffer = dataRef.current[ch];
            const renderOffset = buffer.getRenderOffset();
            console.log(`Channel ${ch}: renderOffset=${renderOffset.toFixed(2)}, bufferSize=${buffer.getSize()}, capacity=${buffer.getCapacity()}`);
          }
        }
      }
      
      // Clear the canvas
      regl.clear({
        color: [0.1, 0.1, 0.2, 1],
        depth: 1
      });
      
      // Draw grid
      drawGrid({
        points: gridLines,
        color: [0.2, 0.2, 0.2, 0.8],
        count: gridLines.length
      });
      
      // Track if any data was drawn
      let totalPointsDrawn = 0;
      
      // Draw each channel - always draw all channels together
      for (let ch = 0; ch < channelCount; ch++) {
        if (!dataRef.current[ch]) continue;
        const buffer = dataRef.current[ch];
        
        const points = pointsArraysRef.current[ch];
        
        // Get data points for this channel
        const count = buffer.getData(points);
        totalPointsDrawn += count;
        
        // Minimal logging in production
        if (!isProduction && ch === 0 && debugInfo.packetsReceived > 0 && now - debugInfo.lastPacketTime > 2000) {
          console.log(`Channel ${ch}: ${count} points`);
        }
        
        // Get the render offset for logging
        const renderOffset = buffer.getRenderOffset();
        
        // Always draw the channel data if we have points, regardless of render offset
        // This ensures continuous rendering as long as there's data in the buffer
        if (count > 0) {
          // Use the same linear distribution as the grid lines
          // This ensures consistent spacing between grid lines and EEG data
          const yOffset = channelCount <= 1
            ? 0
            : -0.9 + (ch / (channelCount - 1)) * 1.8;
          // Check if we might exceed buffer bounds and log warning
          const pointsLength = points.length;
          const neededLength = count * 2;
          
          // Use a safe count to prevent buffer overflow
          let safeCount = count;
          
          if (neededLength > pointsLength) {
            console.warn(`[EegRenderer] Buffer size issue: channel=${ch}, count=${count}, needed=${neededLength}, available=${pointsLength}`);
            // Adjust count to prevent buffer overflow
            safeCount = Math.floor(pointsLength / 2);
          }
          
          // Draw the channel data
          drawLines({
            points: points.subarray(0, safeCount * 2),
            count: safeCount,
            color: getChannelColor(ch),
            yOffset: yOffset,
            yScale: Math.min(0.1, 0.3 / channelCount) * voltageScaleFactor // Scale based on channel count and user-defined scale factor
          });
          
          // Log rendering status in development mode more frequently
          if (!isProduction && Math.random() < 0.02) {
            console.log(`Rendering channel ${ch}: renderOffset=${renderOffset.toFixed(2)}, bufferSize=${buffer.getSize()}, count=${count}, using full offset for continuous scrolling`);
          }
        } else if (count === 0 && !isProduction && Math.random() < 0.02) {
          // Log when we're not rendering because there's no data
          console.log(`No data to render for channel ${ch}: bufferSize=${buffer.getSize()}, renderOffset=${renderOffset.toFixed(2)}`);
        }
      }
      
      // Log data status every 2 seconds in development mode
      if (!isProduction && now - debugInfo.lastPacketTime > 2000) {
        console.log(`Points drawn: ${totalPointsDrawn}, Packets: ${debugInfo.packetsReceived}`);
        debugInfo.lastPacketTime = now;
      }
      
      // Always log when no points were drawn (potential issue)
      if (!isProduction && totalPointsDrawn === 0 && Math.random() < 0.05) {
        console.warn(`No points drawn in this frame! Check if data is available.`);
      }
    };
    
    // Set up rendering interval based on FPS
    const frameInterval = 1000 / renderFps;
    const renderIntervalId = setInterval(render, frameInterval);
    
    // Initial render
    render();
    
    return () => {
      clearInterval(renderIntervalId);
      regl.destroy();
    };
  }, [canvasRef, config, dataRef, latestTimestampRef, debugInfoRef, voltageScaleFactor]);

  return null;
}
