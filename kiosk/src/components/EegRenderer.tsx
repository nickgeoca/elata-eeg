'use client';

import React, { useEffect, useRef, useCallback } from 'react';
// Keep ColorRGBA for potential future use or if setLineColor gets fixed
/* eslint-disable @typescript-eslint/ban-ts-comment */
// @ts-ignore: WebglLine is missing from types but exists at runtime
import { WebglPlot, ColorRGBA, WebglStep } from 'webgl-plot';
import { ScrollingBuffer } from '../utils/ScrollingBuffer';
// Keep getChannelColor for potential future use
import { getChannelColor } from '../utils/colorUtils';

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
  voltageScaleFactor?: number;
  containerRef?: React.RefObject<HTMLDivElement | null>;
}

const DEFAULT_VOLTAGE_SCALE = 100;

export const EegRenderer = React.memo(function EegRenderer({
  canvasRef,
  dataRef,
  config,
  latestTimestampRef,
  debugInfoRef,
  voltageScaleFactor = DEFAULT_VOLTAGE_SCALE,
  containerRef
}: EegRendererProps) {
  const wglpRef = useRef<WebglPlot | null>(null);
  // Array of WebglStep instances, one per channel
  const linesRef = useRef<WebglStep[] | null>(null);
  const animationFrameRef = useRef<number | null>(null);
  const isInitializedRef = useRef<boolean>(false);
  // Last data chunk timestamps per channel
  const lastDataChunkTimeRef = useRef<number[]>([]);

  const numChannels = config?.channels?.length ?? 8;

  // Render loop using single WebglLineRoll with addPoints
  const renderLoop = useCallback(() => {
    if (!wglpRef.current || !linesRef.current || !isInitializedRef.current || numChannels === 0) {
      animationFrameRef.current = requestAnimationFrame(renderLoop);
      return;
    }
  
    const wglp = wglpRef.current;
    const lines = linesRef.current;
    const now = performance.now();
  
    const sampleRate = config?.sample_rate || 250;
    const totalNumSamples = lines[0]?.numPoints || 1000; // fallback
  
    for (let ch = 0; ch < numChannels; ch++) {
      const line = lines[ch];
      if (!line) continue;
  
      // Calculate elapsed time since last data batch
      const elapsedMs = now - (lastDataChunkTimeRef.current[ch] || now);
      const elapsedSec = elapsedMs / 1000;
      const offsetSamples = sampleRate * elapsedSec;
  
      // Set offsetX to smoothly scroll left
      line.offsetX = -offsetSamples / totalNumSamples;
    }
  
    wglp.update();
  
    animationFrameRef.current = requestAnimationFrame(renderLoop);
  
  }, [numChannels, config]);


  // Initialization Effect
  useEffect(() => {
    if (!canvasRef.current || numChannels === 0 || isInitializedRef.current) {
      console.log("[EegRenderer] Skipping initialization.");
      return;
    }

    const canvas = canvasRef.current;
    const elementToMeasure = (containerRef?.current) ? containerRef.current : canvas;
    const initialRect = elementToMeasure.getBoundingClientRect();
    const initialDpr = window.devicePixelRatio || 1;
    if (canvas.width === 0 || canvas.height === 0) {
        if (initialRect.width > 0 && initialRect.height > 0) {
            canvas.width = Math.round(initialRect.width * initialDpr);
            canvas.height = Math.round(initialRect.height * initialDpr);
            console.log(`[EegRenderer] Initial canvas size set during init: ${canvas.width}x${canvas.height}`);
        } else {
            console.warn("[EegRenderer] Canvas dimensions are zero during init. Deferring initialization.");
            return;
        }
    }

    console.log("[EegRenderer] Initializing WebGL Plot (@next) with SINGLE WebglLineRoll instance...");

    try {
      const wglp = new WebglPlot(canvas);
      wglpRef.current = wglp;

      wglp.gScaleX = 1;
      wglp.gScaleY = 1; // Keep Y scale at 1

      // Use canvas.width (physical pixels) as the buffer size/width argument
      const rollWidth = canvas.width;

      const lines: any[] = [];
      const verticalSpacing = 2.5; // Space between channels
      for (let i = 0; i < numChannels; i++) {
        const initialNumPoints = canvas.width; // or fixed size
        const line = new WebglStep(new ColorRGBA(1,1,1,1), initialNumPoints);

        // Set distinct color per channel
        try {
          const colorTuple = getChannelColor(i);
          line.color = new ColorRGBA(
            colorTuple[0] * 255,
            colorTuple[1] * 255,
            colorTuple[2] * 255,
            1
          );
        } catch {
          line.color = new ColorRGBA(255, 255, 255, 1); // fallback white
        }

        line.lineWidth = 1;

        // Scale EEG voltage to visible range
        line.scaleY = voltageScaleFactor;

        // Offset vertically to stack channels
        line.offsetY = i * verticalSpacing;

        line.lineSpaceX(-1, 2 / initialNumPoints);

        (wglp as any).addLine(line);
        lines.push(line);
      }
      linesRef.current = lines;

      // Set colors for each line - KEEP COMMENTED OUT due to runtime errors
      /*
      for (let i = 0; i < numChannels; i++) {
        const colorTuple = getChannelColor(i);
        const color = new ColorRGBA(
          colorTuple[0] * 255,
          colorTuple[1] * 255,
          colorTuple[2] * 255,
          1
        );
        // roll.setLineColor(color, i); // This caused errors
      }
      */

      isInitializedRef.current = true;
      console.log(`[EegRenderer] WebGL Plot initialized with 1 WebglLineRoll instance for ${numChannels} channels, width ${rollWidth}.`);

      animationFrameRef.current = requestAnimationFrame(renderLoop);

    } catch (error) {
      console.error("[EegRenderer] Error initializing WebGL Plot:", error);
      wglpRef.current = null;
      linesRef.current = null; // Clear lines ref on error
    }

    // Cleanup
    return () => {
      console.log("[EegRenderer] Cleaning up WebGL Plot...");
      if (animationFrameRef.current) {
        cancelAnimationFrame(animationFrameRef.current);
      }
      wglpRef.current = null;
      linesRef.current = null; // Clear lines ref on cleanup
      isInitializedRef.current = false;
      console.log("[EegRenderer] Cleanup complete.");
    };
  }, [canvasRef, numChannels, containerRef, renderLoop]);


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

          if (wglpRef.current) {
             wglpRef.current.gScaleY = 1;
             console.log(`[EegRenderer] Kept gScaleY at 1 on resize.`);
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
