'use client';

import { useRef, useEffect, useLayoutEffect, useState, useMemo } from 'react';
// @ts-ignore: WebglLine might be missing from types or setY might be, but setY exists at runtime
import { WebglPlot, ColorRGBA, WebglLine } from 'webgl-plot';
import { getChannelColor } from '../../../kiosk/src/utils/colorUtils';
import { getFrequencyBins } from '../../../kiosk/src/utils/fftUtils'; // To get X-axis values
import { FFT_MIN_FREQ_HZ, FFT_MAX_FREQ_HZ } from '../../../kiosk/src/utils/eegConstants'; // Import constants

const DATA_Y_MAX = 4000.0; // Expected maximum for FFT power data (µV²/Hz)
const Y_AXIS_LOG_MIN_POWER_CLAMP = 0.0001; // Clamp input power to this minimum before log10
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
  fftDataRef: React.MutableRefObject<Record<number, number[]>>;
  fftDataVersion: number; // To trigger updates
  config: any; // EEG configuration (for sample_rate, channels)
  containerWidth: number;
  containerHeight: number;
  targetFps?: number; // Optional, for controlling update rate
}

export function AppletFftRenderer({ // Renamed from FftRenderer
  fftDataRef,
  fftDataVersion,
  config,
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
  const xFreqRef = useRef<number[][]>([]); // Store X-axis frequency values for each line
  const previousXCoordsLengthRef = useRef<number | null>(null); // Store previous xCoords.length

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
        // wglpRef.current.setBackgroundColor(CANVAS_BG_COLOR.r, CANVAS_BG_COLOR.g, CANVAS_BG_COLOR.b, CANVAS_BG_COLOR.a); // Removed, not a valid method
        console.log('[AppletFftRenderer] WebGLPlot instance created.');
      }
      internalCanvasRef.current.width = containerWidth; // Full container size for canvas
      internalCanvasRef.current.height = containerHeight;
      // Viewport is set for the actual plotting area, offset by margins
      wglpRef.current.viewport(MARGIN_LEFT, MARGIN_BOTTOM, plotWidth, plotHeight);
    }
  }, [containerWidth, containerHeight, plotWidth, plotHeight]);


  // Effect to create/update grid lines and labels
  useLayoutEffect(() => {
    if (!wglpRef.current || plotWidth <= 0 || plotHeight <= 0) {
      return;
    }
    const wglp = wglpRef.current;

    // Clear existing grid lines
    gridLinesRef.current.forEach(line => {
      if (wglp && typeof wglp.removeLine === 'function') {
        wglp.removeLine(line);
      } else {
        console.warn('[AppletFftRenderer] wglp.removeLine is not a function or wglp is not available. Cannot remove grid line. This may lead to visual artifacts.');
      }
    });
    gridLinesRef.current = [];

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

    if (!yLogExponentTicks.includes(LOG_Y_MIN_DISPLAY) && LOG_Y_MIN_DISPLAY > Math.floor(LOG_Y_MIN_DISPLAY)) {
        // yLogExponentTicks.unshift(LOG_Y_MIN_DISPLAY); // Add at the beginning if not integer
    }
    if (!yLogExponentTicks.includes(LOG_Y_MAX_DISPLAY) && LOG_Y_MAX_DISPLAY < Math.ceil(LOG_Y_MAX_DISPLAY)) {
        // yLogExponentTicks.push(LOG_Y_MAX_DISPLAY); // Add at the end if not integer
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
    setFftLinesReady(false); 

    if (!wglpRef.current || !config || !config.channels || !config.sample_rate || containerWidth <= 0) {
      if (linesRef.current.length > 0) {
        linesRef.current.forEach(line => wglpRef.current?.removeLine(line));
        linesRef.current = [];
        xFreqRef.current = [];
        previousXCoordsLengthRef.current = null;
      }
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
      return;
    }

    let numFftBins = 0;
    if (fftDataRef.current && typeof fftDataRef.current === 'object' && Object.keys(fftDataRef.current).length > 0) {
        const channelDataArrays = Object.values(fftDataRef.current);
        for (const dataArray of channelDataArrays) {
            if (dataArray && typeof dataArray.length === 'number' && dataArray.length > 0) {
                numFftBins = dataArray.length;
                break; 
            }
        }
    }
    
    if (numFftBins === 0) { 
        console.warn('[AppletFftRenderer] numFftBins is 0. Lines will be created/updated once FFT data arrives.');
        if (linesRef.current.length > 0) {
            linesRef.current.forEach(line => wglpRef.current?.removeLine(line));
            linesRef.current = [];
            xFreqRef.current = [];
            previousXCoordsLengthRef.current = null;
        }
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
        console.warn(`[AppletFftRenderer] No frequency bins found in the range ${FFT_MIN_FREQ_HZ}-${FFT_MAX_FREQ_HZ} Hz.`);
        if (linesRef.current.length > 0) {
            linesRef.current.forEach(line => wglpRef.current?.removeLine(line));
            linesRef.current = [];
            xFreqRef.current = [];
            previousXCoordsLengthRef.current = null;
        }
        return;
    }

    if (previousXCoordsLengthRef.current !== null && previousXCoordsLengthRef.current !== xCoords.length) {
      console.log(`[AppletFftRenderer] xCoords.length changed from ${previousXCoordsLengthRef.current} to ${xCoords.length}. Recreating all lines.`);
      linesRef.current.forEach(line => wglpRef.current?.removeLine(line));
      linesRef.current = [];
    }
    previousXCoordsLengthRef.current = xCoords.length; 

    const newLines: WebglLine[] = [];
    const newXFreqs: number[][] = [];

    for (let i = 0; i < numChannels; i++) {
      const colorTuple = getChannelColor(i);
      const color = new ColorRGBA(colorTuple[0], colorTuple[1], colorTuple[2], 1);
      
      let line = linesRef.current[i]; 

      if (!(line instanceof WebglLine) || line.numPoints !== xCoords.length) {
        if (line instanceof WebglLine && wglpRef.current) {
          wglpRef.current.removeLine(line); 
        }
        line = new WebglLine(color, xCoords.length);
      } else {
        line.color = color;
      }
      
      line.lineWidth = 1.5;
      
      if (line.xy) {
        for (let j = 0; j < xCoords.length; j++) {
          if ((j * 2 + 1) < line.xy.length) { 
            line.xy[j * 2] = xCoords[j];         
            line.xy[j * 2 + 1] = -0.5;           
          }
        }
      }
      newXFreqs[i] = relevantFreqIndices; 

      line.scaleY = 2.0;
      line.offsetY = 0;

      newLines.push(line);
      if (wglpRef.current) {
        wglpRef.current.addLine(line);
      }
    }

    if (linesRef.current.length > newLines.length && previousXCoordsLengthRef.current === xCoords.length) {
        linesRef.current.forEach(oldLine => {
            if (!newLines.includes(oldLine) && wglpRef.current) {
                 wglpRef.current.removeLine(oldLine);
            }
        });
    }

    linesRef.current = newLines;
    xFreqRef.current = newXFreqs;
    setFftLinesReady(true); 

  }, [config, containerWidth, containerHeight, fftDataVersion, fftDataRef]); // Added fftDataRef

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
        const relevantIndices = xFreqRef.current[i]; 
        const channelFullFft = currentFftDataAllBins[i];

        if (line && relevantIndices && channelFullFft) {
          if (line.numPoints !== relevantIndices.length) {
            continue; 
          }
          for (let j = 0; j < relevantIndices.length; j++) {
            const fftBinIndex = relevantIndices[j];
            let currentMagnitude = channelFullFft[fftBinIndex];

            if (!isFinite(currentMagnitude)) {
              currentMagnitude = 0;
            }
            currentMagnitude = Math.max(0, currentMagnitude);
            const cappedMagnitude = Math.min(currentMagnitude, DATA_Y_MAX);
            
            const clampedPositiveMagnitude = Math.max(cappedMagnitude, Y_AXIS_LOG_MIN_POWER_CLAMP);

            let logMagnitude = Math.log10(clampedPositiveMagnitude);

            logMagnitude = Math.max(LOG_Y_MIN_DISPLAY, Math.min(logMagnitude, LOG_Y_MAX_DISPLAY));

            let normalizedMagnitude = (logMagnitude - LOG_Y_MIN_DISPLAY) / (LOG_Y_MAX_DISPLAY - LOG_Y_MIN_DISPLAY) - 0.5;

            if (!isFinite(normalizedMagnitude)) {
                normalizedMagnitude = -0.5; 
            }

            if (line.xy && (j * 2 + 1) < line.xy.length) {
                line.xy[j * 2 + 1] = normalizedMagnitude;
            }
          }
        } else if (line) {
          if (line.xy) {
            for (let j = 0; j < line.numPoints; j++) {
                if ((j * 2 + 1) < line.xy.length) {
                    line.xy[j * 2 + 1] = -0.5; 
                }
            }
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
  }, [fftLinesReady, fftDataVersion, config, fftDataRef, targetFps, plotWidth, plotHeight]); 

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