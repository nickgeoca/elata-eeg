from scipy.signal import butter, filtfilt
import pandas as pd
import numpy as np

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

from scipy.signal import butter, iirnotch, filtfilt


def onboard_filter(signal, Fs, bandpass_order, lowcut, highcut, notch_q):
    # bandpass
    nyq = 0.5 * Fs
    b, a = butter(bandpass_order, [lowcut / nyq, highcut / nyq], btype='band')
    signal = filtfilt(b, a, signal)
    
    nyq = 0.5 * Fs
    b, a = iirnotch(60.0 / nyq, notch_q)
    signal = filtfilt(b, a, signal)
    
    nyq = 0.5 * Fs
    b, a = iirnotch(50.0 / nyq, notch_q)
    signal = filtfilt(b, a, signal)
    
    return signal

def get_fft(signal, Fs):
    N = len(signal)
    fft_vals = np.fft.fft(signal)
    fft_freqs = np.fft.fftfreq(N, d=1/Fs)
    fft_vals = fft_vals[fft_freqs > 0]
    fft_freqs = fft_freqs[fft_freqs > 0]
    fft_power = np.abs(fft_vals) ** 2 / N  # Power spectrum (V²)
    return fft_vals, fft_freqs, fft_power