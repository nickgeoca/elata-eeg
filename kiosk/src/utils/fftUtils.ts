import FFT from 'fft.js';

/**
 * Calculates the next power of two for a given number.
 * @param n The input number.
 * @returns The next power of two.
 */
function getNextPowerOfTwo(n: number): number {
  if (n <= 0) return 1; // Or throw an error, depending on desired behavior for non-positives
  let p = 1;
  while (p < n) {
    p <<= 1;
  }
  // Ensure FFT size is at least 2, as fft.js might have issues with N=1
  return Math.max(2, p);
}

/**
 * Applies a Hann window to an array of numbers.
 * @param data The input data array.
 * @returns A new array with the Hann window applied.
 */
function applyHannWindow(data: number[]): number[] {
  const N = data.length;
  const windowedData = new Array(N);
  for (let i = 0; i < N; i++) {
    const multiplier = 0.5 * (1 - Math.cos((2 * Math.PI * i) / (N - 1)));
    windowedData[i] = data[i] * multiplier;
  }
  return windowedData;
}

/**
 * Calculates the Fast Fourier Transform (FFT) for a given window of EEG data.
 *
 * @param dataWindow An array of numbers representing a window of raw EEG samples.
 * @param sampleRate The sample rate of the EEG data in Hz.
 * @returns An array of numbers representing the power spectrum (magnitudes).
 *          The length of the output will be dataWindow.length / 2.
 */
export function calculateFft(dataWindow: number[], sampleRate: number): number[] {
  if (!dataWindow || dataWindow.length === 0) {
    console.warn('[fftUtils] calculateFft called with empty or invalid dataWindow.');
    return [];
  }

  // 1. Apply a windowing function (e.g., Hann window)
  const originalWindowedData = applyHannWindow([...dataWindow]);
  const originalLength = originalWindowedData.length;

  if (originalLength === 0) {
    console.warn('[fftUtils] calculateFft: windowedData is empty after applying Hann window.');
    return [];
  }

  // 2. Determine FFT size (must be power of 2 and >= 2) and pad data
  const N_fft = getNextPowerOfTwo(originalLength);
  
  let paddedData: number[];
  if (originalLength === N_fft) {
    paddedData = originalWindowedData;
  } else {
    paddedData = new Array(N_fft).fill(0);
    for (let i = 0; i < originalLength; i++) {
      paddedData[i] = originalWindowedData[i];
    }
    // console.log(`[fftUtils] Padded data from ${originalLength} to ${N_fft}`);
  }

  // 3. Perform the FFT
  const f = new FFT(N_fft);
  const out = f.createComplexArray(); // Output complex array
  f.realTransform(out, paddedData);   // Perform FFT on (potentially padded) real data

  // 4. Calculate the power spectrum (magnitudes)
  // The output of realTransform is packed. For N_fft input points, it produces N_fft/2 magnitudes.
  const numMagnitudeBins = N_fft / 2;
  const magnitudes = new Array(numMagnitudeBins);

  // DC component (0 Hz) - out[0] is Re(DC)
  // Normalize by N_fft. For one-sided spectrum, DC is not multiplied by 2.
  magnitudes[0] = Math.abs(out[0]) / N_fft;

  // AC components
  // For fft.js's `realTransform`, the layout is:
  // out[0] = Re(DC)
  // out[1] = Re(Nyquist) (if N_fft is even)
  // out[2*i] = Re(freq_i) for i = 1...N_fft/2-1
  // out[2*i+1] = Im(freq_i) for i = 1...N_fft/2-1
  for (let i = 1; i < numMagnitudeBins; i++) {
    // For the last bin (Nyquist frequency) if N_fft is even, it's stored in out[1]
    if (N_fft % 2 === 0 && i === numMagnitudeBins - 1) {
      magnitudes[i] = Math.abs(out[1]) / N_fft; // Not multiplied by 2
    } else {
      const real = out[i * 2];
      const imag = out[i * 2 + 1];
      // For one-sided spectrum, AC components are multiplied by 2.
      magnitudes[i] = (2 * Math.sqrt(real * real + imag * imag)) / N_fft;
    }
  }
  
  // Ensure the output array has exactly N_fft/2 elements.
  // This should now be guaranteed by the loop structure and numMagnitudeBins.
  // The typical output length for magnitudes is floor(N/2) + 1 if DC and Nyquist are distinct.
  // Or N/2 if Nyquist is not specially handled or N is odd.
  // fft.js realTransform is designed to give N/2 frequency bins of magnitude.

  return magnitudes;
}

/**
 * Generates an array of frequency values corresponding to the bins of an FFT output.
 *
 * @param numFftBins The number of frequency bins in the FFT output (typically N/2).
 * @param sampleRate The sample rate of the original data in Hz.
 * @returns An array of frequency values in Hz.
 */
export function getFrequencyBins(numFftBins: number, sampleRate: number): number[] {
  const frequencies = new Array(numFftBins);
  // The total number of points in the original FFT input data (N) is numFftBins * 2
  const N = numFftBins * 2; 
  const frequencyResolution = sampleRate / N;
  for (let i = 0; i < numFftBins; i++) {
    frequencies[i] = i * frequencyResolution;
  }
  return frequencies;
}