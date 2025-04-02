import numpy as np
from scipy.signal import butter, lfilter, filtfilt, iirnotch, welch

# Low-pass filter
def butter_lowpass_filter(data, Fs, cutoff_hz=4, order=4):
    nyquist = 0.5 * Fs
    normal_cutoff = cutoff_hz / nyquist
    b, a = butter(order, normal_cutoff, btype='low', analog=False)
    return filtfilt(b, a, data)

def butter_bandpass_filter(data, Fs, lowcut_hz=0.5, highcut_hz=45, order=4):
    b, a = butter(order, [lowcut_hz / (0.5 * Fs), highcut_hz / (0.5 * Fs)], btype='band')
    return filtfilt(b, a, data)

def raw_to_volts(data, vref, avss, gain):
    vmid = (vref + avss) / 2
    return vmid + (data / (2**23)) * ((vref - avss) / gain)

def get_board_attributes(fname):
    board = fname.split('_')[3]
    gain = float(fname.split('_')[2][4:])
    Fs = 250.0
    if board == 'boardAds1299':
        vref = 4.5
        avss = 0
        resolution = 24
    vmid = (vref + avss) / 2
    return board, gain, Fs, vref, avss, resolution, vmid

def add_time_and_raw_voltage_columns(df, fname):
    new_cols = []
    board, gain, Fs, vref, avss, resolution, vmid = get_board_attributes(fname)
    df['time_sec'] = (df['timestamp'] - df['timestamp'].iloc[0]) / 1000000.
    new_cols.append('time_sec')
    raw_cols = [i for i in list(df.columns) if i.find('_raw_sample') != -1]
    for raw_col in raw_cols:
        ch = raw_col.split('_')[0]
        df[f"{ch}_raw_voltage"] = raw_to_volts(df[raw_col], vref, avss, gain)
        new_cols.append(f"{ch}_raw_voltage")
    print(f"added these new columns, {new_cols}")
    return df


def get_fft(signal, Fs):
    N = len(signal)
    fft_vals = np.fft.fft(signal)
    fft_freqs = np.fft.fftfreq(N, d=1/Fs)
    fft_vals = fft_vals[fft_freqs > 0]
    fft_freqs = fft_freqs[fft_freqs > 0]
    fft_power = np.abs(fft_vals) ** 2 / N  # Power spectrum (V²)
    return fft_vals, fft_freqs, fft_power

def plot_raw_voltages(df, channels):
    import matplotlib.pyplot as plt
    import matplotlib.ticker as ticker

    filtered_signals = {}
    
    # Plot all filtered channels
    plt.figure(figsize=(8, 6))
    for ch in channels:
        plt.plot(df['time_sec'], df[ch], label=ch, linewidth=1.5)
    
    plt.xlabel("Time (s)")
    plt.ylabel("Voltage (V)")
    plt.title("Raw EEG Voltages")
    plt.legend()
    plt.grid()
    plt.tight_layout()
    
    # Define a custom formatter function that will never use scientific notation
    def custom_formatter(x, pos):
        return f'{x:.4g}'  # Use general format with 4 significant digits
    
    ax = plt.gca()
    ax.yaxis.set_major_formatter(ticker.FuncFormatter(custom_formatter))
    plt.show()

