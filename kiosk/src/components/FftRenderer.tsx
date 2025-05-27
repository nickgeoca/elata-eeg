'use client';

import { useRef, useEffect, useLayoutEffect, useState } from 'react';
// @ts-ignore: WebglLine might be missing from types or setY might be, but setY exists at runtime
import { WebglPlot, ColorRGBA, WebglLine } from 'webgl-plot';
import { getChannelColor } from '../utils/colorUtils';
import { getFrequencyBins } from '../utils/fftUtils'; // To get X-axis values
import { FFT_MIN_FREQ_HZ, FFT_MAX_FREQ_HZ } from '../utils/eegConstants'; // Import constants

const DATA_Y_MAX = 10.0; // Expected maximum for FFT power data (µV²/Hz), display range 0-10.0

interface FftRendererProps {
  canvasRef: React.RefObject<HTMLCanvasElement | null>; // Allow null for canvasRef
  fftDataRef: React.MutableRefObject<Record<number, number[]>>;
  fftDataVersion: number; // To trigger updates
  config: any; // EEG configuration (for sample_rate, channels)
  containerWidth: number;
  containerHeight: number;
  targetFps?: number; // Optional, for controlling update rate
}

export function FftRenderer({
  canvasRef,
  fftDataRef,
  fftDataVersion,
  config,
  containerWidth,
  containerHeight,
  targetFps = 30, // Default FFT update FPS
}: FftRendererProps) {
  const [fftLinesReady, setFftLinesReady] = useState(false);
  const wglpRef = useRef<WebglPlot | null>(null); // For WebglPlot instance
  const linesRef = useRef<WebglLine[]>([]); // Holds WebglLine line objects for FFT
  const animationFrameIdRef = useRef<number | null>(null);
  const lastUpdateTimeRef = useRef<number>(0);
  const xFreqRef = useRef<number[][]>([]); // Store X-axis frequency values for each line
  const previousXCoordsLengthRef = useRef<number | null>(null); // Store previous xCoords.length

  // Initialize WebGL context and plot instance
  useLayoutEffect(() => {
    if (canvasRef.current && containerWidth > 0 && containerHeight > 0) {
      if (!wglpRef.current) {
        // @ts-ignore
        wglpRef.current = new WebglPlot(canvasRef.current, {
          antialias: true,
          transparent: false,
        });
        console.log('[FftRenderer] WebGLPlot instance created.');
      }
      canvasRef.current.width = containerWidth;
      canvasRef.current.height = containerHeight;
      wglpRef.current.viewport(0, 0, containerWidth, containerHeight);
    }
    // No explicit cleanup for wglpRef here, managed by EegMonitor's canvas lifecycle
  }, [canvasRef, containerWidth, containerHeight]);

  // Effect to create/update FFT lines
  useEffect(() => {
    setFftLinesReady(false); // Assume lines are not ready until successfully created/updated

    if (!wglpRef.current || !config || !config.channels || !config.sample_rate || containerWidth <= 0) {
      if (linesRef.current.length > 0) {
        linesRef.current.forEach(line => wglpRef.current?.removeLine(line));
        linesRef.current = [];
        xFreqRef.current = [];
        previousXCoordsLengthRef.current = null;
      }
      // setFftLinesReady(false); // Already set at the start
      return;
    }

    const numChannels = config.channels.length;
    const sampleRate = config.sample_rate;

    if (numChannels === 0) {
      if (linesRef.current.length > 0) {
        linesRef.current.forEach(line => wglpRef.current?.removeLine(line));
        linesRef.current = [];
        xFreqRef.current = [];
        previousXCoordsLengthRef.current = null;
      }
      // setFftLinesReady(false); // Already set at the start
      return;
    }

    // Determine numFftBins from the first available FFT data or estimate
    let numFftBins = 0;
    if (fftDataRef.current && Object.keys(fftDataRef.current).length > 0) {
        const firstChannelKey = Object.keys(fftDataRef.current)[0];
        numFftBins = fftDataRef.current[parseInt(firstChannelKey)]?.length || 0;
    }
    
    if (numFftBins === 0) { // Fallback if no FFT data yet to determine bins
        // Estimate based on a typical FFT window (e.g., 2 seconds)
        // This is a rough estimation and might not match actual FFT output bins perfectly initially.
        // const estimatedFftWindowSamples = 2 * sampleRate; // 2 seconds of data
        // numFftBins = estimatedFftWindowSamples / 2;
        // For now, if numFftBins is 0, we can't create lines properly.
        // This will be resolved once fftDataRef is populated.
        console.warn('[FftRenderer] numFftBins is 0. Lines will be created/updated once FFT data arrives.');
        // Clear existing lines if numFftBins becomes 0 after being non-zero
        if (linesRef.current.length > 0) {
            linesRef.current.forEach(line => wglpRef.current?.removeLine(line));
            linesRef.current = [];
            xFreqRef.current = [];
            previousXCoordsLengthRef.current = null;
        }
        // setFftLinesReady(false); // Already set at the start
        return;
    }

    const allFreqs = getFrequencyBins(numFftBins, sampleRate);
    const relevantFreqIndices: number[] = [];
    const xCoords: number[] = [];

    allFreqs.forEach((freq, index) => {
        if (freq >= FFT_MIN_FREQ_HZ && freq <= FFT_MAX_FREQ_HZ) {
            relevantFreqIndices.push(index);
            const normalizedX = 2 * (freq - FFT_MIN_FREQ_HZ) / (FFT_MAX_FREQ_HZ - FFT_MIN_FREQ_HZ) - 1;
            xCoords.push(normalizedX);
        }
    });

    if (xCoords.length === 0) {
        console.warn(`[FftRenderer] No frequency bins found in the range ${FFT_MIN_FREQ_HZ}-${FFT_MAX_FREQ_HZ} Hz.`);
        if (linesRef.current.length > 0) {
            linesRef.current.forEach(line => wglpRef.current?.removeLine(line));
            linesRef.current = [];
            xFreqRef.current = [];
            previousXCoordsLengthRef.current = null;
        }
        // setFftLinesReady(false); // Already set at the start
        return;
    }

    // If xCoords.length has changed, it means the fundamental structure of the plot points has changed.
    // Clear all existing lines to force a full recreation.
    if (previousXCoordsLengthRef.current !== null && previousXCoordsLengthRef.current !== xCoords.length) {
      console.log(`[FftRenderer] xCoords.length changed from ${previousXCoordsLengthRef.current} to ${xCoords.length}. Recreating all lines.`);
      linesRef.current.forEach(line => wglpRef.current?.removeLine(line));
      linesRef.current = [];
      // xFreqRef will be repopulated naturally.
    }
    previousXCoordsLengthRef.current = xCoords.length; // Update for the next run.

    const newLines: WebglLine[] = [];
    const newXFreqs: number[][] = [];
    // const ySpacing = 2.0 / numChannels; // Removed for overlaying lines

    for (let i = 0; i < numChannels; i++) {
      const colorTuple = getChannelColor(i);
      const color = new ColorRGBA(colorTuple[0], colorTuple[1], colorTuple[2], 1);
      
      let line = linesRef.current[i]; // Try to get existing line

      // If line doesn't exist (e.g., first run, or after linesRef was cleared due to xCoords.length change),
      // or if its numPoints is somehow incorrect (safeguard), create a new one.
      if (!(line instanceof WebglLine) || line.numPoints !== xCoords.length) {
        if (line instanceof WebglLine && wglpRef.current) {
          // Exists but numPoints is wrong, remove the old one from plot
          wglpRef.current.removeLine(line);
        }
        line = new WebglLine(color, xCoords.length);
        // Set X coordinates for the new line first
        if (line.xy) { // line.xy should exist after WebglStep constructor
          for (let j = 0; j < xCoords.length; j++) {
            if (j * 2 < line.xy.length) { // Bounds check for safety
              line.xy[j * 2] = xCoords[j];
            }
          }
        }
        // Initialize Y values using setY to ensure proper internal handling for WebglStep
        for (let k = 0; k < xCoords.length; k++) {
          // @ts-ignore: WebglLine type definition might be missing setY
          line.setY(k, -0.5); // Initialize to normalized bottom
        }
        // console.log(`[FftRenderer] Ch ${i}: Created/Recreated WebglStep with ${xCoords.length} points, X and Y initialized via setY.`);
      } else {
        // Line exists and numPoints is correct, just update its color
        line.color = color;
      }
      
      line.lineWidth = 1.5;
      
      // Set X coordinates for the visible frequency range
      for(let j=0; j < xCoords.length; j++) {
        if (line.xy && j * 2 < line.xy.length) { // Check if xy exists and index is within bounds
          line.xy[j * 2] = xCoords[j]; // X coordinate
        }
      }
      newXFreqs[i] = relevantFreqIndices; // Store indices of the FFT output to use

      // Scale Y and OffsetY for overlaying lines:
      // Data is normalized to [-0.5, 0.5] in the animation loop.
      // scaleY = 2.0 maps this [-0.5, 0.5] range to the full WebGL Y range of [-1, 1].
      // offsetY = 0 centers the lines.
      line.scaleY = 2.0;
      line.offsetY = 0;

      newLines.push(line);
      // Assuming addLine is idempotent or handles existing lines appropriately
      // Removed hasLine check as it's not a function on the actual object
      if (wglpRef.current) {
        wglpRef.current.addLine(line);
      }
    }

    // Remove old lines not in newLines (e.g. if numChannels decreased)
    // This check should only run if we didn't just clear all lines due to xCoords.length changing.
    // If xCoords.length changed, linesRef.current was already empty before this loop,
    // and newLines contains all fresh lines.
    // This handles the case where numChannels decreases but xCoords.length remains the same.
    if (linesRef.current.length > newLines.length && previousXCoordsLengthRef.current === xCoords.length) {
        linesRef.current.forEach(oldLine => {
            if (!newLines.includes(oldLine) && wglpRef.current) {
                 wglpRef.current.removeLine(oldLine);
            }
        });
    }

    linesRef.current = newLines;
    xFreqRef.current = newXFreqs;
    setFftLinesReady(true); // Lines are now successfully created/updated
    // console.log(`[FftRenderer] Updated ${numChannels} FFT lines with ${xCoords.length} points each (freq range ${FFT_MIN_FREQ_HZ}-${FFT_MAX_FREQ_HZ} Hz). fftLinesReady: true`);

  }, [config, containerWidth, containerHeight, fftDataVersion]); // Rerun if these change. fftDataVersion signals new data structure.

  // Animation loop to update FFT lines
  useEffect(() => {
    if (!wglpRef.current || !fftLinesReady || linesRef.current.length === 0 || xFreqRef.current.length === 0) {
      if (animationFrameIdRef.current) {
        cancelAnimationFrame(animationFrameIdRef.current);
        animationFrameIdRef.current = null;
      }
      return;
    }

    const updateInterval = 1000 / targetFps;

    const animate = (timestamp: number) => {
      animationFrameIdRef.current = requestAnimationFrame(animate);

      if (timestamp - lastUpdateTimeRef.current < updateInterval) {
        return;
      }
      lastUpdateTimeRef.current = timestamp;

      const activeChannels = config?.channels?.length || 0;
      const currentFftDataAllBins = fftDataRef.current;

      for (let i = 0; i < activeChannels; i++) {
        const line = linesRef.current[i];
        const relevantIndices = xFreqRef.current[i]; // Indices for the visible frequency range
        const channelFullFft = currentFftDataAllBins[i];

        if (line && relevantIndices && channelFullFft) {
          if (line.numPoints !== relevantIndices.length) {
            // This case should ideally be handled by the line creation effect.
            // console.warn(`[FftRenderer Ch ${i}] Mismatch line points (${line.numPoints}) vs relevant indices (${relevantIndices.length}). Recreating lines might be needed.`);
            continue; 
          }
          for (let j = 0; j < relevantIndices.length; j++) {
            const fftBinIndex = relevantIndices[j];
            let currentMagnitude = channelFullFft[fftBinIndex];

            // Sanitize and clamp the magnitude before normalization
            // 1. Ensure it's a finite number; if not, treat as 0.
            if (!isFinite(currentMagnitude)) {
              currentMagnitude = 0;
            }
            // 2. Ensure it's not negative (power should not be negative).
            currentMagnitude = Math.max(0, currentMagnitude);
            // 3. Clamp to DATA_Y_MAX to prevent excessively large positive normalized values.
            const displayMagnitude = Math.min(currentMagnitude, DATA_Y_MAX);

            // Normalize magnitude from [0, DATA_Y_MAX] to [-0.5, 0.5]
            // A magnitude of 0 will be at the bottom (-0.5).
            // A magnitude of DATA_Y_MAX will be at the top (0.5).
            let normalizedMagnitude = (displayMagnitude / DATA_Y_MAX) - 0.5;

            // Final check: ensure normalizedMagnitude is finite, otherwise default to bottom.
            if (!isFinite(normalizedMagnitude)) {
                normalizedMagnitude = -0.5;
            }

            // Use the line's setY method to update the Y coordinate.
            // The index 'j' corresponds to the data point index.
            // normalizedMagnitude is already calculated to be in the [-0.5, 0.5] range.
            // The line.scaleY and line.offsetY will handle final WebGL coordinate mapping.
            // @ts-ignore: WebglLine type definition might be missing setY, but it should exist at runtime.
            line.setY(j, normalizedMagnitude);
          }
        } else if (line) {
          // Clear line if data is missing by setting to the bottom of its range
          for (let j = 0; j < line.numPoints; j++) {
            // @ts-ignore: WebglLine type definition might be missing setY, but it should exist at runtime.
            line.setY(j, -0.5); // Y coordinate
          }
        }
      }
      wglpRef.current?.update();
    };

    animationFrameIdRef.current = requestAnimationFrame(animate);

    return () => {
      if (animationFrameIdRef.current) {
        cancelAnimationFrame(animationFrameIdRef.current);
        animationFrameIdRef.current = null;
      }
    };
  }, [fftLinesReady, fftDataVersion, config, fftDataRef, targetFps, containerWidth, containerHeight]);

  return null;
}