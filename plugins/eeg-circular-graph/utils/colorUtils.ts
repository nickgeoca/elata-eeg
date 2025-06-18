export const getChannelColor = (index: number): number[] => {
  const BASE_CHANNEL_COLORS = [
    [1, 0, 0, 1],    // Red
    [0, 1, 0, 1],    // Green
    [0, 0, 1, 1],    // Blue
    [1, 1, 0, 1],    // Yellow
    [1, 0, 1, 1],    // Magenta
    [0, 1, 1, 1],    // Cyan
    [1, 0.5, 0, 1],  // Orange
    [0.5, 0, 1, 1]   // Purple
  ];

  if (index < BASE_CHANNEL_COLORS.length) {
    return BASE_CHANNEL_COLORS[index];
  }
  
  // Generate colors for additional channels using HSL
  const hue = (index * 137.5) % 360;
  const saturation = 0.75;
  const lightness = 0.5;
  
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