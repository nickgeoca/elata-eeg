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
 *
 * Double buffering is implemented to eliminate flickering:
 * 1. All drawing operations are performed on an off-screen canvas
 * 2. The completed frame is copied to the visible canvas in a single operation
 * 3. This prevents the user from seeing partial renders
 */

import React, { useEffect, useRef } from 'react';
import REGL from 'regl';
import { ScrollingBuffer } from '../utils/ScrollingBuffer';
import { getChannelColor } from '../utils/colorUtils';
import { VOLTAGE_TICKS, TIME_TICKS, WINDOW_DURATION, DEFAULT_SAMPLE_RATE } from '../utils/eegConstants';

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

export const EegRenderer = React.memo(function EegRenderer({
  canvasRef,
  dataRef,
  config,
  latestTimestampRef,
  debugInfoRef,
  voltageScaleFactor = 0.01
}: EegRendererProps) {
  const reglRef = useRef<any>(null);
  const offscreenCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const offscreenReglRef = useRef<any>(null);
  const pointsArraysRef = useRef<Float32Array[]>([]);
  const lastFrameTimeRef = useRef(Date.now());
  const frameCountRef = useRef(0);
  const lastFpsLogTimeRef = useRef(Date.now());
  const canvasDimensionsRef = useRef({ width: 0, height: 0 });
  const isProduction = process.env.NODE_ENV === 'production';

  // Track canvas dimensions to detect changes
  useEffect(() => {
    if (!canvasRef.current) return;
    
    const updateDimensions = () => {
      if (canvasRef.current) {
        // Get CSS dimensions
        const rect = canvasRef.current.getBoundingClientRect();
        const cssWidth = rect.width;
        const cssHeight = rect.height;
        
        // Get device pixel ratio
        const dpr = window.devicePixelRatio || 1;
        
        // Calculate physical dimensions
        const physicalWidth = Math.round(cssWidth * dpr);
        const physicalHeight = Math.round(cssHeight * dpr);

        // Only update if dimensions have changed
        if (physicalWidth !== canvasDimensionsRef.current.width ||
            physicalHeight !== canvasDimensionsRef.current.height) {
          
          canvasDimensionsRef.current = { width: physicalWidth, height: physicalHeight };
          
          // Update offscreen canvas dimensions to match
          if (offscreenCanvasRef.current) {
            // Set canvas attributes to physical dimensions
            offscreenCanvasRef.current.width = physicalWidth;
            offscreenCanvasRef.current.height = physicalHeight;
            
            // Ensure visible canvas also has correct attributes and style
            canvasRef.current.width = physicalWidth;
            canvasRef.current.height = physicalHeight;
            canvasRef.current.style.width = `${cssWidth}px`;
            canvasRef.current.style.height = `${cssHeight}px`;
            
            // Reinitialize WebGL contexts when dimensions change
            if (offscreenReglRef.current) {
              offscreenReglRef.current.destroy();
              offscreenReglRef.current = REGL({
                canvas: offscreenCanvasRef.current,
                attributes: {
                  antialias: false,
                  depth: false,
                  preserveDrawingBuffer: true
                }
              });
            }
            // Also update the main REGL context's viewport
            if (reglRef.current) {
              reglRef.current.poll();
            }
          }
          
          if (!isProduction) {
            console.log(`Canvas dimensions changed: CSS=${cssWidth}x${cssHeight}, Physical=${physicalWidth}x${physicalHeight} (DPR=${dpr}), updated canvases`);
          }
        }
      }
    };
    
    // Initial update
    updateDimensions();
    
    // Create observer to detect canvas resize
    const observer = new ResizeObserver(() => {
      updateDimensions();
    });
    
    observer.observe(canvasRef.current);
    
    return () => {
      if (canvasRef.current) {
        observer.unobserve(canvasRef.current);
      }
      observer.disconnect();
    };
  }, [canvasRef, isProduction]);

  // Pre-allocate point arrays for each channel to avoid GC
  useEffect(() => {
    // Use channel count from config or default to 4
    const channelCount = config?.channels?.length || 4;
    
    // Get buffer capacity from the first buffer or calculate from sample rate
    const bufferCapacity = dataRef.current[0]?.getCapacity() || 
                          Math.ceil(((config?.sample_rate || 250) * WINDOW_DURATION) / 1000);
    
    // Only recreate arrays if needed (channel count changed or not initialized or capacity changed)
    const needsUpdate = 
      pointsArraysRef.current.length !== channelCount ||
      (pointsArraysRef.current.length > 0 && pointsArraysRef.current[0].length < bufferCapacity * 2);
    
    if (needsUpdate) {
      // Allocate enough space for all points (x,y pairs)
      pointsArraysRef.current = Array(channelCount).fill(null).map(() =>
        new Float32Array(bufferCapacity * 2)
      );
      
      if (!isProduction) {
        console.log(`Initialized ${channelCount} point arrays with capacity ${bufferCapacity}`);
      }
    }
  }, [config, dataRef, isProduction]);

  // WebGL setup
  useEffect(() => {
    if (!canvasRef.current) return;
    
    if (!isProduction) {
      console.log("Initializing WebGL renderer with double buffering");
    }
    
    // Create offscreen canvas with the same dimensions
    const visibleCanvas = canvasRef.current;
    const { width, height } = visibleCanvas;
    
    // Create offscreen canvas
    const offscreenCanvas = document.createElement('canvas');
    offscreenCanvas.width = width;
    offscreenCanvas.height = height;
    offscreenCanvasRef.current = offscreenCanvas;
    
    // Initialize regl for the offscreen canvas
    const offscreenRegl = REGL({
      canvas: offscreenCanvas,
      attributes: {
        antialias: false,
        depth: false,
        preserveDrawingBuffer: true
      }
    });
    
    offscreenReglRef.current = offscreenRegl;
    
    // Initialize regl for the visible canvas (for copying only)
    const regl = REGL({
      canvas: visibleCanvas,
      attributes: {
        antialias: false,
        depth: false,
        preserveDrawingBuffer: true
      }
    });
    
    reglRef.current = regl;
    
    // Get FPS from config with no fallback
    const renderFps = config?.fps || 0;
    
    if (!isProduction) {
      console.log(`Setting render FPS to ${renderFps}`);
    }
    
    // Create WebGL command for drawing the grid
    const drawGrid = offscreenRegl({
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
        position: offscreenRegl.prop('points')
      },
      uniforms: {
        color: offscreenRegl.prop('color')
      },
      primitive: 'lines',
      count: offscreenRegl.prop('count'), // Add count property
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
    const drawLines = offscreenRegl({
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
        position: offscreenRegl.prop('points')
      },
      uniforms: {
        color: offscreenRegl.prop('color'),
        yOffset: offscreenRegl.prop('yOffset'),
        yScale: offscreenRegl.prop('yScale')
      },
      primitive: 'line strip',
      lineWidth: 1.0, // Minimum allowed line width in REGL (must be between 1 and 32)
      count: offscreenRegl.prop('count'),
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
    
    // Function to create grid lines
    const createGridLines = () => {
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
        // Channel placement algorithm: (ch_num / (n_channels + 1)) * 100 (% from top)
        // Convert to percentage from top (ch+1 because channels start at 1)
        const percentFromTop = ((ch + 1) / (channelCount + 1)) * 100;
        // Convert percentage to WebGL y-coordinate (1 at top, -1 at bottom)
        let chOffset = 1.0 - (percentFromTop / 50.0);
        
        VOLTAGE_TICKS.forEach(voltage => {
          // Normalize voltage to [-1, 1] range within channel space
          // Scale based on channel count to prevent overlap with many channels
          // Consolidated scaling: Base scale allocates space per channel, voltageScaleFactor adjusts amplitude.
          const scaleFactor = (1 / (channelCount + 1)) * voltageScaleFactor;
          const normalizedVoltage = (voltage / 3) * scaleFactor;
          const y = chOffset + normalizedVoltage;
          
          gridLines.push(
            [-1, y], // Left
            [1, y]   // Right
          );
        });
      }
      
      return gridLines;
    };
    
    // Create initial grid lines
    const gridLines = createGridLines();
    
    // Create a command to copy from offscreen to visible canvas
    const copyToScreen = regl({
      frag: `
        precision mediump float;
        uniform sampler2D texture;
        varying vec2 uv;
        void main() {
          gl_FragColor = texture2D(texture, uv);
        }
      `,
      vert: `
        precision mediump float;
        attribute vec2 position;
        varying vec2 uv;
        void main() {
          uv = 0.5 * (position + 1.0);
          gl_Position = vec4(position, 0, 1);
        }
      `,
      attributes: {
        position: [
          -1, -1,
          1, -1,
          -1, 1,
          1, 1
        ]
      },
      uniforms: {
        texture: regl.prop('texture')
      },
      depth: { enable: false },
      count: 4,
      primitive: 'triangle strip'
    });

    // Render function with time-based scrolling and double buffering
    const render = () => {
      // --- Initialization Check ---
      // Ensure data buffers exist and timestamp is a valid positive number
      const isDataReady = dataRef.current &&
                          dataRef.current.length > 0 &&
                          dataRef.current[0] && // Check if at least the first buffer is initialized
                          typeof latestTimestampRef.current === 'number' &&
                          latestTimestampRef.current > 0;
      
      if (!isDataReady) {
        // Data not ready, skip this frame
        if (!isProduction) {
            // Log detailed status only once per second to avoid flooding console
            const nowLog = Date.now();
            // Use a temporary window property for throttling state
            if (!(window as any)._eegRendererLastLog || nowLog - (window as any)._eegRendererLastLog > 1000) {
                console.log(`EEG Renderer: Data not ready, skipping frame. Status: dataRef.current=${!!dataRef.current}, length=${dataRef.current?.length}, [0]=${!!dataRef.current?.[0]}, timestamp=${latestTimestampRef.current}`);
                (window as any)._eegRendererLastLog = nowLog;
            }
        }
        return; // Skip rendering this frame
      }
      // --- End Initialization Check ---

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
      const sampleRate = config?.sample_rate || DEFAULT_SAMPLE_RATE;
      
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
          
          // No need to update render offset here as it's calculated on-demand
          // Log renderOffset occasionally for debugging
          if (!isProduction && ch === 0 && Math.random() < 0.05) {
            console.log(`Current renderOffset for channel ${ch}: ${dataRef.current[ch].getRenderOffset().toFixed(2)}, dt=${deltaTime.toFixed(4)}s`);
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
      
      // STEP 1: Draw to the offscreen canvas
      // Clear the offscreen canvas
      offscreenReglRef.current.clear({
        color: [0.1, 0.1, 0.2, 1],
        depth: 1
      });
      
      // Draw grid on offscreen canvas
      drawGrid({
        points: gridLines,
        color: [0.2, 0.2, 0.2, 0.8],
        count: gridLines.length
      });
      
      // Track if any data was drawn
      let totalPointsDrawn = 0;
      
      // Draw each channel on offscreen canvas - always draw all channels together
      for (let ch = 0; ch < channelCount; ch++) {
        if (!dataRef.current[ch]) continue;
        const buffer = dataRef.current[ch];
        
        // Ensure point arrays are large enough
        if (ch >= pointsArraysRef.current.length ||
            pointsArraysRef.current[ch].length < buffer.getCapacity() * 2) {
          // Reallocate this channel's points array
          pointsArraysRef.current[ch] = new Float32Array(buffer.getCapacity() * 2);
          if (!isProduction) {
            console.log(`Reallocated points array for channel ${ch} with capacity ${buffer.getCapacity()}`);
          }
        }
        
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
          // Channel placement algorithm: (ch_num / (n_channels + 1)) * 100 (% from top)
          // Convert to percentage from top (ch+1 because channels start at 1)
          const percentFromTop = ((ch + 1) / (channelCount + 1)) * 100;
          // Convert percentage to WebGL y-coordinate (1 at top, -1 at bottom)
          const yOffset = 1.0 - (percentFromTop / 50.0);
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
          
          // Draw the channel data on offscreen canvas
          drawLines({
            points: points.subarray(0, safeCount * 2),
            count: safeCount,
            color: getChannelColor(ch),
            yOffset: yOffset,
            // Max channel height equation: 100 / (n_channels + 1) (% of total height)
            // Convert percentage to WebGL scale factor and apply user voltage scaling
            // Consolidated scaling: Base scale allocates space per channel, voltageScaleFactor adjusts amplitude.
            yScale: (1 / (channelCount + 1)) * voltageScaleFactor
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
      
      // STEP 2: Copy from offscreen canvas to visible canvas
      // Create a texture from the offscreen canvas
      const offscreenTexture = reglRef.current.texture(offscreenCanvasRef.current);
      
      // Clear the visible canvas
      reglRef.current.clear({
        color: [0.1, 0.1, 0.2, 1],
        depth: 1
      });
      
      // Copy the offscreen texture to the visible canvas
      copyToScreen({
        texture: offscreenTexture
      });
      
      // Clean up the texture to prevent memory leaks
      offscreenTexture.destroy();
    };
    // Set up rendering with FPS control
    let animationFrameId: number;
    let lastRenderTime = 0;
    
    const animationLoop = (timestamp: number) => {
      // Calculate time since last render
      const elapsed = timestamp - lastRenderTime;
      
      // Calculate frame interval based on desired FPS
      const frameInterval = 1000 / (renderFps || 60); // Default to 60 FPS if not specified
      
      // Only render if enough time has elapsed
      if (elapsed >= frameInterval) {
        render();
        lastRenderTime = timestamp - (elapsed % frameInterval); // Adjust for any remainder
        
        if (!isProduction && Math.random() < 0.05) {
          console.log(`Rendering at interval: ${elapsed.toFixed(2)}ms (target: ${frameInterval.toFixed(2)}ms)`);
        }
      }
      
      // Schedule next frame
      animationFrameId = requestAnimationFrame(animationLoop);
    };
    
    // Start the animation
    animationFrameId = requestAnimationFrame(animationLoop);
    
    // Clean up
    return () => {
      cancelAnimationFrame(animationFrameId);
      
      // Clean up both regl instances
      if (reglRef.current) {
        reglRef.current.destroy();
      }
      
      if (offscreenReglRef.current) {
        offscreenReglRef.current.destroy();
      }
      
      // Remove references to canvases
      offscreenCanvasRef.current = null;
    };
  }, [canvasRef, config, dataRef, latestTimestampRef, debugInfoRef, voltageScaleFactor]);

  return null;
});