def plot_power_spectrum_welch(df, channels, Fs, lowcut, highcut, order, max_x_axis_freq):
    import matplotlib.pyplot as plt
    import matplotlib.ticker as ticker

    # Create figure BEFORE the loop
    plt.figure(figsize=(10, 6))
    
    # Process and plot each channel
    for col in channels:
        voltage_filtered = butter_bandpass_filter(df[col].values, Fs, lowcut, highcut, order)
        freq, psd = welch(
            voltage_filtered,
            fs=Fs,
            window='hamming',
            nperseg=min(4096, len(voltage_filtered)),  # Ensure nperseg is not larger than signal
            noverlap=min(2048, len(voltage_filtered)//2),  # Adjust overlap accordingly
            scaling='density'
        )
        plt.semilogy(freq, psd, label=col)
    
    # EEG bands
    eeg_bands = {
        "Delta (0.5–4 Hz)": (0.5, 4),
        "Theta (4–8 Hz)": (4, 8),
        "Alpha (8–13 Hz)": (8, 13),
        "Beta (13–30 Hz)": (13, 30),
        "Gamma (30–45 Hz)": (30, 45),
    }
    colors = ['gray', 'purple', 'green', 'orange', 'red']
    for (label, (f_min, f_max)), color in zip(eeg_bands.items(), colors):
        plt.axvspan(f_min, f_max, color=color, alpha=0.2, label=label)
    
    # Add labels and formatting
    plt.xlabel("Frequency (Hz)")
    plt.ylabel("Power Spectral Density (V²/Hz)")
    plt.title("EEG Power Spectrum using Welch's Method")
    plt.grid(True)
    
    # Set x-axis limit to focus on relevant frequencies
    plt.xlim(0, max_x_axis_freq)  # Limit to either highcut or 80Hz, whichever is smaller
    
    # Add legend with best placement
    plt.legend(loc='best')
    plt.tight_layout()
    
    # For log scale plots, we need a different approach for formatting
    ax = plt.gca()
    # This will format the y-axis ticks without scientific notation
    # ax.yaxis.set_major_formatter(ticker.ScalarFormatter())
    # ax.yaxis.get_major_formatter().set_scientific(False)
    # ax.yaxis.get_major_formatter().set_useOffset(False)
    plt.show()
   
def get_raw_volt_cols(df):
    import re
    return [col for col in df.columns if re.match(r'ch\d+_raw_voltage', col)]
    
def eeg_board_filter(signal, Fs, order, lowcut, highcut, notch_q):
    from scipy.signal import lfilter_zi
    
    nyq = 0.5 * Fs
    
    # Highpass filter with proper state initialization
    high_b, high_a = butter(order, lowcut / nyq, btype='highpass')
    zi_high = lfilter_zi(high_b, high_a) * signal[0]  # Initialize with first sample
    signal, _ = lfilter(high_b, high_a, signal, axis=0, zi=zi_high)
    
    # Notch filter 50Hz with state initialization
    notch_b, notch_a = iirnotch(50.0 / nyq, notch_q)
    zi_notch50 = lfilter_zi(notch_b, notch_a) * signal[0]
    signal, _ = lfilter(notch_b, notch_a, signal, axis=0, zi=zi_notch50)
    
    # Notch filter 60Hz with state initialization
    notch_b, notch_a = iirnotch(60.0 / nyq, notch_q)
    zi_notch60 = lfilter_zi(notch_b, notch_a) * signal[0]
    signal, _ = lfilter(notch_b, notch_a, signal, axis=0, zi=zi_notch60)
    
    # Lowpass filter with state initialization
    low_b, low_a = butter(order, highcut / nyq, btype='lowpass')
    zi_low = lfilter_zi(low_b, low_a) * signal[0]
    signal, _ = lfilter(low_b, low_a, signal, axis=0, zi=zi_low)
    
    return signal


def plot_cwt(df_col, Fs, freq_min=1, freq_max=64, num_freqs=128):
    """
    Plot a continuous wavelet transform (CWT) scalogram for a single EEG channel.
    
    Parameters:
    -----------
    df_col : pandas.Series or numpy.ndarray
        The voltage data for a single channel
    Fs : float
        Sampling frequency in Hz
    freq_min : float, optional
        Minimum frequency to analyze (default: 1 Hz)
    freq_max : float, optional
        Maximum frequency to analyze (default: 64 Hz)
    num_freqs : int, optional
        Number of frequency points to analyze (default: 128)
    """
    import numpy as np
    import matplotlib.pyplot as plt
    import pywt
    
    # Convert to numpy array if it's not already
    voltage = df_col.to_numpy() if hasattr(df_col, 'to_numpy') else np.array(df_col)
    
    # Create time array
    time = np.arange(len(voltage)) / Fs
    
    # Define Wavelet Parameters
    freqs = np.linspace(freq_min, freq_max, num_freqs)  # Frequencies to analyze
    scales = Fs / (2 * freqs)  # Convert frequencies to scales
    
    # Compute CWT using Morlet wavelet
    coefficients, frequencies = pywt.cwt(voltage, scales, 'cmor', 1/Fs)
    
    # Plot the scalogram
    plt.figure(figsize=(10, 6))
    plt.imshow(np.abs(coefficients), aspect='auto',
               extent=[time[0], time[-1], freqs[0], freqs[-1]],
               cmap='jet', origin='lower')
    plt.colorbar(label="Magnitude")
    plt.xlabel("Time (s)")
    plt.ylabel("Frequency (Hz)")
    plt.title(f"CWT Scalogram of {df_col.name if hasattr(df_col, 'name') else ''}")
    plt.ylim(freq_min, freq_max)  # Focus on relevant frequency range
    plt.show()
    
    return coefficients, frequencies  # Return the computed values for further analysis if needed

def plot_spectrogram(df_col, Fs, highcut_hz=50, nperseg=256, noverlap=None):
    """
    Plot a spectrogram for a single EEG channel.
    
    Parameters:
    -----------
    df_col : pandas.Series or numpy.ndarray
        The voltage data for a single channel
    Fs : float
        Sampling frequency in Hz
    highcut_hz : float, optional
        Upper frequency limit for the plot (default: 50)
    nperseg : int, optional
        Length of each segment for the spectrogram (default: 256)
    noverlap : int, optional
        Number of points to overlap between segments (default: nperseg//2)
    """
    from scipy.signal import spectrogram
    import matplotlib.pyplot as plt
    import numpy as np
    
    # Convert to numpy array if it's not already
    voltage = df_col.to_numpy() if hasattr(df_col, 'to_numpy') else np.array(df_col)
    
    # Default overlap is half the segment length
    if noverlap is None:
        noverlap = nperseg // 2
    
    f, t, Sxx = spectrogram(
        voltage,
        fs=Fs,
        window='hamming',
        nperseg=nperseg,
        noverlap=noverlap,
        scaling='density'  # V^2/Hz
    )
    
    plt.figure(figsize=(12, 6))
    # Plot log power; add small constant to avoid log(0)
    plt.pcolormesh(t, f, 10 * np.log10(Sxx + 1e-10), shading='gouraud', cmap='viridis')
    plt.ylabel('Frequency (Hz)')
    plt.xlabel('Time (sec)')
    plt.title(f'Spectrogram for Channel {df_col.name if hasattr(df_col, "name") else ""}')
    plt.colorbar(label='Power Spectral Density (dB/Hz)')  # dB = 10*log10(V^2/Hz)
    plt.ylim(0, highcut_hz)  # Focus on relevant frequency range
    plt.show()
    
    return f, t, Sxx  # Return the computed values for further analysis if needed

def plot_ffts(Fs, signal_label_pairs):
    """
    Plot FFT graphs vertically for multiple signals.
    
    Parameters:
    -----------
    Fs : float
        Sampling frequency in Hz
    signal_label_pairs : list of tuples
        Each tuple contains (signal_data, label_string)
    """
    import matplotlib.pyplot as plt
    import matplotlib.ticker as ticker

    # Create figure with subplots (one row per signal)
    n_signals = len(signal_label_pairs)
    fig, axes = plt.subplots(n_signals, 1, figsize=(8, 4*n_signals), sharex=True)
    
    # If only one signal, make axes iterable
    if n_signals == 1:
        axes = [axes]
    
    # EEG bands
    eeg_bands = {
        "Delta (0.5–4 Hz)": (0.5, 4),
        "Theta (4–8 Hz)": (4, 8),
        "Alpha (8–13 Hz)": (8, 13),
        "Beta (13–30 Hz)": (13, 30),
        "Gamma (30–45 Hz)": (30, 45),
    }
    colors = ['gray', 'purple', 'green', 'orange', 'red']
    
    # Define a custom formatter function that will never use scientific notation
    def custom_formatter(x, pos):
        return f'{x:.4g}'  # Use general format with 4 significant digits
    
    # Plot each signal
    for i, ((signal, label), ax) in enumerate(zip(signal_label_pairs, axes)):
        # Calculate FFT
        fft_vals, fft_freqs, fft_power = get_fft(signal, Fs)
        N = len(signal)
        
        # Plot FFT
        ax.plot(fft_freqs, np.abs(fft_vals) / N, label=f"{label}", color='b' if i % 2 == 0 else 'g')
        
        # Add EEG bands
        for (band_label, (f_min, f_max)), color in zip(eeg_bands.items(), colors):
            ax.axvspan(f_min, f_max, color=color, alpha=0.2, label=band_label)
        
        # Add legend (with unique entries only)
        handles, labels = ax.get_legend_handles_labels()
        ax.legend(dict(zip(labels, handles)).values(), dict(zip(labels, handles)).keys())
        
        # Add labels and grid
        ax.set_ylabel("Magnitude")
        ax.set_title(label)
        ax.grid()
        
        # Set x-axis limit for all plots
        ax.set_xlim(0, 80)
        
        # Apply custom formatter to this axis
        ax.yaxis.set_major_formatter(ticker.FuncFormatter(custom_formatter))
    
    # Add x-label only to the bottom subplot
    axes[-1].set_xlabel("Frequency (Hz)")
    
    # Adjust layout
    fig.tight_layout()
    return fig, axes