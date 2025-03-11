'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import throttle from 'lodash.throttle';
import { ScrollingBuffer } from '../utils/ScrollingBuffer';
import { DEFAULT_SAMPLE_RATE, DEFAULT_BATCH_SIZE } from '../utils/eegConstants';

interface EegDataHandlerProps {
  config: any;
  onDataUpdate: (dataReceived: boolean) => void;
  dataRef: React.MutableRefObject<ScrollingBuffer[]>;
  windowSizeRef: React.MutableRefObject<number>;
  debugInfoRef: React.MutableRefObject<{
    lastPacketTime: number;
    packetsReceived: number;
    samplesProcessed: number;
  }>;
  renderNeededRef: React.MutableRefObject<boolean>;
  latestTimestampRef: React.MutableRefObject<number>;
}

export function useEegDataHandler({
  config,
  onDataUpdate,
  dataRef,
  windowSizeRef,
  debugInfoRef,
  renderNeededRef,
  latestTimestampRef
}: EegDataHandlerProps) {
  const [status, setStatus] = useState('Connecting...');
  const wsRef = useRef<WebSocket | null>(null);
  const handleMessageRef = useRef<any>(null);
  const dataReceivedTimeoutRef = useRef<NodeJS.Timeout | null>(null);
  const lastTimestampRef = useRef<number>(Date.now());
  const isProduction = process.env.NODE_ENV === 'production';

  // Calculate optimal throttle interval based on config
  const getThrottleInterval = useCallback(() => {
    if (config && config.sample_rate && config.batch_size) {
      // Calculate frame interval based on sample rate and batch size
      // This gives us the time between frames in milliseconds
      return Math.max(8, 1000 / (config.sample_rate / config.batch_size));
    }
    return 16; // Default to ~60fps
  }, [config]);

  // Ensure all buffers are initialized - but only when necessary
  useEffect(() => {
    // Initialize buffers for all channels
    const channelCount = config?.channels?.length || 4;
    
    // Only reinitialize if channel count changed or buffers not initialized
    const needsReinitialization =
      dataRef.current.length !== channelCount ||
      dataRef.current.length === 0 ||
      (dataRef.current[0] && dataRef.current[0].getCapacity() !== windowSizeRef.current);
    
    if (needsReinitialization) {
      dataRef.current = Array(channelCount).fill(null).map(() =>
        new ScrollingBuffer(windowSizeRef.current)
      );
      
      // Set render needed flag to ensure all channels are drawn
      renderNeededRef.current = true;
      
      if (!isProduction) {
        console.log(`Initialized ${channelCount} channel buffers in useEffect`);
      }
    }
  }, [config, dataRef, windowSizeRef, renderNeededRef, isProduction]);

  // Create message handler function
  const createMessageHandler = useCallback(() => {
    const interval = getThrottleInterval();
    if (!isProduction) {
      console.log(`Setting throttle interval to ${interval.toFixed(2)}ms (${(1000/interval).toFixed(2)} FPS)`);
    }
    
    // Cancel previous handler if it exists
    if (handleMessageRef.current) {
      handleMessageRef.current.cancel();
    }
    
    // Create new throttled handler
    const handler = throttle((event: MessageEvent) => {
      try {
        // For binary data format
        if (event.data instanceof ArrayBuffer) {
          const dataView = new DataView(event.data);
          
          // First 8 bytes are the timestamp (as BigInt64)
          let timestamp = Number(dataView.getBigInt64(0, true));
          const now = Date.now();
          lastTimestampRef.current = now;
          
          // Only convert seconds to milliseconds if needed
          const timeDiff = Math.abs(timestamp - now);
          if (timeDiff > 10000) {
            if (!isProduction) {
              console.log(`Timestamp adjustment: ${timestamp} -> ${timestamp < 1000000000000 ? timestamp * 1000 : timestamp}`);
            }
            
            // Only convert seconds to milliseconds if needed
            if (timestamp < 1000000000000) { // If timestamp is less than year 2001 in ms
              timestamp = timestamp * 1000; // Convert to milliseconds
            }
          }
          
          // Update the latest timestamp reference for our rendering window
          latestTimestampRef.current = timestamp;
          
          // Calculate how many samples per channel
          const channelCount = config?.channels?.length || 4;
          const samplesPerChannel = (event.data.byteLength - 8) / 4 / channelCount; // channelCount channels, 4 bytes per float
          const sampleRate = config?.sample_rate || DEFAULT_SAMPLE_RATE;
          const sampleInterval = 1000 / sampleRate;
          
          // Update debug info
          const debugInfo = debugInfoRef.current;
          debugInfo.packetsReceived++;
          debugInfo.samplesProcessed += samplesPerChannel * channelCount; // Use dynamic channel count
          
          // Set data received indicator
          onDataUpdate(true);
          
          // Clear previous timeout if it exists
          if (dataReceivedTimeoutRef.current) {
            clearTimeout(dataReceivedTimeoutRef.current);
          }
          
          // Reset data received indicator after 500ms of no data
          dataReceivedTimeoutRef.current = setTimeout(() => {
            onDataUpdate(false);
          }, 500);
          
          // Set flag to indicate rendering is needed
          renderNeededRef.current = true;
          
          // Process each channel - optimized for performance
          // Use the channelCount already defined above
          for (let ch = 0; ch < channelCount; ch++) {
            if (!dataRef.current[ch]) {
              if (!isProduction) {
                console.warn(`Channel ${ch} buffer not initialized!`);
              }
              continue;
            }
            
            // Pre-calculate base offset for this channel to avoid repeated calculations
            const channelBaseOffset = 8 + (ch * samplesPerChannel * 4);
            
            // Process all samples for this channel in a single loop
            for (let i = 0; i < samplesPerChannel; i++) {
              const offset = channelBaseOffset + (i * 4);
              const value = dataView.getFloat32(offset, true); // true for little-endian
              
              // Fast path for valid values (most common case)
              if (isFinite(value) && Math.abs(value) <= 10) {
                dataRef.current[ch].push(value);
                continue;
              }
              
              // Handle edge cases
              if (isNaN(value) || !isFinite(value)) {
                if (!isProduction) {
                  console.warn(`Invalid value for channel ${ch}: ${value}`);
                }
                dataRef.current[ch].push(0);
              } else {
                // Clamp large values
                dataRef.current[ch].push(Math.max(-3, Math.min(3, value)));
              }
            }
          }
        }
        // Fallback for JSON data (for backward compatibility)
        else {
          const data = JSON.parse(event.data);
          lastTimestampRef.current = Date.now();
          
          // Process JSON data more efficiently
          data.channels.forEach((channel: number[], channelIndex: number) => {
            if (!dataRef.current[channelIndex]) return;
            
            // Process all values at once
            for (let i = 0; i < channel.length; i++) {
              const value = channel[i];
              if (isFinite(value) && Math.abs(value) <= 10) {
                dataRef.current[channelIndex].push(value);
              } else if (isNaN(value) || !isFinite(value)) {
                dataRef.current[channelIndex].push(0);
              } else {
                dataRef.current[channelIndex].push(Math.max(-3, Math.min(3, value)));
              }
            }
          });
          
          // Set flag to indicate rendering is needed
          renderNeededRef.current = true;
        }
      } catch (error) {
        console.error('WebSocket error:', error);
      }
    }, interval, { trailing: true });
    
    handleMessageRef.current = handler;
    return handler;
  }, [config, getThrottleInterval, onDataUpdate, dataRef, debugInfoRef, renderNeededRef, latestTimestampRef]);

  // Update window size when config changes
  useEffect(() => {
    if (config) {
      // Add safeguard for sample rate as suggested in the code review
      const safeSampleRate = Math.max(1, config.sample_rate || DEFAULT_SAMPLE_RATE);
      windowSizeRef.current = Math.ceil((safeSampleRate * 2000) / 1000); // 2000ms window
      
      // Get channel count from config
      const channelCount = config?.channels?.length || 4;
      
      // Reinitialize buffers with new size - always do this to ensure consistency
      dataRef.current = Array(channelCount).fill(null).map(() =>
        new ScrollingBuffer(windowSizeRef.current)
      );
      
      // Set render needed flag to ensure all channels are drawn
      renderNeededRef.current = true;
      
      if (!isProduction) {
        console.log(`Updated window size to ${windowSizeRef.current} based on sample rate ${safeSampleRate}Hz`);
        console.log(`Reinitialized ${channelCount} channel buffers`);
      }
      
      // Recreate message handler with new throttle interval
      if (wsRef.current) {
        const handler = createMessageHandler();
        wsRef.current.onmessage = handler;
      }
    }
  }, [config, createMessageHandler, dataRef, windowSizeRef, renderNeededRef]);

  // WebSocket connection
  useEffect(() => {
    // Initialize buffers if not already done
    if (dataRef.current.length === 0) {
      const channelCount = config?.channels?.length || 4;
      dataRef.current = Array(channelCount).fill(null).map(() =>
        new ScrollingBuffer(windowSizeRef.current)
      );
      
      // Set render needed flag to ensure all channels are drawn on initial setup
      renderNeededRef.current = true;
    }
    
    const ws = new WebSocket('ws://localhost:8080/eeg');
    wsRef.current = ws;
    
    // Set binary type for WebSocket
    ws.binaryType = 'arraybuffer';
    
    ws.onopen = () => setStatus('Connected');
    
    // Create message handler with current config
    const handler = createMessageHandler();
    ws.onmessage = handler;
    
    ws.onclose = () => setStatus('Disconnected');
    ws.onerror = () => setStatus('Error');
    
    return () => {
      if (handleMessageRef.current) {
        handleMessageRef.current.cancel();
      }
      ws.close();
      wsRef.current = null;
    };
  }, [createMessageHandler, config, dataRef, windowSizeRef]);

  // Calculate FPS from config
  const fps = config ? (config.sample_rate / config.batch_size) : (DEFAULT_SAMPLE_RATE / DEFAULT_BATCH_SIZE);

  return { status, fps };
}