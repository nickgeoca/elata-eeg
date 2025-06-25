'use client';

import { useRef, useEffect, useLayoutEffect, useState, useMemo } from 'react';
// @ts-ignore: WebglLine might be missing from types or setY might be, but setY exists at runtime
import { WebglPlot, ColorRGBA, WebglLine } from 'webgl-plot';
import { getChannelColor } from '../../../kiosk/src/utils/colorUtils';

// Constants for FFT display range (can be adjusted as needed)
export const FFT_MIN_FREQ_HZ = 1;  // Minimum frequency to display on the FFT plot in Hz
export const FFT_MAX_FREQ_HZ = 70; // Maximum frequency to display on the FFT plot in Hz


const DATA_Y_MAX = 10000.0; // Expected maximum for FFT power data (µV²/Hz)
const Y_AXIS_LOG_MIN_POWER_CLAMP = 0.01; // Clamp input power to this minimum before log10
const LOG_Y_MIN_DISPLAY = Math.log10(Y_AXIS_LOG_MIN_POWER_CLAMP); // e.g., -2 for 0.01
const LOG_Y_MAX_DISPLAY = Math.ceil(Math.log10(DATA_Y_MAX));    // e.g., Math.ceil(log10(4000)) = 4
                                                              // This defines the log display range, e.g., [-2, 4]

const GRID_COLOR = new ColorRGBA(0.25, 0.25, 0.25, 1); // Darker gray for grid lines
const LABEL_COLOR = '#bbbbbb'; // Light gray for labels
const AXIS_TITLE_COLOR = '#dddddd';
const CANVAS_BG_COLOR = new ColorRGBA(0.05, 0.05, 0.05, 1); // Dark background

// Margins for labels
const MARGIN_LEFT = 50; // Space for Y-axis labels
const MARGIN_BOTTOM = 40; // Space for X-axis labels & title
const MARGIN_TOP = 20; // Space for plot title (if any) or just padding
const MARGIN_RIGHT = 20; // Padding

interface LabelInfo {
  value: string;
  position: number; // Pixel position on the canvas dimension
  normalized: number; // Normalized WebGL coordinate (-1 to 1)
}

interface FftRendererProps {
  config: any; // EEG configuration (for sample_rate, channels)
  fftData: Record<number, number[]>; // FFT data passed as a prop
  containerWidth: number;
  containerHeight: number;
  targetFps?: number; // Optional, for controlling update rate
}

