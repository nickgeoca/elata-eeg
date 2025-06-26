'use client';
import React, { useRef, useState, useEffect, useLayoutEffect } from 'react';
import { EegRenderer } from './EegRenderer';
import { CircularGraphWrapper } from './CircularGraphWrapper';
import { FftRenderer } from '../../../plugins/brain_waves_fft/ui/FftRenderer';
import { useEegData } from '../context/EegDataContext';
import { useDataBuffer } from '../hooks/useDataBuffer';
import { SampleChunk } from '../types/eeg';
/* eslint-disable @typescript-eslint/ban-ts-comment */
// @ts-ignore: WebglStep might be missing from types but exists at runtime
import { WebglStep, ColorRGBA } from 'webgl-plot';
import { WINDOW_DURATION } from '../utils/eegConstants';

type DataView = 'signalGraph' | 'appletBrainWaves' | 'circularGraph';

interface EegDataVisualizerProps {
  activeView: DataView;
  config: any; // Consider defining a more specific type for config
  uiVoltageScaleFactor: number;
}

export default function EegDataVisualizer({ activeView, config, uiVoltageScaleFactor }: EegDataVisualizerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const dataRef = useRef<any[]>([]);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const latestTimestampRef = useRef<number>(0);
  const debugInfoRef = useRef({
    lastPacketTime: 0,
    packetsReceived: 0,
    samplesProcessed: 0,
  });
  const [viewReadyState, setViewReadyState] = useState({ signalGraph: false, circularGraph: false, appletBrainWaves: false });
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const [dataVersion, setDataVersion] = useState(0);
  const [localDataVersion, setLocalDataVersion] = useState(0);

  const circularGraphBuffer = useDataBuffer<SampleChunk>();
  const signalGraphBuffer = useDataBuffer<SampleChunk>();

  const { subscribeRaw, subscribe, unsubscribe, fftData, fullFftPacket } = useEegData();

  // Effect for raw data subscriptions (Signal and Circular graphs)
  useEffect(() => {
    if (activeView !== 'signalGraph' && activeView !== 'circularGraph') {
      return;
    }

    let unsubRaw: (() => void) | null = null;

    if (activeView === 'signalGraph') {
      console.log('[Visualizer] Subscribing to raw data for Signal Graph.');
      signalGraphBuffer.clear();
      unsubRaw = subscribeRaw((newSampleChunks) => {
        if (newSampleChunks.length > 0) {
          signalGraphBuffer.addData(newSampleChunks);
          setLocalDataVersion(v => v + 1);
        }
      });
    } else { // activeView === 'circularGraph'
      console.log('[Visualizer] Subscribing to raw data for Circular Graph.');
      circularGraphBuffer.clear();
      unsubRaw = subscribeRaw((newSampleChunks) => {
        if (newSampleChunks.length > 0) {
          circularGraphBuffer.addData(newSampleChunks);
          setLocalDataVersion(v => v + 1);
        }
      });
    }

    return () => {
      if (unsubRaw) {
        console.log(`[Visualizer] Unsubscribing from raw data for view: ${activeView}`);
        unsubRaw();
      }
    };
  }, [activeView, subscribeRaw, signalGraphBuffer, circularGraphBuffer]);

  // Effect for FFT data subscription
  useEffect(() => {
    if (activeView === 'appletBrainWaves') {
      console.log('[Visualizer] Subscribing to Fft');
      subscribe(['Fft']);
      setViewReadyState(s => ({ ...s, appletBrainWaves: true }));
      return () => {
        console.log('[Visualizer] Unsubscribing from Fft');
        unsubscribe(['Fft']);
      };
    }
  }, [activeView, subscribe, unsubscribe]);

  // Effect to setup ResizeObserver
  useLayoutEffect(() => {
    const target = containerRef.current;
    if (!target) return;

    const resizeObserver = new ResizeObserver(entries => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        setContainerSize({ width, height });
      }
    });

    resizeObserver.observe(target);
    
    // Set initial size
    setContainerSize({
        width: target.offsetWidth,
        height: target.offsetHeight,
    });

    return () => resizeObserver.disconnect();
  }, []);

  // Effect to create/update WebGL lines
  useEffect(() => {
    const numChannels = config?.channels?.length || 0;
    const sampleRate = config?.sample_rate;
    const MIN_VALID_WIDTH = 50;

    if (config && sampleRate && numChannels > 0 && containerSize.width > MIN_VALID_WIDTH) {
// Constants for scaling
  const MICROVOLT_CONVERSION_FACTOR = 1e6; // V to uV
  const REFERENCE_UV_RANGE = 100.0;
      const initialNumPoints = Math.ceil(sampleRate * (WINDOW_DURATION / 1000));
      const ySpacing = 2.0 / numChannels;

      if (dataRef.current.length !== numChannels) {
        console.log(`[Visualizer] Creating ${numChannels} WebGL lines.`);
        const lines: WebglStep[] = [];
        for (let i = 0; i < numChannels; i++) {
          const line = new WebglStep(new ColorRGBA(1, 1, 1, 1), initialNumPoints);
          line.lineSpaceX(-1, 2 / initialNumPoints);
          lines.push(line);
        }
        dataRef.current = lines;
        if (activeView === 'signalGraph') {
          setViewReadyState(s => ({ ...s, signalGraph: true }));
        }
      }

      dataRef.current.forEach((line, i) => {
        line.lineWidth = 1;
        const calculatedScaleY = ((ySpacing * MICROVOLT_CONVERSION_FACTOR) / REFERENCE_UV_RANGE) * uiVoltageScaleFactor;
        line.scaleY = calculatedScaleY;
        line.offsetY = 1 - (i + 0.5) * ySpacing;
      });

      setDataVersion(v => v + 1);
    } else {
      if (dataRef.current.length > 0) {
        dataRef.current = [];
        setDataVersion(v => v + 1);
      }
    }
  }, [config?.channels?.length, config?.sample_rate, containerSize.width, uiVoltageScaleFactor, activeView]);

  return (
    <div ref={containerRef} className="w-full h-full relative bg-gray-950">
      {containerSize.width > 0 && containerSize.height > 0 ? (
        <>
          {activeView === 'signalGraph' && (
            !config || !config.channels || !Array.isArray(config.channels) || config.channels.length === 0 ? (
              <div className="absolute inset-0 flex items-center justify-center text-gray-400">
                Waiting for channel configuration...
              </div>
            ) : (
              <div className="relative h-full min-h-[300px]">
                <EegRenderer
                  isActive={activeView === 'signalGraph'}
                  canvasRef={canvasRef}
                  dataRef={dataRef}
                  config={config}
                  latestTimestampRef={latestTimestampRef}
                  debugInfoRef={debugInfoRef}
                  dataBuffer={signalGraphBuffer}
                  containerWidth={containerSize.width}
                  containerHeight={containerSize.height}
                  dataVersion={dataVersion}
                />
                <canvas ref={canvasRef} className="absolute top-0 left-0 w-full h-full" />
              </div>
            )
          )}

          {activeView === 'circularGraph' && (
            <CircularGraphWrapper
                isActive={activeView === 'circularGraph'}
                config={config}
                containerWidth={containerSize.width}
                containerHeight={containerSize.height}
                dataBuffer={circularGraphBuffer}
                dataVersion={dataVersion}
            />
          )}

          {activeView === 'appletBrainWaves' && (
            <FftRenderer
              data={fullFftPacket as any}
              isActive={activeView === 'appletBrainWaves'}
              containerWidth={containerSize.width}
              containerHeight={containerSize.height}
            />
          )}
        </>
      ) : (
        <div className="absolute inset-0 flex items-center justify-center text-gray-400">
            Initializing...
        </div>
      )}
    </div>
  );
}