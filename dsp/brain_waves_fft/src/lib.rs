use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;

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

    let n = data.len();
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);

    let mut buffer: Vec<Complex<f32>> = data
        .iter()
        .map(|&x| Complex::new(x * 1_000_000.0, 0.0)) // Convert V to µV
        .collect();

    fft.process(&mut buffer);

    // Calculate power spectrum (PSD in (µV)²/Hz)
    let spectrum_len = n / 2 + 1;
    let mut power_spectrum_psd: Vec<f32> = Vec::with_capacity(spectrum_len);

    if n == 0 || sample_rate == 0.0 {
        // Fill with zeros if n or sample_rate is zero to avoid division by zero
        // and ensure the vector has the correct length.
        power_spectrum_psd.resize(spectrum_len, 0.0);
    } else {
        let norm_denominator = n as f32 * sample_rate;
        for k_idx in 0..spectrum_len {
            let val = if k_idx == 0 || (n % 2 == 0 && k_idx == n / 2) { // DC or Nyquist
                buffer[k_idx].norm_sqr() / norm_denominator
            } else { // AC components
                2.0 * buffer[k_idx].norm_sqr() / norm_denominator
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