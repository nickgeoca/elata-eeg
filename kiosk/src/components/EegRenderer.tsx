'use client';

import { useEffect, useRef } from 'react';
import REGL from 'regl';
import { ScrollingBuffer } from '../utils/ScrollingBuffer';
import { getChannelColor } from '../utils/colorUtils';
import { VOLTAGE_TICKS, TIME_TICKS, WINDOW_DURATION } from '../utils/eegConstants';

interface EegRendererProps {
  canvasRef: React.RefObject<HTMLCanvasElement | null>;
  dataRef: React.MutableRefObject<ScrollingBuffer[]>;
  config: any;
  renderNeededRef: React.MutableRefObject<boolean>;
  latestTimestampRef: React.MutableRefObject<number>;
  debugInfoRef: React.MutableRefObject<{
    lastPacketTime: number;
    packetsReceived: number;
    samplesProcessed: number;
  }>;
}

export function EegRenderer({
  canvasRef,
  dataRef,
  config,
  renderNeededRef,
  latestTimestampRef,
  debugInfoRef
}: EegRendererProps) {
  const reglRef = useRef<any>(null);
  const pointsArraysRef = useRef<Float32Array[]>([]);
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
      
      // Set render needed flag to ensure all channels are drawn after initialization
      renderNeededRef.current = true;
    }
  }, [config, isProduction, renderNeededRef]);

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
        const scaleFactor = Math.min(0.1, 0.3 / channelCount);
        const normalizedVoltage = (voltage / 3) * scaleFactor;
        const y = chOffset + normalizedVoltage;
        
        gridLines.push(
          [-1, y], // Left
          [1, y]   // Right
        );
      });
    }
    
    // Optimized animation frame function
    const animate = () => {
      // Get current time for logging and other operations
      const now = Date.now();
      
      // Use a relative time window based on the latest data timestamp
      const latestTimestamp = latestTimestampRef.current;
      const startTime = latestTimestamp - WINDOW_DURATION;
      const endTime = latestTimestamp;
      const debugInfo = debugInfoRef.current;
      
      // Only log in development mode and very infrequently
      if (!isProduction && Math.random() < 0.01) {
        console.log(`Time window: ${new Date(startTime).toISOString()} to ${new Date(endTime).toISOString()}`);
      }
      
      // Always render to ensure data is displayed
      const shouldRender = true;
      
      if (shouldRender) {
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
        
        // Always draw all channels
        const channelCount = config?.channels?.length || 4;
        
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
          
          if (count > 1) {
            // Use the same linear distribution as the grid lines
            // This ensures consistent spacing between grid lines and EEG data
            const yOffset = channelCount <= 1
              ? 0
              : -0.9 + (ch / (channelCount - 1)) * 1.8;
            
            // Draw the channel data
            drawLines({
              points: points.subarray(0, count * 2),
              count: count,
              color: getChannelColor(ch),
              yOffset: yOffset,
              yScale: Math.min(0.1, 0.3 / channelCount) // Scale based on channel count
            });
          }
        }
        
        // Reset the render needed flag after drawing all channels
        renderNeededRef.current = false;
        
        // Log data status every 2 seconds in development mode
        if (!isProduction && now - debugInfo.lastPacketTime > 2000) {
          console.log(`Points drawn: ${totalPointsDrawn}, Packets: ${debugInfo.packetsReceived}`);
          debugInfo.lastPacketTime = now;
        }
      }
      
      requestAnimationFrame(animate);
    };
    
    // Start animation
    const animationId = requestAnimationFrame(animate);
    
    return () => {
      cancelAnimationFrame(animationId);
      regl.destroy();
    };
  }, [canvasRef, config, dataRef, renderNeededRef, latestTimestampRef, debugInfoRef]);

  return null;
}
