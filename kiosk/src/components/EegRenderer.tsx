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
  canvasRef: React.RefObject<HTMLCanvasElement | null>;
  dataRef: React.RefObject<any>; // Re-added as required prop
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
  dataVersion: number;
}

export const EegRenderer = React.memo(function EegRenderer({
  isActive,
  canvasRef,
  dataRef, // Add dataRef prop here
  config,
  latestTimestampRef,
  debugInfoRef,
  dataBuffer, // Destructure dataBuffer
  targetFps,
  containerWidth, // Destructure new prop
  containerHeight, // Destructure new prop
  dataVersion
}: EegRendererProps) {
  const wglpRef = useRef<WebglPlot | null>(null);
  // Array of WebglStep instances, one per channel
  // const linesRef = useRef<WebglStep[] | null>(null); // Removed - Use dataRef prop instead
  const animationFrameRef = useRef<number | null>(null);
  const isInitializedRef = useRef<boolean>(false);
  // const [canvasSized, setCanvasSized] = useState<boolean>(false); // Removed - use containerWidth/Height props
  // Removed wglpInstance state, reverting to refs
  // Last data chunk timestamps per channel
  const lastDataChunkTimeRef = useRef<number[]>([]);
  const lastRenderTimeRef = useRef<number>(0); // For FPS throttling

  const numChannels = config?.channels?.length ?? 8;

  // Render logic now pulls from the buffer and processes data asynchronously
  if (isActive && wglpRef.current && dataRef.current && isInitializedRef.current && numChannels > 0) {
    const wglp = wglpRef.current;
    const lines = dataRef.current;

    // Get data from the buffer
    const sampleChunks = dataBuffer.getAndClearData();

    if (sampleChunks.length > 0) {
      let dataWasAdded = false;
      const channelBatches: Record<number, number[]> = {};
      const channelOrder: number[] = [];

      sampleChunks.forEach((chunk: SampleChunk) => {
        chunk.samples.forEach((sample) => {
          const chIndex = sample.channelIndex;
          if (chIndex < numChannels) {
            if (!channelBatches[chIndex]) {
              channelBatches[chIndex] = [];
              channelOrder.push(chIndex);
            }
            channelBatches[chIndex].push(sample.value);
          }
        });
      });

      channelOrder.forEach((chIndex) => {
        if (lines[chIndex] && channelBatches[chIndex]) {
          lines[chIndex].shiftAdd(new Float32Array(channelBatches[chIndex]));
          dataWasAdded = true;
        }
      });
    }

    wglp.update();
  }


  // Effect 1: Initialize WebGL Plot when canvas is ready and sized
  useEffect(() => {
    // Skip if plot already exists, canvas missing, or container dimensions are not valid
    const validDimensions = containerWidth > 0 && containerHeight > 0;
    if (!isActive || wglpRef.current || !canvasRef.current || !validDimensions || numChannels === 0) {
      console.log(`[EegRenderer InitEffect1] Skipping plot creation (Active: ${isActive}, Plot Exists: ${!!wglpRef.current}, Canvas: ${!!canvasRef.current}, ValidDimensions: ${validDimensions} [${containerWidth}x${containerHeight}], Channels: ${numChannels}).`);
      return;
    }

    const canvas = canvasRef.current;
    // Explicitly size the canvas using current props BEFORE initializing WebglPlot
    const dpr = window.devicePixelRatio || 1;
    const cssWidth = containerWidth;
    const cssHeight = containerHeight; // Or a fraction if EegMonitor calculates it for aspect ratio

    const physicalWidth = Math.round(cssWidth * dpr);
    const physicalHeight = Math.round(cssHeight * dpr);

    // Check if canvas actually needs resizing before applying.
    // This ensures that if the effect re-runs due to other dependency changes
    // but the size is already correct, we don't unnecessarily manipulate the DOM.
    if (canvas.width !== physicalWidth || canvas.height !== physicalHeight) {
      console.log(`[EegRenderer InitEffect1] Sizing canvas for initialization: ${cssWidth}x${cssHeight} (CSS), ${physicalWidth}x${physicalHeight} (Physical), DPR: ${dpr}`);
      canvas.width = physicalWidth;
      canvas.height = physicalHeight;
      canvas.style.width = `${cssWidth}px`;
      canvas.style.height = `${cssHeight}px`;
    } else {
      console.log(`[EegRenderer InitEffect1] Canvas already correctly sized for initialization: ${cssWidth}x${cssHeight} (CSS), ${physicalWidth}x${physicalHeight} (Physical), DPR: ${dpr}`);
    }
    
    console.log("[EegRenderer InitEffect1] Initializing WebGL Plot instance (after explicit sizing)...");

    try {
      const wglp = new WebglPlot(canvas);
      wglpRef.current = wglp; // Store in ref

      wglp.gScaleX = 1;
      wglp.gScaleY = 1;

      isInitializedRef.current = true; // Mark plot as initialized using ref
      console.log(`[EegRenderer InitEffect1] WebGL Plot initialized.`);

      // Render loop is no longer started here

    } catch (error) {
      console.error("[EegRenderer InitEffect1] Error initializing WebGL Plot:", error);
      wglpRef.current = null;
      isInitializedRef.current = false; // Reset ref on error
    }

    // Cleanup for THIS effect (plot creation)
    return () => {
      console.log("[EegRenderer InitEffect1] Cleaning up WebGL Plot instance...");
      if (animationFrameRef.current) {
        cancelAnimationFrame(animationFrameRef.current);
        animationFrameRef.current = null;
      }
      if (wglpRef.current) {
        // @ts-ignore
        wglpRef.current.removeAllLines();
      }
      wglpRef.current = null; // Clear ref on cleanup
      isInitializedRef.current = false; // Reset ref on cleanup
      console.log("[EegRenderer InitEffect1] Plot instance cleanup complete.");
    };
    // Depend on canvasRef, numChannels, containerWidth, containerHeight
  }, [isActive, canvasRef, numChannels, containerWidth, containerHeight]);


  // Effect 2: Add/Update lines when they are ready AND plot is initialized
  useEffect(() => {
    // Use wglpRef
    const wglp = wglpRef.current;

    // Only proceed if plot is initialized (via ref) AND plot exists
    if (!isInitializedRef.current || !wglp) {
        // console.log(`[EegRenderer InitEffect2] Skipping line addition (Initialized: ${isInitializedRef.current}, Plot Exists: ${!!wglp})`);
        return;
    }

    // Check if dataRef has lines
    const lines = dataRef.current;
    if (!lines || lines.length === 0) {
        console.warn("[EegRenderer InitEffect2] Lines are ready, but dataRef is empty. Cannot add lines.");
        return;
    }

    console.log(`[EegRenderer InitEffect2] Adding/Updating ${lines.length} lines.`);

    // Clear existing lines before adding new ones - IMPORTANT
    // Assuming webgl-plot doesn't have a dedicated clear, we might need to remove lines individually
    // or manage the lines array internally. For now, let's re-add, assuming addLine handles it.
    // A better approach might involve checking if a line instance is already added.

    lines.forEach((line: WebglStep, i: number) => {
      if (line) {
        try {
          const colorTuple = getChannelColor(i);
          line.color = new ColorRGBA(colorTuple[0], colorTuple[1], colorTuple[2], 1);
        } catch (error) {
          console.error(`[EegRenderer InitEffect2] Ch ${i}: Error setting color:`, error);
          line.color = new ColorRGBA(1, 1, 1, 1); // fallback white
        }
        try {
            (wglp as any).addLine(line);
        } catch(addError) {
            console.error(`[EegRenderer InitEffect2] Ch ${i}: Error adding line:`, addError, line);
        }
      } else {
          console.warn(`[EegRenderer InitEffect2] Ch ${i}: Line instance is null or undefined in dataRef.`);
      }
    });

    console.log(`[EegRenderer InitEffect2] Lines added/updated.`);
    wglp.update(); // Update plot after adding/updating lines

    // No cleanup needed specifically for adding lines, Effect 1 handles plot cleanup.

  // Depend on plot initialization state, lines readiness state, and the actual dataRef content
  // Check isInitializedRef.current inside
  }, [dataVersion, wglpRef, isInitializedRef]);


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


  // Component renders nothing itself
  return null;
});
