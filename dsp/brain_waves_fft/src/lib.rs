use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;
use std::f32::consts::PI; // Added for Hann window

// WebSocket handling imports
use warp::ws::{Message, WebSocket};
use warp::Filter;
use serde::{Serialize, Deserialize};
use futures_util::{StreamExt, SinkExt};
use std::sync::Arc;
use tokio::sync::broadcast;
use eeg_driver::AdcConfig;

// Import EegBatchData from the driver crate to avoid duplication
pub use eeg_driver::EegBatchData;

/// Configuration shared with DSP modules
#[derive(Clone, Debug)]
pub struct DspSharedConfig {
    // For now, this can be empty or contain daemon-specific config
    // In the future, this might contain paths, ports, or other shared settings
}

/// FFT result for a single channel
#[derive(Serialize, Debug, Clone)]
pub struct ChannelFftResult {
    pub power: Vec<f32>,
    pub frequencies: Vec<f32>,
}

/// Response structure for brain waves FFT WebSocket
#[derive(Serialize)]
pub struct BrainWavesAppletResponse {
    pub timestamp: u64,
    pub fft_results: Vec<ChannelFftResult>,
    pub error: Option<String>,
}

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

/// Sets up the brain waves FFT WebSocket endpoint
///
/// This function creates a warp filter that handles WebSocket connections
/// for the brain waves FFT applet. It processes EEG data and responds with FFT results.
///
/// # Arguments
///
/// * `config` - Shared configuration for DSP modules
/// * `eeg_data_rx` - Broadcast receiver for EEG batch data
/// * `adc_config_rx` - Broadcast receiver for ADC configuration updates
///
/// # Returns
///
/// A warp filter that can be combined with other routes
pub fn setup_fft_websocket_endpoint(
    _config: &DspSharedConfig, // Remains for now, though unused
    eeg_data_tx: broadcast::Sender<EegBatchData>,
    adc_config_tx: broadcast::Sender<AdcConfig>, // For subscribing to updates
    shared_adc_config_arc: Arc<tokio::sync::Mutex<AdcConfig>>, // For getting initial state
) -> warp::filters::BoxedFilter<(impl warp::Reply,)> {
    warp::path("applet")
        .and(warp::path("brain_waves"))
        .and(warp::path("data"))
        .and(warp::ws())
        .and(warp::any().map(move || eeg_data_tx.subscribe()))
        .and(warp::any().map(move || adc_config_tx.subscribe()))
        .and(warp::any().map(move || shared_adc_config_arc.clone())) // Pass the Arc
        .map(|ws: warp::ws::Ws, eeg_rx: broadcast::Receiver<EegBatchData>, config_rx: broadcast::Receiver<AdcConfig>, initial_config_arc_cloned: Arc<tokio::sync::Mutex<AdcConfig>>| {
            ws.on_upgrade(move |socket| handle_brain_waves_fft_websocket(socket, eeg_rx, config_rx, initial_config_arc_cloned))
        })
        .boxed()
}

