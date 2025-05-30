use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;
use std::f32::consts::PI; // Added for Hann window

// Private helper function to generate Hann window coefficients
fn generate_hann_window(n: usize) -> Vec<f32> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        // For a single point, the window value is 1.0.
        // (0.5 * (1.0 - cos(0))) = 0.5 * (1.0 - 1.0) = 0.0 if using N in denom for i=0.
        // Using N-1 in denom: 0.5 * (1.0 - cos(2*PI*0 / 0)) is problematic.
        // Standard practice for N=1 window is often [1.0].
        return vec![1.0];
    }
    (0..n)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (n - 1) as f32).cos()))
        .collect()
}

/// Processes a chunk of EEG data to calculate its power spectrum.
///
/// # Arguments
///
/// * `data` - A slice of f32 representing raw EEG data for a single channel.
/// * `sample_rate` - The sample rate of the EEG data in Hz.
///
/// # Returns
///
/// A `Result` containing a tuple of two `Vec<f32>`:
/// * The first vector is the power spectrum.
/// * The second vector is the corresponding frequency bins.
/// Returns an error string if processing fails (e.g., empty data).
pub fn process_eeg_data(data: &[f32], sample_rate: f32) -> Result<(Vec<f32>, Vec<f32>), String> {
    if data.is_empty() {
        return Err("Input data cannot be empty".to_string());
    }
    if sample_rate <= 0.0 {
        return Err("Sample rate must be positive".to_string());
    }

    let n = data.len();

    // Generate Hann window coefficients
    let hann_coeffs = generate_hann_window(n);

    // Apply Hann window and scale data (V to µV)
    // The scaling by 1_000_000.0 converts Volts to microvolts.
    // Windowing should be applied to the signal before FFT.
    let mut windowed_data_uv: Vec<Complex<f32>> = Vec::with_capacity(n);
    for i in 0..n {
        windowed_data_uv.push(Complex::new(data[i] * hann_coeffs[i] * 1_000_000.0, 0.0));
    }

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    
    // FFT operates in-place on `buffer`
    let mut buffer = windowed_data_uv; // buffer is now the windowed and µV-scaled data
    fft.process(&mut buffer);

    // Calculate sum of squares of window coefficients for PSD normalization
    let window_sum_sq: f32 = hann_coeffs.iter().map(|&w| w * w).sum();

    // If n=1, hann_coeffs=[1.0], window_sum_sq=1.0.
    // If n=0, data.is_empty() catches it.
    if window_sum_sq == 0.0 && n > 0 { // Should not happen for n > 0 with Hann window
        return Err("Window sum of squares is zero, cannot normalize PSD.".to_string());
    }
    
    // Calculate power spectrum (PSD in (µV)²/Hz)
    let spectrum_len = n / 2 + 1;
    let mut power_spectrum_psd: Vec<f32> = Vec::with_capacity(spectrum_len);

    // Normalization for PSD:
    // For DC (k=0) and Nyquist (k=N/2 if N is even): P[k] = |X[k]|^2 / (Fs * WSS)
    // For other frequencies (0 < k < N/2):         P[k] = 2 * |X[k]|^2 / (Fs * WSS)
    // where WSS = sum(window_coeffs[i]^2) and Fs is sample_rate.
    // rustfft output X[k] is not normalized by 1/N.
    // buffer[k_idx].norm_sqr() is the |X[k]|^2 term we need.
    
    let norm_denominator_psd = sample_rate * window_sum_sq;
    
    if norm_denominator_psd == 0.0 {
        // This case should ideally be prevented by earlier checks (sample_rate > 0 and window_sum_sq > 0 for n > 0)
        // Fill with zeros if normalization is not possible to maintain output structure.
        power_spectrum_psd.resize(spectrum_len, 0.0);
    } else {
        for k_idx in 0..spectrum_len {
            let val = if k_idx == 0 || (n > 0 && n % 2 == 0 && k_idx == n / 2) { // DC or Nyquist
                buffer[k_idx].norm_sqr() / norm_denominator_psd
            } else { // AC components
                2.0 * buffer[k_idx].norm_sqr() / norm_denominator_psd
            };
            power_spectrum_psd.push(val);
        }
    }

    // Generate frequency bins
    // Frequencies range from 0 Hz to Nyquist frequency (sample_rate / 2)
    let mut frequency_bins: Vec<f32> = Vec::with_capacity(spectrum_len);
    for i in 0..spectrum_len {
        frequency_bins.push(i as f32 * sample_rate / n as f32);
    }

    Ok((power_spectrum_psd, frequency_bins))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_eeg_data_simple_sine_wave() {
        let sample_rate = 100.0; // 100 Hz
        let duration = 1.0; // 1 second
        let n_samples = (sample_rate * duration) as usize;
        let frequency = 10.0; // 10 Hz sine wave

        let mut data = Vec::with_capacity(n_samples);
        for i in 0..n_samples {
            let time = i as f32 / sample_rate;
            data.push((2.0 * std::f32::consts::PI * frequency * time).sin());
        }

        match process_eeg_data(&data, sample_rate) {
            Ok((power_spectrum, frequency_bins)) => {
                assert_eq!(power_spectrum.len(), n_samples / 2 + 1);
                assert_eq!(frequency_bins.len(), n_samples / 2 + 1);

                // Find the peak frequency
                let mut max_power = 0.0;
                let mut peak_freq_bin = 0.0;
                for i in 0..power_spectrum.len() {
                    if power_spectrum[i] > max_power {
                        max_power = power_spectrum[i];
                        peak_freq_bin = frequency_bins[i];
                    }
                }
                // Allow for some tolerance due to FFT leakage and binning
                assert!((peak_freq_bin - frequency).abs() < sample_rate / n_samples as f32 * 1.5, "Peak frequency {} is not close to {}", peak_freq_bin, frequency);
            }
            Err(e) => panic!("Processing failed: {}", e),
        }
    }

    #[test]
    fn test_process_eeg_data_empty_input() {
        let data: Vec<f32> = Vec::new();
        let sample_rate = 250.0;
        assert!(process_eeg_data(&data, sample_rate).is_err());
    }

    #[test]
    fn test_frequency_bins_correctness() {
        let sample_rate = 200.0;
        let n_samples = 128;
        let data: Vec<f32> = vec![0.0; n_samples]; // Dummy data

        match process_eeg_data(&data, sample_rate) {
            Ok((_, frequency_bins)) => {
                assert_eq!(frequency_bins.len(), n_samples / 2 + 1);
                assert_eq!(frequency_bins[0], 0.0); // DC component
                assert_eq!(frequency_bins.last().unwrap(), &(sample_rate / 2.0)); // Nyquist frequency

                let expected_bin_step = sample_rate / n_samples as f32;
                for i in 0..frequency_bins.len() {
                    assert!((frequency_bins[i] - (i as f32 * expected_bin_step)).abs() < 1e-6);
                }
            }
            Err(e) => panic!("Processing failed: {}", e),
        }
    }
}
#[test]
    fn test_process_eeg_data_n_equals_one() {
        let data = [1.0_f32];
        let sample_rate = 100.0_f32;
        match process_eeg_data(&data, sample_rate) {
            Ok((power, freqs)) => {
                assert_eq!(power.len(), 1);
                assert_eq!(freqs.len(), 1);
                // For n=1, hann_coeffs is [1.0], window_sum_sq is 1.0.
                // Input data[0] = 1.0 (V). Scaled to 1e6 µV.
                // FFT output X[0] = 1e6 µV. |X[0]|^2 = 1e12 (µV)^2.
                // PSD[0] = |X[0]|^2 / (sample_rate * window_sum_sq)
                //        = 1e12 / (100.0 * 1.0) = 1e10 (µV)^2/Hz.
                assert!((power[0] - 1.0e10).abs() < 1e-3, "Expected power {} but got {}", 1.0e10, power[0]);
                assert_eq!(freqs[0], 0.0);
            }
            Err(e) => panic!("Processing failed for n=1: {}", e),
        }
    }

    #[test]
    fn test_process_eeg_data_n_equals_two() {
        let data = [1.0_f32, 0.5_f32]; // Arbitrary data for n=2
        let sample_rate = 100.0_f32;
        // For n=2, the Hann window is [0.0, 0.0] because the formula uses (n-1) in the denominator:
        // hann_coeffs[i] = 0.5 * (1.0 - cos(2.0 * PI * i / (n - 1)))
        // For i=0, n=2: 0.5 * (1.0 - cos(0)) = 0.5 * (1-1) = 0.0
        // For i=1, n=2: 0.5 * (1.0 - cos(2*PI)) = 0.5 * (1-1) = 0.0
        // This results in window_sum_sq = 0.0.
        // The function should return an error "Window sum of squares is zero, cannot normalize PSD."
        match process_eeg_data(&data, sample_rate) {
            Ok((power, freqs)) => panic!("Processing should fail for n=2 due to zero WSS, but it succeeded with power: {:?}, freqs: {:?}", power, freqs),
            Err(e) => {
                assert_eq!(e, "Window sum of squares is zero, cannot normalize PSD.");
            }
        }
    }