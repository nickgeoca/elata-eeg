'use client';

import React, { useEffect, useRef, useCallback, useState } from 'react';
// Keep ColorRGBA for potential future use or if setLineColor gets fixed
/* eslint-disable @typescript-eslint/ban-ts-comment */
// @ts-ignore: WebglLine is missing from types but exists at runtime
import { WebglPlot, ColorRGBA, WebglStep } from 'webgl-plot';
// Import getChannelColor for setting colors here
import { getChannelColor } from '../utils/colorUtils';

interface EegRendererProps {
  canvasRef: React.RefObject<HTMLCanvasElement | null>;
  dataRef: React.RefObject<any>; // Re-added as required prop
  config: any;
  latestTimestampRef: React.MutableRefObject<number>;
  debugInfoRef: React.MutableRefObject<{
    lastPacketTime: number;
    packetsReceived: number;
    samplesProcessed: number;
  }>;
  containerRef?: React.RefObject<HTMLDivElement | null>;
  linesReady: boolean; // Add prop to signal when lines are ready
  dataVersion: number; // Add prop to track data updates
  targetFps?: number; // Optional target FPS for rendering
}

export const EegRenderer = React.memo(function EegRenderer({
  canvasRef,
  dataRef, // Add dataRef prop here
  config,
  latestTimestampRef,
  debugInfoRef,
  containerRef,
  linesReady, // Destructure linesReady
  dataVersion, // Destructure dataVersion
  targetFps
}: EegRendererProps) {
  const wglpRef = useRef<WebglPlot | null>(null);
  // Array of WebglStep instances, one per channel
  // const linesRef = useRef<WebglStep[] | null>(null); // Removed - Use dataRef prop instead
  const animationFrameRef = useRef<number | null>(null);
  const isInitializedRef = useRef<boolean>(false);
  const [canvasSized, setCanvasSized] = useState<boolean>(false); // Track if canvas has been sized
  // Removed wglpInstance state, reverting to refs
  // Last data chunk timestamps per channel
  const lastDataChunkTimeRef = useRef<number[]>([]);
  const lastRenderTimeRef = useRef<number>(0); // For FPS throttling

  const numChannels = config?.channels?.length ?? 8;

  // Render loop using single WebglLineRoll with addPoints
  const renderLoop = useCallback(() => {
    animationFrameRef.current = requestAnimationFrame(renderLoop); // Request next frame immediately

    if (!wglpRef.current || !dataRef.current || !isInitializedRef.current || numChannels === 0) { // Use dataRef
      return;
    }
  
    const wglp = wglpRef.current;
    const lines = dataRef.current; // Use dataRef
    const now = performance.now();
  
    // FPS Throttling Logic
    if (targetFps && targetFps > 0) {
      const frameInterval = 1000 / targetFps;
      const elapsed = now - lastRenderTimeRef.current;

      if (elapsed < frameInterval) {
        return; // Skip this frame
      }
      lastRenderTimeRef.current = now - (elapsed % frameInterval); // Adjust for consistent timing
    } else {
      // No FPS target, or invalid target, render as fast as possible (synced with rAF)
      lastRenderTimeRef.current = now;
    }

    // The following loop for offsetX is not strictly needed for wglp.update()
    // but kept if any per-line logic might be re-introduced.
    // If it's purely for wglp.update(), it can be removed.
    for (let ch = 0; ch < numChannels; ch++) {
      const line = lines[ch];
      if (!line || line.numPoints === 0) continue; // Skip if line missing or has no points
      // No need to set offsetX here anymore
    }
  
    wglp.update();
  
  }, [numChannels, config, targetFps, dataRef, isInitializedRef]);


  // Effect 1: Initialize WebGL Plot when canvas is ready and sized
  useEffect(() => {
    // Skip if plot already exists, canvas missing, or canvas not sized yet
    if (wglpRef.current || !canvasRef.current || !canvasSized || numChannels === 0) {
      console.log(`[EegRenderer InitEffect1] Skipping plot creation (Plot Exists: ${!!wglpRef.current}, Canvas: ${!!canvasRef.current}, CanvasSized: ${canvasSized}, Channels: ${numChannels}).`);
      return;
    }

    const canvas = canvasRef.current;
    console.log("[EegRenderer InitEffect1] Initializing WebGL Plot instance...");

    try {
      const wglp = new WebglPlot(canvas);
      wglpRef.current = wglp; // Store in ref
      // setWglpInstance(wglp); // Removed state update

      wglp.gScaleX = 1;
      wglp.gScaleY = 1;

      isInitializedRef.current = true; // Mark plot as initialized using ref
      console.log(`[EegRenderer InitEffect1] WebGL Plot initialized.`);

      // Start render loop AFTER initialization
      if (!animationFrameRef.current) {
          animationFrameRef.current = requestAnimationFrame(renderLoop);
          console.log(`[EegRenderer InitEffect1] Render loop started.`);
      }

    } catch (error) {
      console.error("[EegRenderer InitEffect1] Error initializing WebGL Plot:", error);
      wglpRef.current = null;
      // setWglpInstance(null); // Removed state update
      isInitializedRef.current = false; // Reset ref on error
    }

    // Cleanup for THIS effect (plot creation)
    return () => {
      console.log("[EegRenderer InitEffect1] Cleaning up WebGL Plot instance...");
      if (animationFrameRef.current) {
        cancelAnimationFrame(animationFrameRef.current);
        animationFrameRef.current = null;
      }
      // wglpRef.current?.destroy?.(); // Optional destroy
      wglpRef.current = null; // Clear ref on cleanup
      // setWglpInstance(null); // Removed state update
      isInitializedRef.current = false; // Reset ref on cleanup
      // Don't reset canvasSized here, ResizeObserver handles it
      console.log("[EegRenderer InitEffect1] Plot instance cleanup complete.");
    };
    // Depend on canvasRef, numChannels, and canvasSized
  }, [canvasRef, numChannels, canvasSized, renderLoop]); // Added renderLoop dependency


  // Effect 2: Add/Update lines when they are ready AND plot is initialized
  useEffect(() => {
    // Use wglpRef
    const wglp = wglpRef.current;

    // Only proceed if plot is initialized (via ref) AND plot exists AND lines are ready
    if (!isInitializedRef.current || !wglp || !linesReady) {
        // console.log(`[EegRenderer InitEffect2] Skipping line addition (Initialized: ${isInitializedRef.current}, Plot Exists: ${!!wglp}, LinesReady: ${linesReady})`);
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
  // Check isInitializedRef.current inside, depend on linesReady and dataVersion
  }, [linesReady, dataVersion]);


  // Resize Effect (Keep as is, gScaleY=1 is handled)
  useEffect(() => {
    if (!canvasRef.current) return;

    const canvas = canvasRef.current;
    const elementToMeasure = (containerRef?.current) ? containerRef.current : canvas;

    const resizeObserver = new ResizeObserver(entries => {
      for (let entry of entries) {
        const { width, height } = entry.contentRect;
        const dpr = window.devicePixelRatio || 1;
        const displayWidth = Math.round(width * dpr);
        const displayHeight = Math.round(height * dpr);

        if (canvas.width !== displayWidth || canvas.height !== displayHeight) {
          console.log(`[EegRenderer] Resizing canvas to: ${width}x${height} (CSS), ${displayWidth}x${displayHeight} (Physical), DPR: ${dpr}`);
          canvas.width = displayWidth;
          canvas.height = displayHeight;

          // Use wglpRef here too
          if (wglpRef.current) {
             wglpRef.current.gScaleY = 1;
             console.log(`[EegRenderer] Kept gScaleY at 1 on resize.`);
             // Explicitly update the plot after resizing the canvas
             wglpRef.current.update();
          }
          // Mark canvas as sized if dimensions are valid
          if (displayWidth > 0 && displayHeight > 0) {
              setCanvasSized(true);
          }
          // NOTE: WebglLineRoll width/buffer size is fixed on initialization.
          // Resizing requires re-initialization if buffer size needs to change.
        }
      }
    });

    resizeObserver.observe(elementToMeasure);

    return () => {
      resizeObserver.unobserve(elementToMeasure);
      resizeObserver.disconnect();
    };
  }, [canvasRef, containerRef]);


  // Component renders nothing itself
  return null;
});