/// Handles the brain waves FFT WebSocket connection
async fn handle_brain_waves_fft_websocket(
    ws: WebSocket,
    mut rx_eeg: broadcast::Receiver<EegBatchData>,
    mut rx_config: broadcast::Receiver<AdcConfig>, // For updates
    shared_adc_config_arc: Arc<tokio::sync::Mutex<AdcConfig>>, // For initial state
) {
    println!("[BW_FFT_HANDLER_TRACE] Entered handle_brain_waves_fft_websocket");
    let (mut ws_tx, mut ws_rx) = ws.split();
    println!("[BW_FFT_HANDLER_TRACE] WebSocket split into sender and receiver.");
    println!("Brain Waves FFT WebSocket client connected"); // Existing log

    const FFT_WINDOW_DURATION_SECONDS: f32 = 1.0; // Process 1 second of data for FFT
    const FFT_WINDOW_SLIDE_SECONDS: f32 = 0.5; // Slide window by 0.5 seconds (50% overlap if duration is 1s)

    let mut channel_buffers: Vec<Vec<f32>> = Vec::new();
    let mut num_channels = 0;
    let mut sample_rate_f32 = 250.0; // Default sample rate
    let mut fft_window_samples = 0;
    let mut fft_slide_samples = 0;

    // Function to reinitialize based on config
    let reinitialize = |
        num_channels: &mut usize,
        sample_rate_f32: &mut f32,
        channel_buffers: &mut Vec<Vec<f32>>,
        fft_window_samples: &mut usize,
        fft_slide_samples: &mut usize,
        config: &AdcConfig
    | {
        println!("[BW_FFT_HANDLER_TRACE] reinitialize called.");
        *num_channels = config.channels.len();
        *sample_rate_f32 = config.sample_rate as f32;
        *channel_buffers = vec![Vec::new(); *num_channels];
        *fft_window_samples = (*sample_rate_f32 * FFT_WINDOW_DURATION_SECONDS).round() as usize;
        *fft_slide_samples = (*sample_rate_f32 * FFT_WINDOW_SLIDE_SECONDS).round() as usize;
        
        println!(
            "Brain Waves FFT: Initialized for {} channels, sample rate: {} Hz",
            *num_channels, *sample_rate_f32
        );
        println!(
            "Brain Waves FFT: Window size: {} samples, Slide size: {} samples",
            *fft_window_samples, *fft_slide_samples
        );
    };

    // Get initial config directly from the shared Arc<Mutex<AdcConfig>>
    println!("[BW_FFT_HANDLER_TRACE] Attempting to get initial config from shared Arc<Mutex<AdcConfig>>.");
    let initial_config_from_arc = {
        let guard = shared_adc_config_arc.lock().await;
        guard.clone()
    };
    println!("[BW_FFT_HANDLER_TRACE] Cloned initial config from Arc: {:?}", initial_config_from_arc);
    reinitialize(&mut num_channels, &mut sample_rate_f32, &mut channel_buffers, &mut fft_window_samples, &mut fft_slide_samples, &initial_config_from_arc);

    // The following try_recv() is now less critical for initial setup,
    // but we can keep it to see if any broadcast happened *just* after subscription and before this.
    // It's unlikely to succeed if the Arc method worked.
    match rx_config.try_recv() {
        Ok(initial_config_broadcast) => {
            println!("[BW_FFT_HANDLER_TRACE] Initial config also received via try_recv() (unexpected but handled): {:?}", initial_config_broadcast);
            // Optionally, re-reinitialize or just log if it differs, though Arc should be canonical.
            // For now, just log. If it's different, it implies a race condition or very fast update.
            if initial_config_broadcast != initial_config_from_arc {
                println!("[BW_FFT_HANDLER_TRACE] WARNING: Config from try_recv() differs from Arc config. Arc: {:?}, Broadcast: {:?}", initial_config_from_arc, initial_config_broadcast);
            }
        }
        Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
            println!("[BW_FFT_HANDLER_TRACE] rx_config.try_recv() found channel empty (expected after Arc init).");
        }
        Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
            println!("[BW_FFT_HANDLER_TRACE] rx_config.try_recv() found channel closed.");
        }
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
            println!("[BW_FFT_HANDLER_TRACE] rx_config.try_recv() found channel lagged by {}.", n);
        }
    }


    if num_channels == 0 { // This check should ideally not be true if Arc init worked
        println!("Brain Waves FFT: Warning - num_channels is 0 after Arc init, check config. Waiting for broadcast update.");
    }

    println!("[BW_FFT_HANDLER_TRACE] Entering main select loop.");
    loop {
        tokio::select! {
            // Handle EEG data
            Ok(eeg_batch_data) = rx_eeg.recv() => {
                println!("[BW_FFT_HANDLER_TRACE] Received EegBatchData.");
                // Check for errors in the EEG data
                if let Some(err_msg) = &eeg_batch_data.error {
                    println!("[BW_FFT_HANDLER_TRACE] EegBatchData contains error: {}", err_msg);
                    println!("Brain Waves FFT: Received error in EegBatchData: {}", err_msg); // Existing log
                    let response = BrainWavesAppletResponse {
                        timestamp: eeg_batch_data.timestamp,
                        fft_results: Vec::new(),
                        error: Some(err_msg.clone()),
                    };
                    if let Ok(json_response) = serde_json::to_string(&response) {
                        if ws_tx.send(Message::text(json_response)).await.is_err() {
                            println!("[BW_FFT_HANDLER_TRACE] WebSocket send error failed, breaking loop.");
                            println!("Brain Waves FFT: WebSocket client disconnected while sending error."); // Existing log
                            break;
                        }
                    }
                    continue;
                }

                // Check if we have valid configuration
                if num_channels == 0 {
                    println!("[BW_FFT_HANDLER_TRACE] No configuration (num_channels == 0), skipping data processing.");
                    println!("Brain Waves FFT: No configuration available, skipping data processing"); // Existing log
                    continue;
                }

                // Check for channel count mismatch
                if eeg_batch_data.channels.len() != num_channels {
                    println!("[BW_FFT_HANDLER_TRACE] Channel count mismatch. Expected {}, got {}. Skipping.", num_channels, eeg_batch_data.channels.len());
                    println!(
                        "Brain Waves FFT: Channel count mismatch. Expected {}, got {}. Skipping this batch.", // Existing log
                        num_channels, eeg_batch_data.channels.len()
                    );
                    continue;
                }
                println!("[BW_FFT_HANDLER_TRACE] Adding data to channel buffers. Batch channel count: {}", eeg_batch_data.channels.len());
                // Add data to channel buffers
                for (i, data_vec) in eeg_batch_data.channels.iter().enumerate() {
                    if i < num_channels {
                        channel_buffers[i].extend_from_slice(data_vec);
                    }
                }

                // Process FFT for channels that have enough data
                let mut all_channel_fft_results: Vec<ChannelFftResult> = Vec::with_capacity(num_channels);
                let mut processing_error: Option<String> = None;

                for i in 0..num_channels {
                    if channel_buffers[i].len() >= fft_window_samples {
                        let window_data: Vec<f32> = channel_buffers[i][..fft_window_samples].to_vec();
                        println!("[BW_FFT_HANDLER_TRACE] Processing FFT for channel {} with {} samples.", i, window_data.len());
                        // Perform FFT
                        match process_eeg_data(&window_data, sample_rate_f32) {
                            Ok((power, frequencies)) => {
                                println!("[BW_FFT_HANDLER_TRACE] FFT success for channel {}. Power len: {}, Freq len: {}", i, power.len(), frequencies.len());
                                all_channel_fft_results.push(ChannelFftResult { power, frequencies });
                            }
                            Err(e) => {
                                println!("[BW_FFT_HANDLER_TRACE] FFT error for channel {}: {}", i, e);
                                println!("Brain Waves FFT: Error processing channel {}: {}", i, e); // Existing log
                                processing_error = Some(format!("FFT processing error on channel {}: {}", i, e));
                                // Add an empty result to maintain channel order if one fails
                                all_channel_fft_results.push(ChannelFftResult { power: Vec::new(), frequencies: Vec::new()});
                            }
                        }

                        // Slide the window by removing processed samples
                        if fft_slide_samples > 0 && channel_buffers[i].len() >= fft_slide_samples {
                            channel_buffers[i].drain(..fft_slide_samples);
                        }
                    } else {
                        // Not enough data for this channel yet
                        all_channel_fft_results.push(ChannelFftResult { power: Vec::new(), frequencies: Vec::new()});
                    }
                }

                // Send response if we have any results or errors
                if !all_channel_fft_results.iter().all(|res| res.power.is_empty()) || processing_error.is_some() {
                    println!("[BW_FFT_HANDLER_TRACE] Preparing to send FFT results. Has error: {}", processing_error.is_some());
                    let response = BrainWavesAppletResponse {
                        timestamp: eeg_batch_data.timestamp,
                        fft_results: all_channel_fft_results,
                        error: processing_error,
                    };

                    if let Ok(json_response) = serde_json::to_string(&response) {
                        println!("[BW_FFT_HANDLER_TRACE] Sending JSON response ({} bytes).", json_response.len());
                        if ws_tx.send(Message::text(json_response)).await.is_err() {
                            println!("[BW_FFT_HANDLER_TRACE] WebSocket send FFT results failed, breaking loop.");
                            println!("Brain Waves FFT: WebSocket client disconnected while sending FFT results."); // Existing log
                            break;
                        }
                    } else {
                        println!("[BW_FFT_HANDLER_TRACE] Failed to serialize FFT response to JSON.");
                    }
                } else {
                    println!("[BW_FFT_HANDLER_TRACE] No FFT results to send and no processing error.");
                }
            },

            // Handle configuration updates
            config_result = rx_config.recv() => {
                match config_result {
                    Ok(new_config) => {
                        println!("[BW_FFT_HANDLER_TRACE] Successfully received new AdcConfig: {:?}", new_config);
                        println!("Brain Waves FFT: Received config update - reinitializing"); // Existing log
                        reinitialize(&mut num_channels, &mut sample_rate_f32, &mut channel_buffers, &mut fft_window_samples, &mut fft_slide_samples, &new_config);
                    }
                    Err(e) => {
                        println!("[BW_FFT_HANDLER_TRACE] Error receiving AdcConfig from rx_config.recv(): {:?}", e);
                        // If the channel is closed, we can't get any more configs, so break.
                        if matches!(e, tokio::sync::broadcast::error::RecvError::Closed) {
                            println!("[BW_FFT_HANDLER_TRACE] AdcConfig broadcast channel is closed. Breaking loop.");
                            break;
                        }
                        // For RecvError::Lagged, we might have missed some messages.
                        // We'll log it and continue, hoping a future config update might arrive.
                        // Or, if initial config was missed, this handler might remain unconfigured.
                    }
                }
            },

            // Handle incoming WebSocket messages (currently not used, but good to have)
            msg = ws_rx.next() => {
                println!("[BW_FFT_HANDLER_TRACE] Received message from WebSocket client or stream ended.");
                match msg {
                    Some(Ok(msg)) => {
                        println!("[BW_FFT_HANDLER_TRACE] Message content: {:?}", msg);
                        if msg.is_text() {
                            println!("[BW_FFT_HANDLER_TRACE] Received text message: {}", msg.to_str().unwrap_or_default());
                        } else if msg.is_binary() {
                            println!("[BW_FFT_HANDLER_TRACE] Received binary message (len: {}).", msg.as_bytes().len());
                        } else if msg.is_ping() {
                            println!("[BW_FFT_HANDLER_TRACE] Received ping message.");
                        } else if msg.is_pong() {
                            println!("[BW_FFT_HANDLER_TRACE] Received pong message.");
                        }

                        if msg.is_close() {
                            println!("[BW_FFT_HANDLER_TRACE] WebSocket client requested close. Breaking loop.");
                            println!("Brain Waves FFT: WebSocket client requested close"); // Existing log
                            break;
                        }
                        // For now, we don't handle any incoming messages from the client
                        // In the future, this could be used for custom FFT configuration
                    }
                    Some(Err(e)) => {
                        println!("[BW_FFT_HANDLER_TRACE] WebSocket receive error: {}. Breaking loop.", e);
                        println!("Brain Waves FFT: WebSocket error: {}", e); // Existing log
                        break;
                    }
                    None => {
                        println!("[BW_FFT_HANDLER_TRACE] WebSocket stream ended (None received). Breaking loop.");
                        println!("Brain Waves FFT: WebSocket stream ended"); // Existing log
                        break;
                    }
                }
            }
        }
    }
    println!("[BW_FFT_HANDLER_TRACE] Exited main select loop.");
    println!("Brain Waves FFT: WebSocket client disconnected"); // Existing log
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