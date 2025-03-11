'use client';

// Default constants (will be overridden by server config when available)
export const DEFAULT_SAMPLE_RATE = 250;
export const DEFAULT_BATCH_SIZE = 32;
export const WINDOW_DURATION = 2000; // ms
export const GRAPH_HEIGHT = 100;
export const GRAPH_WIDTH = 400;

// Base colors for the first 4 channels
export const BASE_CHANNEL_COLORS = [
  [0.13, 0.59, 0.95, 1], // #2196f3 - Blue
  [0.30, 0.69, 0.31, 1], // #4caf50 - Green
  [0.96, 0.26, 0.21, 1], // #f44336 - Red
  [0.61, 0.15, 0.69, 1]  // #9c27b0 - Purple
];

export const VOLTAGE_TICKS = [-1.5, -0.75, 0, 0.75, 1.5];
export const TIME_TICKS = [0, 0.5, 1.0, 1.5, 2.0];