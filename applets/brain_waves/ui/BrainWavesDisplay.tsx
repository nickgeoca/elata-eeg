import React, { useEffect, useState, useRef } from 'react';
import { AppletFftRenderer } from './AppletFftRenderer';

// Define the expected structure of the data from the WebSocket
// This should match BrainWavesAppletResponse and BrainWavesChannelData from daemon/src/server.rs
interface BrainWavesChannelData {
    power_spectrum: number[];
    frequency_bins: number[];
}

interface BrainWavesAppletResponse {
    timestamp: number;
    channels_data: (BrainWavesChannelData | null)[];
    error?: string | null;
}

interface BrainWavesDisplayProps {
    containerWidth?: number;
    containerHeight?: number;
    // This config would ideally come from a shared context or props
    // For now, we'll define a basic one here or expect it.
    eegConfig: any;
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
                setData(message); // Keep raw data if needed for other UI elements

                if (message.error) {
                    console.error('Brain Waves Applet error:', message.error);
                    setError(`Applet Error: ${message.error}`);
                    fftDataRef.current = {}; // Clear data on error
                } else if (message.channels_data) {
                    const newFftData: Record<number, number[]> = {};
                    message.channels_data.forEach((channelData, index) => {
                        if (channelData) {
                            // Assuming channelData.power_spectrum is what FftRenderer needs
                            newFftData[index] = channelData.power_spectrum;
                        }
                    });
                    fftDataRef.current = newFftData;
                    // console.log('Processed FFT data for renderer:', newFftData);
                }
                setFftDataVersion(prev => prev + 1); // Trigger re-render in AppletFftRenderer
            } catch (e) {
                console.error('Error parsing Brain Waves applet message:', e);
                setError('Error parsing message from applet.');
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