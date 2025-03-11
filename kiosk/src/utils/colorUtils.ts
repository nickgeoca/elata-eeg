'use client';

import { BASE_CHANNEL_COLORS } from './eegConstants';

// Function to get color for any channel index
export const getChannelColor = (index: number): number[] => {
  if (index < BASE_CHANNEL_COLORS.length) {
    return BASE_CHANNEL_COLORS[index];
  }
  
  // Generate colors for additional channels using HSL color space
  // This ensures good color separation for visualization
  const hue = (index * 137.5) % 360; // Golden angle approximation for good distribution
  const saturation = 0.75;
  const lightness = 0.5;
  
  // Convert HSL to RGB
  const c = (1 - Math.abs(2 * lightness - 1)) * saturation;
  const x = c * (1 - Math.abs((hue / 60) % 2 - 1));
  const m = lightness - c / 2;
  
  let r, g, b;
  if (hue < 60) {
    [r, g, b] = [c, x, 0];
  } else if (hue < 120) {
    [r, g, b] = [x, c, 0];
  } else if (hue < 180) {
    [r, g, b] = [0, c, x];
  } else if (hue < 240) {
    [r, g, b] = [0, x, c];
  } else if (hue < 300) {
    [r, g, b] = [x, 0, c];
  } else {
    [r, g, b] = [c, 0, x];
  }
  
  return [r + m, g + m, b + m, 1];
};