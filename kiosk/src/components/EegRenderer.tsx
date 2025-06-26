'use client';

import React, { useEffect, useRef, useCallback, useState } from 'react';
// Keep ColorRGBA for potential future use or if setLineColor gets fixed
/* eslint-disable @typescript-eslint/ban-ts-comment */
// @ts-ignore: WebglLine is missing from types but exists at runtime
import { WebglPlot, ColorRGBA, WebglStep } from 'webgl-plot';
// Import getChannelColor for setting colors here
import { getChannelColor } from '../utils/colorUtils';
import { useDataBuffer } from '../hooks/useDataBuffer';
import { SampleChunk } from '../types/eeg';

interface EegRendererProps {
  isActive: boolean;
  lines: WebglStep[];
  config: any;
  latestTimestampRef: React.MutableRefObject<number>;
  debugInfoRef: React.MutableRefObject<{
    lastPacketTime: number;
    packetsReceived: number;
    samplesProcessed: number;
  }>;
  dataBuffer: ReturnType<typeof useDataBuffer<SampleChunk>>; // Add the dataBuffer prop
  targetFps?: number; // Optional target FPS for rendering
  containerWidth: number; // New prop for container width
  containerHeight: number; // New prop for container height
}

export const EegRenderer = React.memo(function EegRenderer({
  isActive,
  lines,
  config,
  latestTimestampRef,
  debugInfoRef,
  dataBuffer, // Destructure dataBuffer
  targetFps,
  containerWidth, // Destructure new prop
  containerHeight, // Destructure new prop
}: EegRendererProps) {
  const wglpRef = useRef<WebglPlot | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animationFrameRef = useRef<number | null>(null);
  const isInitializedRef = useRef<boolean>(false);
  // const [canvasSized, setCanvasSized] = useState<boolean>(false); // Removed - use containerWidth/Height props
  // Removed wglpInstance state, reverting to refs
  // Last data chunk timestamps per channel
  const lastDataChunkTimeRef = useRef<number[]>([]);
  const lastRenderTimeRef = useRef<number>(0); // For FPS throttling

  const numChannels = config?.channels?.length ?? 8;

  // This space is intentionally left blank. The render loop is now managed by `useAnimationFrame` below.


  // Effect 1: Initialize and clean up WebGL Plot
  useEffect(() => {
    if (!isActive || !canvasRef.current) {
      return;
    }

    console.log("[EegRenderer] Initializing WebGL Plot instance...");
    const canvas = canvasRef.current;
    const wglp = new WebglPlot(canvas);
    wglpRef.current = wglp;
    isInitializedRef.current = true;

    // Cleanup function
    return () => {
      console.log("[EegRenderer] Cleaning up WebGL Plot instance...");
      if (animationFrameRef.current) {
        cancelAnimationFrame(animationFrameRef.current);
        animationFrameRef.current = null;
      }
      // @ts-ignore
      wglp.removeAllLines();
      wglpRef.current = null;
      isInitializedRef.current = false;
      console.log("[EegRenderer] Plot instance cleanup complete.");
    };
  }, [isActive]); // Only depends on isActive

  // Effect 2: Add/Update lines when they are ready AND plot is initialized
  useEffect(() => {
    const wglp = wglpRef.current;
    if (!wglp || !isInitializedRef.current || lines.length === 0) {
      return;
    }

    console.log(`[EegRenderer] Adding/Updating ${lines.length} lines.`);
    
    // @ts-ignore
    wglp.removeAllLines(); // Clear previous lines to prevent duplicates

    lines.forEach((line, i) => {
      if (line) {
        try {
          const colorTuple = getChannelColor(i);
          line.color = new ColorRGBA(colorTuple[0], colorTuple[1], colorTuple[2], 1);
          wglp.addLine(line);
        } catch (error) {
          console.error(`[EegRenderer] Ch ${i}: Error adding line:`, error);
        }
      }
    });

    wglp.update();
  }, [lines]); // Reruns when lines array changes


  // Resize Effect: Now depends on containerWidth and containerHeight props
  useEffect(() => {
    if (!canvasRef.current || containerWidth === 0 || containerHeight === 0) {
      // console.log(`[EegRenderer ResizeEffect] Skipping resize: Canvas: ${!!canvasRef.current}, ContainerDims: ${containerWidth}x${containerHeight}`);
      return;
    }

    const canvas = canvasRef.current;
    const dpr = window.devicePixelRatio || 1;
    
    // Use containerWidth and containerHeight directly for CSS size
    const cssWidth = containerWidth;
    const cssHeight = containerHeight; // Or a fraction if EegMonitor calculates it for aspect ratio

    const physicalWidth = Math.round(cssWidth * dpr);
    const physicalHeight = Math.round(cssHeight * dpr);

    if (canvas.width !== physicalWidth || canvas.height !== physicalHeight) {
      console.log(`[EegRenderer ResizeEffect] Resizing canvas to: ${cssWidth}x${cssHeight} (CSS), ${physicalWidth}x${physicalHeight} (Physical), DPR: ${dpr}`);
      canvas.width = physicalWidth;
      canvas.height = physicalHeight;
      canvas.style.width = `${cssWidth}px`;
      canvas.style.height = `${cssHeight}px`;

      if (wglpRef.current) {
         wglpRef.current.gScaleY = 1; // Maintain consistent Y scaling
         console.log(`[EegRenderer ResizeEffect] Kept gScaleY at 1 on resize.`);
         wglpRef.current.update(); // Update plot after canvas resize
      }
    }
    // No cleanup needed here as we are not using ResizeObserver anymore
  }, [canvasRef, containerWidth, containerHeight]); // Depend on props


  // Effect 4: The Render Loop
  useEffect(() => {
    if (!isActive || !wglpRef.current || lines.length === 0) {
      return;
    }

    const wglp = wglpRef.current;
    let animationFrameId: number;

    const render = () => {
      const sampleChunks = dataBuffer.getAndClearData();
      if (sampleChunks.length > 0) {
        const channelBatches: Record<number, number[]> = {};

        sampleChunks.forEach(chunk => {
          chunk.samples.forEach(sample => {
            if (!channelBatches[sample.channelIndex]) {
              channelBatches[sample.channelIndex] = [];
            }
            channelBatches[sample.channelIndex].push(sample.value);
          });
        });

        Object.entries(channelBatches).forEach(([chIndexStr, values]) => {
          const chIndex = parseInt(chIndexStr, 10);
          if (lines[chIndex] && values.length > 0) {
            lines[chIndex].shiftAdd(new Float32Array(values));
          }
        });
        
        wglp.update();
      }
      animationFrameId = requestAnimationFrame(render);
    };

    render();

    return () => {
      cancelAnimationFrame(animationFrameId);
    };
  }, [isActive, lines, dataBuffer]); // Simplified dependencies


  return (
    <div className="relative w-full h-full">
      <canvas ref={canvasRef} className="absolute top-0 left-0 w-full h-full" />
    </div>
  );
});
