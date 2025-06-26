'use client';

import React, { useEffect, useRef } from 'react';
/* eslint-disable @typescript-eslint/ban-ts-comment */
// @ts-ignore: WebglLine is missing from types but exists at runtime
import { WebglPlot, ColorRGBA, WebglLine } from 'webgl-plot';
import { getChannelColor } from '../../../kiosk/src/utils/colorUtils';

interface FftData {
  psd_packets: { channel: number; psd: number[] }[];
  fft_config: {
    fft_size: number;
    sample_rate: number;
    window_function: string;
  };
}

interface FftRendererProps {
  data: FftData | null;
  isActive: boolean;
  containerWidth: number;
  containerHeight: number;
}

export const FftRenderer: React.FC<FftRendererProps> = ({
  data,
  isActive,
  containerWidth,
  containerHeight,
}) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const wglpRef = useRef<WebglPlot | null>(null);
  const linesRef = useRef<WebglLine[]>([]);

  useEffect(() => {
    if (!canvasRef.current || !isActive || containerWidth === 0 || containerHeight === 0) {
      return;
    }

    const dpr = window.devicePixelRatio || 1;
    const physicalWidth = Math.round(containerWidth * dpr);
    const physicalHeight = Math.round(containerHeight * dpr);

    if (!wglpRef.current) {
      canvasRef.current.width = physicalWidth;
      canvasRef.current.height = physicalHeight;
      canvasRef.current.style.width = `${containerWidth}px`;
      canvasRef.current.style.height = `${containerHeight}px`;

      const wglp = new WebglPlot(canvasRef.current);
      wglpRef.current = wglp;
      linesRef.current = [];
    } else {
        if (canvasRef.current.width !== physicalWidth || canvasRef.current.height !== physicalHeight) {
            canvasRef.current.width = physicalWidth;
            canvasRef.current.height = physicalHeight;
            wglpRef.current.update();
        }
    }

    const wglp = wglpRef.current;
    if (data && data.psd_packets) {
      const numChannels = data.psd_packets.length;
      const { fft_size, sample_rate } = data.fft_config;
      const maxFreq = sample_rate / 2;

      // Create or update lines
      if (linesRef.current.length !== numChannels) {
        wglp.removeAllLines();
          linesRef.current = data.psd_packets.map((packet, i) => {
            const colorTuple = getChannelColor(packet.channel);
            const color = new ColorRGBA(colorTuple[0], colorTuple[1], colorTuple[2], 1);
            const line = new WebglLine(color, packet.psd.length);
            wglp.addLine(line);
            return line;
          });
        }
  
        // Configure plot scaling
        // Update data
        data.psd_packets.forEach((packet, i) => {
          if (linesRef.current[i]) {
            const numPoints = packet.psd.length;
            const xData = new Float32Array(numPoints);
            const yData = new Float32Array(numPoints);
            
            // units should be in uV^2/Hz ... not dB
            for (let j = 0; j < numPoints; j++) {
              xData[j] = (j * maxFreq) / (numPoints > 1 ? numPoints - 1 : 1);
              const dbValue = 10 * Math.log10(packet.psd[j]);
              // Normalize or scale Y values to fit in the -1 to 1 viewport
              yData[j] = (dbValue + 50) / 100; // Example scaling: assumes dB range is roughly -50 to 50
            }
            
            // @ts-ignore - The types seem to be wrong, but this is the likely signature
            linesRef.current[i].setY(xData, yData);
          }
        });
        
        wglp.update();
    }

  }, [data, isActive, containerWidth, containerHeight]);

  return (
    <canvas
      ref={canvasRef}
      style={{ display: isActive ? 'block' : 'none', width: '100%', height: '100%' }}
    />
  );
};