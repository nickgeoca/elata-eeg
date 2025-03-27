from scipy.signal import butter, filtfilt
import pandas as pd
import numpy as np
from scipy.signal import butter, lfilter, iirnotch, sosfilt_zi, sosfilt

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
    import numpy as np
    
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
    
    # Add x-label only to the bottom subplot
    axes[-1].set_xlabel("Frequency (Hz)")
    
    # Adjust layout
    fig.tight_layout()
    
    return fig, axes