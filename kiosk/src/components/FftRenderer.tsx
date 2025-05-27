'use client';

import { useRef, useEffect, useLayoutEffect } from 'react';
// @ts-ignore: WebglStep might be missing from types but exists at runtime
import { WebglPlot, ColorRGBA, WebglStep } from 'webgl-plot';
import { getChannelColor } from '../utils/colorUtils';
import { getFrequencyBins } from '../utils/fftUtils'; // To get X-axis values
import { FFT_MIN_FREQ_HZ, FFT_MAX_FREQ_HZ } from '../utils/eegConstants'; // Import constants

interface FftRendererProps {
  canvasRef: React.RefObject<HTMLCanvasElement | null>; // Allow null for canvasRef
  fftDataRef: React.MutableRefObject<Record<number, number[]>>;
  fftDataVersion: number; // To trigger updates
  config: any; // EEG configuration (for sample_rate, channels)
  containerWidth: number;
  containerHeight: number;
  linesReady: boolean; // Similar to EegRenderer, to know when lines are set up
  targetFps?: number; // Optional, for controlling update rate
}

export function FftRenderer({
  canvasRef,
  fftDataRef,
  fftDataVersion,
  config,
  containerWidth,
  containerHeight,
  linesReady, // This prop might need to be managed specifically for FFT lines in EegMonitor
  targetFps = 30, // Default FFT update FPS
}: FftRendererProps) {
  const wglpRef = useRef<WebglPlot | null>(null); // For WebglPlot instance
  const linesRef = useRef<WebglStep[]>([]); // Holds WebglStep line objects for FFT
  const animationFrameIdRef = useRef<number | null>(null);
  const lastUpdateTimeRef = useRef<number>(0);
  const xFreqRef = useRef<number[][]>([]); // Store X-axis frequency values for each line

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
    if (!wglpRef.current || !config || !config.channels || !config.sample_rate || containerWidth <= 0) {
      linesRef.current.forEach(line => wglpRef.current?.removeLine(line));
      linesRef.current = [];
      xFreqRef.current = [];
      return;
    }

    const numChannels = config.channels.length;
    const sampleRate = config.sample_rate;

    if (numChannels === 0) {
      linesRef.current.forEach(line => wglpRef.current?.removeLine(line));
      linesRef.current = [];
      xFreqRef.current = [];
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
        }
        return;
    }


    const newLines: WebglStep[] = [];
    const newXFreqs: number[][] = [];
    const ySpacing = 2.0 / numChannels;

    const allFreqs = getFrequencyBins(numFftBins, sampleRate);
    
    // Filter frequencies and map to X coordinates
    const relevantFreqIndices: number[] = [];
    const xCoords: number[] = [];

    allFreqs.forEach((freq, index) => {
        if (freq >= FFT_MIN_FREQ_HZ && freq <= FFT_MAX_FREQ_HZ) {
            relevantFreqIndices.push(index);
            // Map [FFT_MIN_FREQ_HZ, FFT_MAX_FREQ_HZ] to [-1, 1]
            const normalizedX = 2 * (freq - FFT_MIN_FREQ_HZ) / (FFT_MAX_FREQ_HZ - FFT_MIN_FREQ_HZ) - 1;
            xCoords.push(normalizedX);
        }
    });
    
    if (xCoords.length === 0) {
        console.warn(`[FftRenderer] No frequency bins found in the range ${FFT_MIN_FREQ_HZ}-${FFT_MAX_FREQ_HZ} Hz. Check FFT settings and constants.`);
         linesRef.current.forEach(line => wglpRef.current?.removeLine(line));
         linesRef.current = [];
         xFreqRef.current = [];
        return;
    }


    for (let i = 0; i < numChannels; i++) {
      const colorTuple = getChannelColor(i);
      const color = new WebGLPlotNamespace.ColorRGBA(colorTuple[0] / 255, colorTuple[1] / 255, colorTuple[2] / 255, 1);
      
      const line = linesRef.current[i] instanceof WebGLPlotNamespace.WebglStep ? linesRef.current[i] : new WebGLPlotNamespace.WebglStep(color, xCoords.length);
      
      if (line.numPoints !== xCoords.length) {
        line.numPoints = xCoords.length;
      }
      line.color = color;
      line.lineWidth = 1.5;
      
      // Set X coordinates for the visible frequency range
      for(let j=0; j < xCoords.length; j++) {
        line.setX(j, xCoords[j]);
      }
      newXFreqs[i] = relevantFreqIndices; // Store indices of the FFT output to use

      // Scale Y: This needs to be dynamic or based on expected max magnitude.
      // Placeholder: normalize Y to fit within channel's allocated space.
      // Max magnitude could be passed or estimated. For now, assume data is somewhat normalized.
      line.scaleY = ySpacing * 0.4; // Use 40% of space, FFT magnitudes can be peaky
      line.offsetY = 1 - (i + 0.5) * ySpacing;

      newLines.push(line);
      if (!wglpRef.current.hasLine(line)) {
        wglpRef.current.addLine(line);
      }
    }

    // Remove old lines not in newLines (e.g. if numChannels decreased)
    linesRef.current.forEach(oldLine => {
        if (!newLines.includes(oldLine)) {
            wglpRef.current?.removeLine(oldLine);
        }
    });

    linesRef.current = newLines;
    xFreqRef.current = newXFreqs;
    // console.log(`[FftRenderer] Updated ${numChannels} FFT lines with ${xCoords.length} points each (freq range ${FFT_MIN_FREQ_HZ}-${FFT_MAX_FREQ_HZ} Hz).`);

  }, [config, containerWidth, containerHeight, fftDataVersion]); // Rerun if these change. fftDataVersion signals new data structure.

  // Animation loop to update FFT lines
  useEffect(() => {
    if (!wglpRef.current || !linesReady || linesRef.current.length === 0 || xFreqRef.current.length === 0) {
      if (animationFrameIdRef.current) cancelAnimationFrame(animationFrameIdRef.current);
      animationFrameIdRef.current = null;
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
            const magnitude = channelFullFft[fftBinIndex];
            // For WebglStep, we need to set the data directly in the xy array
            if (line.xy && j * 2 + 1 < line.xy.length) {
              line.xy[j * 2 + 1] = magnitude || 0; // Y coordinate
            }
          }
        } else if (line) {
          // Clear line if data is missing
          for (let j = 0; j < line.numPoints; j++) {
            if (line.xy && j * 2 + 1 < line.xy.length) {
              line.xy[j * 2 + 1] = 0; // Y coordinate
            }
          }
        }
      }
      wglpRef.current.update();
    };

    animationFrameIdRef.current = requestAnimationFrame(animate);

    return () => {
      if (animationFrameIdRef.current) {
        cancelAnimationFrame(animationFrameIdRef.current);
        animationFrameIdRef.current = null;
      }
    };
  }, [linesReady, fftDataVersion, config, fftDataRef, targetFps, containerWidth, containerHeight]);

  return null;
}