export function AppletFftRenderer({ // Renamed from FftRenderer
  config,
  fftData,
  containerWidth,
  containerHeight,
  targetFps = 30, // Default FFT update FPS
}: FftRendererProps) {
  const internalCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const [fftLinesReady, setFftLinesReady] = useState(false);
  const wglpRef = useRef<WebglPlot | null>(null); // For WebglPlot instance
  const linesRef = useRef<WebglLine[]>([]); // Holds WebglLine line objects for FFT
  const gridLinesRef = useRef<WebglLine[]>([]); // Holds WebglLine objects for the grid
  const animationFrameIdRef = useRef<number | null>(null);
  const lastUpdateTimeRef = useRef<number>(0);

  const [axisLabels, setAxisLabels] = useState<{ x: LabelInfo[], y: LabelInfo[] }>({ x: [], y: [] });
  const plotWidth = useMemo(() => Math.max(0, containerWidth - MARGIN_LEFT - MARGIN_RIGHT), [containerWidth]);
  const plotHeight = useMemo(() => Math.max(0, containerHeight - MARGIN_TOP - MARGIN_BOTTOM), [containerHeight]);

  // Initialize WebGL context and plot instance
  useLayoutEffect(() => {
    if (internalCanvasRef.current && containerWidth > 0 && containerHeight > 0) {
      if (!wglpRef.current) {
        // @ts-ignore
        wglpRef.current = new WebglPlot(internalCanvasRef.current, {
          antialias: true,
          transparent: false, // Keep canvas opaque for performance
          powerPerformance: "high-performance", // Corrected property name
        });
        console.log('[AppletFftRenderer] WebGLPlot instance created.');
      }
      internalCanvasRef.current.width = containerWidth; // Full container size for canvas
      internalCanvasRef.current.height = containerHeight;
      // Viewport is set for the actual plotting area, offset by margins
      wglpRef.current.viewport(MARGIN_LEFT, MARGIN_BOTTOM, plotWidth, plotHeight);
    }

    // Cleanup function
    return () => {
      if (wglpRef.current) {
        // Clear all lines before destroying the plot
        try {
          if (typeof wglpRef.current.clear === 'function') {
            wglpRef.current.clear();
          } else if (typeof wglpRef.current.removeLine === 'function') {
            // Fallback if clear is not available for some reason, or to be very thorough
            [...gridLinesRef.current, ...linesRef.current].forEach(line => {
              if (wglpRef.current?.removeLine) { // Ensure removeLine exists before calling
                 wglpRef.current.removeLine(line);
              }
            });
          }
        } catch (error) {
          console.warn('[AppletFftRenderer] Error clearing lines during cleanup:', error);
        }
        
        gridLinesRef.current = [];
        linesRef.current = [];
        
        // WebGL plot instance is managed by React's lifecycle and garbage collection.
        // The canvas element will be removed from the DOM.
        // Previous steps (clear/removeLine) handle clearing plot contents.
        
        wglpRef.current = null;
        console.log('[AppletFftRenderer] WebGLPlot instance cleaned up.');
      }
    };
  }, [containerWidth, containerHeight, plotWidth, plotHeight]);

  // Effect to create/update grid lines and labels
  useLayoutEffect(() => {
    if (!wglpRef.current || plotWidth <= 0 || plotHeight <= 0) {
      return;
    }
    const wglp = wglpRef.current;

    // Clear existing grid lines - use a more robust approach
    if (gridLinesRef.current.length > 0) {
      try {
        // Try different methods to clear lines
        if (wglp && typeof wglp.removeAllLines === 'function') {
          wglp.removeAllLines();
        } else if (wglp && typeof wglp.clear === 'function') {
          wglp.clear();
        } else if (wglp && typeof wglp.removeLine === 'function') {
          gridLinesRef.current.forEach(line => wglp.removeLine(line));
        } else {
          console.warn('[AppletFftRenderer] No clear method available, recreating WebGL plot instance');
          // Recreate the WebGL plot instance as a fallback
          if (internalCanvasRef.current) {
            wglpRef.current = new WebglPlot(internalCanvasRef.current, {
              antialias: true,
              transparent: false,
              powerPerformance: "high-performance",
            });
            const newWglp = wglpRef.current;
            newWglp.viewport(MARGIN_LEFT, MARGIN_BOTTOM, plotWidth, plotHeight);
          }
        }
      } catch (error) {
        console.warn('[AppletFftRenderer] Error clearing grid lines:', error);
      }
      gridLinesRef.current = [];
    }

    const newGridLines: WebglLine[] = [];
    const newXLabels: LabelInfo[] = [];
    const newYLabels: LabelInfo[] = [];

    // X-axis grid lines and labels (Frequency)
    const xTicks = [1, 10, 20, 30, 40, 50, 60, 70]; // Hz
    xTicks.forEach(freq => {
      if (freq >= FFT_MIN_FREQ_HZ && freq <= FFT_MAX_FREQ_HZ) {
        const normalizedX = 2 * (freq - FFT_MIN_FREQ_HZ) / (FFT_MAX_FREQ_HZ - FFT_MIN_FREQ_HZ) - 1;
        // Vertical grid line
        const gridX = new WebglLine(GRID_COLOR, 2);
        gridX.xy = new Float32Array([normalizedX, -1, normalizedX, 1]);
        newGridLines.push(gridX);

        // Label position (pixel space on canvas)
        const labelXPos = MARGIN_LEFT + (normalizedX + 1) / 2 * plotWidth;
        newXLabels.push({ value: freq.toString(), position: labelXPos, normalized: normalizedX });
      }
    });

    // Y-axis grid lines and labels (Log Power - Exponents)
    const yLogExponentTicks: number[] = [];
    for (let i = Math.floor(LOG_Y_MIN_DISPLAY); i <= Math.ceil(LOG_Y_MAX_DISPLAY); i++) {
      yLogExponentTicks.push(i);
    }

    yLogExponentTicks.forEach(logExponent => {
      const normalizedY = (logExponent - LOG_Y_MIN_DISPLAY) / (LOG_Y_MAX_DISPLAY - LOG_Y_MIN_DISPLAY) * 2 - 1;

      const gridY = new WebglLine(GRID_COLOR, 2);
      gridY.xy = new Float32Array([-1, normalizedY, 1, normalizedY]);
      newGridLines.push(gridY);
      
      const labelValue = logExponent.toString();
      
      const labelYPos = MARGIN_BOTTOM + (normalizedY + 1) / 2 * plotHeight; // Y is from bottom up for labels
      newYLabels.push({ value: labelValue, position: labelYPos, normalized: normalizedY });
    });

    newGridLines.forEach(line => wglp.addLine(line));
    gridLinesRef.current = newGridLines;
    setAxisLabels({ x: newXLabels, y: newYLabels.reverse() }); // Reverse Y labels for top-down display

  }, [plotWidth, plotHeight, FFT_MIN_FREQ_HZ, FFT_MAX_FREQ_HZ]);

  // Effect to create/update FFT lines
  useEffect(() => {
    if (!wglpRef.current || !config || !config.channels || !config.sample_rate || containerWidth <= 0) {
      return;
    }
    const wglp = wglpRef.current;
    const numChannels = config.channels.length;
    const sampleRate = config.sample_rate;
    const firstChannelData = Object.values(fftData)[0];
    const numFreqBins = firstChannelData ? firstChannelData.length : 0;

    if (numChannels === 0 || numFreqBins === 0) {
      if (linesRef.current.length > 0) {
        linesRef.current.forEach(line => wglp.removeLine(line));
        linesRef.current = [];
      }
      return;
    }

    // Calculate frequency bins and their corresponding x-coordinates
    const freqStep = sampleRate / (2 * (numFreqBins - 1));
    const relevantFreqIndices: number[] = [];
    const xCoords: number[] = [];

    for (let i = 0; i < numFreqBins; i++) {
      const freq = i * freqStep;
      if (freq >= FFT_MIN_FREQ_HZ && freq <= FFT_MAX_FREQ_HZ) {
        relevantFreqIndices.push(i);
        const normalizedX = 2 * (freq - FFT_MIN_FREQ_HZ) / (FFT_MAX_FREQ_HZ - FFT_MIN_FREQ_HZ) - 1;
        xCoords.push(normalizedX);
      }
    }

    // Recreate lines if channel count or frequency bins change
    if (linesRef.current.length !== numChannels || (linesRef.current[0] && linesRef.current[0].numPoints !== xCoords.length)) {
      linesRef.current.forEach(line => wglp.removeLine(line));
      const newLines: WebglLine[] = [];
      for (let i = 0; i < numChannels; i++) {
        const chIndex = config.channels[i];
        const colorTuple = getChannelColor(chIndex);
        const color = new ColorRGBA(colorTuple[0], colorTuple[1], colorTuple[2], 1);
        const line = new WebglLine(color, xCoords.length);
        line.lineWidth = 1.5;
        
        // Set initial X coordinates
        for (let j = 0; j < xCoords.length; j++) {
          line.xy[j * 2] = xCoords[j];
          line.xy[j * 2 + 1] = -0.5; // Default Y
        }
        
        line.scaleY = 2.0;
        line.offsetY = 0;
        wglp.addLine(line);
        newLines.push(line);
      }
      linesRef.current = newLines;
    }
    setFftLinesReady(true);

  }, [config, containerWidth, plotWidth]);

  // Animation loop to update FFT lines with new data
  useEffect(() => {
    if (!wglpRef.current || !fftLinesReady || linesRef.current.length === 0 || Object.keys(fftData).length === 0) {
      return;
    }

    const updateInterval = 1000 / targetFps;

    const animate = (timestamp: number) => {
      animationFrameIdRef.current = requestAnimationFrame(animate);

      if (timestamp - lastUpdateTimeRef.current < updateInterval) {
        return;
      }
      lastUpdateTimeRef.current = timestamp;

      const sampleRate = config.sample_rate;
      const firstChannelData = Object.values(fftData)[0];
      const numFreqBins = firstChannelData ? firstChannelData.length : 0;
      if (numFreqBins === 0) return;

      const freqStep = sampleRate / (2 * (numFreqBins - 1));
      const relevantFreqIndices: number[] = [];
      for (let i = 0; i < numFreqBins; i++) {
        const freq = i * freqStep;
        if (freq >= FFT_MIN_FREQ_HZ && freq <= FFT_MAX_FREQ_HZ) {
          relevantFreqIndices.push(i);
        }
      }

      linesRef.current.forEach((line, lineIndex) => {
        const chIndex = config.channels[lineIndex];
        const channelPowerData = fftData[chIndex];

        if (line && channelPowerData && line.numPoints === relevantFreqIndices.length) {
          for (let j = 0; j < relevantFreqIndices.length; j++) {
            const fftBinIndex = relevantFreqIndices[j];
            let currentMagnitude = channelPowerData[fftBinIndex] || 0;

            if (currentMagnitude <= 0 || !isFinite(currentMagnitude)) {
              currentMagnitude = Y_AXIS_LOG_MIN_POWER_CLAMP;
            }
            
            const logMagnitude = Math.log10(currentMagnitude);
            let normalizedMagnitude = (logMagnitude - LOG_Y_MIN_DISPLAY) / (LOG_Y_MAX_DISPLAY - LOG_Y_MIN_DISPLAY) - 0.5;

            if (!isFinite(normalizedMagnitude)) {
                normalizedMagnitude = -0.5;
            }
            line.xy[j * 2 + 1] = normalizedMagnitude;
          }
        }
      });
      
      wglpRef.current?.update();
    };

    animationFrameIdRef.current = requestAnimationFrame(animate);

    return () => {
      if (animationFrameIdRef.current) {
        cancelAnimationFrame(animationFrameIdRef.current);
        animationFrameIdRef.current = null;
      }
    };
  }, [fftLinesReady, fftData, config, targetFps]);

  return (
    <div style={{ width: containerWidth, height: containerHeight, position: 'relative', backgroundColor: `rgba(${CANVAS_BG_COLOR.r*255}, ${CANVAS_BG_COLOR.g*255}, ${CANVAS_BG_COLOR.b*255}, ${CANVAS_BG_COLOR.a})` }}>
      <canvas
        ref={internalCanvasRef}
        style={{
          width: containerWidth,
          height: containerHeight,
          display: 'block',
        }}
      />
      {/* X-Axis Labels */}
      {axisLabels.x.map((label, index) => (
        <div
          key={`x-label-${index}`}
          style={{
            position: 'absolute',
            left: label.position,
            bottom: MARGIN_BOTTOM - 20, 
            transform: 'translateX(-50%)',
            color: LABEL_COLOR,
            fontSize: '10px',
          }}
        >
          {label.value}
        </div>
      ))}
      {/* Y-Axis Labels */}
      {axisLabels.y.map((label, index) => (
        <div
          key={`y-label-${index}`}
          style={{
            position: 'absolute',
            left: MARGIN_LEFT - 30, 
            bottom: label.position, 
            transform: 'translateY(50%)', 
            color: LABEL_COLOR,
            fontSize: '10px',
            width: '25px', 
            textAlign: 'right',
          }}
        >
          {label.value}
        </div>
      ))}
      {/* X-Axis Title */}
      <div
        style={{
          position: 'absolute',
          left: MARGIN_LEFT + plotWidth / 2,
          bottom: 5, 
          transform: 'translateX(-50%)',
          color: AXIS_TITLE_COLOR,
          fontSize: '12px',
        }}
      >
        Frequency (Hz)
      </div>
      {/* Y-Axis Title */}
      <div
        style={{
          position: 'absolute',
          top: MARGIN_TOP + plotHeight / 2,
          left: 10, 
          transform: 'translateY(-50%) rotate(-90deg)', 
          transformOrigin: 'left top',
          color: AXIS_TITLE_COLOR,
          fontSize: '12px',
          whiteSpace: 'nowrap',
        }}
      >
        Power (log₁₀ µV²/Hz)
      </div>
    </div>
  );
}