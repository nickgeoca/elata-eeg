import React, { useEffect, useState, useRef } from 'react';
import { AppletFftRenderer } from './AppletFftRenderer';
import { calculateFft } from '../../../kiosk/src/utils/fftUtils'; // Adjusted path

// FFT Processing parameters
const FFT_WINDOW_DURATION_MS = 2000; // e.g., 2 seconds
const FFT_HOP_DURATION_MS = 1000;   // e.g., 1 second overlap

// Define the expected structure of the data from the WebSocket
// This should match the updated BrainWavesAppletResponse from daemon/src/server.rs
interface BrainWavesAppletResponse {
    timestamp: number;
    channels: number[][]; // Array of channel data arrays (raw voltage samples)
    error?: string | null;
}

interface BrainWavesDisplayProps {
    containerWidth?: number;
    containerHeight?: number;
    eegConfig: {
        sample_rate: number;
        channels: number[] | { channel_number: number; // Or whatever the actual structure is
                                is_active: boolean
                              }[];
        // Add other properties of eegConfig if known and used
    };
}

const BrainWavesDisplay: React.FC<BrainWavesDisplayProps> = ({
    containerWidth = 600, // Default width
    containerHeight = 400, // Default height
    eegConfig
}) => {
    const [data, setData] = useState<BrainWavesAppletResponse | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [ws, setWs] = useState<WebSocket | null>(null);

    const fftDataRef = useRef<Record<number, number[]>>({});
    const [fftDataVersion, setFftDataVersion] = useState(0);
    const channelBuffersRef = useRef<Record<number, number[]>>({}); // To accumulate samples per channel
    const lastProcessTimeRef = useRef<Record<number, number>>({}); // To track last FFT processing time per channel

    useEffect(() => {
        // Construct WebSocket URL
        // The daemon is expected to run on localhost:8080
        // The path is from the manifest: /applet/brain_waves/data
        const socketUrl = `ws://${window.location.hostname}:8080/applet/brain_waves/data`;
        const socket = new WebSocket(socketUrl);
        setWs(socket);

        socket.onopen = () => {
            console.log('Brain Waves Applet WebSocket connected');
            setError(null);
        };

        socket.onmessage = (event) => {
            try {
                const message: BrainWavesAppletResponse = JSON.parse(event.data as string);
                setData(message);

                if (message.error) {
                    console.error('Brain Waves Applet error:', message.error);
                    setError(`Applet Error: ${message.error}`);
                    fftDataRef.current = {};
                    channelBuffersRef.current = {};
                    setFftDataVersion((prev: number) => prev + 1);
                    return;
                }

                if (message.channels && eegConfig && eegConfig.sample_rate) {
                    const sampleRate = eegConfig.sample_rate;
                    const fftWindowSize = Math.floor(FFT_WINDOW_DURATION_MS / 1000 * sampleRate);
                    const fftHopSize = Math.floor(FFT_HOP_DURATION_MS / 1000 * sampleRate);
                    let newFftDataAvailable = false;

                    message.channels.forEach((channelSamples, channelIndex) => {
                        if (!channelBuffersRef.current[channelIndex]) {
                            channelBuffersRef.current[channelIndex] = [];
                        }
                        channelBuffersRef.current[channelIndex].push(...channelSamples);

                        // Initialize last process time if not set
                        if (lastProcessTimeRef.current[channelIndex] === undefined) {
                            lastProcessTimeRef.current[channelIndex] = Date.now();
                        }

                        // Check if enough data and time since last FFT
                        while (channelBuffersRef.current[channelIndex].length >= fftWindowSize &&
                               (Date.now() - (lastProcessTimeRef.current[channelIndex] || 0)) >= FFT_HOP_DURATION_MS) {

                            const dataWindow = channelBuffersRef.current[channelIndex].slice(0, fftWindowSize);
                            const psd = calculateFft(dataWindow, sampleRate);
                            
                            if (psd && psd.length > 0) {
                                fftDataRef.current[channelIndex] = psd;
                                newFftDataAvailable = true;
                            }
                            
                            // Slide the buffer
                            channelBuffersRef.current[channelIndex] = channelBuffersRef.current[channelIndex].slice(fftHopSize);
                            lastProcessTimeRef.current[channelIndex] = Date.now(); // Update last process time
                        }
                    });

                    if (newFftDataAvailable) {
                        setFftDataVersion((prev: number) => prev + 1);
                    }
                }
            } catch (e) {
                console.error('Error processing Brain Waves applet message:', e);
                setError('Error processing message from applet.');
            }
        };

        socket.onerror = (err) => {
            console.error('Brain Waves Applet WebSocket error:', err);
            setError('WebSocket connection error to Brain Waves applet.');
            setData(null);
        };

        socket.onclose = () => {
            console.log('Brain Waves Applet WebSocket disconnected');
            setWs(null);
            // Optionally, implement reconnection logic here
        };

        // Clean up the WebSocket connection when the component unmounts
        return () => {
            if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) {
                socket.close();
            }
        };
    }, []); // Empty dependency array means this effect runs once on mount and cleanup on unmount

    return (
        <div style={{ width: containerWidth, height: containerHeight, display: 'flex', flexDirection: 'column' }}>
            <h3>Brain Waves FFT Display</h3>
            {error && <p style={{ color: 'red' }}>Error: {error}</p>}
            {!error && (!ws || ws.readyState !== WebSocket.OPEN) && <p>Connecting to WebSocket...</p>}
            
            {eegConfig && eegConfig.channels && eegConfig.sample_rate ? (
                <AppletFftRenderer
                    fftDataRef={fftDataRef}
                    fftDataVersion={fftDataVersion}
                    config={eegConfig} // Pass the mock/provided config
                    containerWidth={containerWidth}
                    containerHeight={containerHeight - 30} // Adjust height for title
                />
            ) : (
                <p>EEG Config not available or incomplete. Cannot render FFT.</p>
            )}

            {/* Optional: Display raw data for debugging */}
            {/* {data && (
                <details style={{ marginTop: '10px' }}>
                    <summary>Raw Data</summary>
                    <pre style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-all', maxHeight: '100px', overflowY: 'auto', fontSize: '10px' }}>
                        {JSON.stringify(data, null, 2)}
                    </pre>
                </details>
            )} */}
        </div>
    );
};

export default BrainWavesDisplay;