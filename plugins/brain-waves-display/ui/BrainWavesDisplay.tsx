import React from 'react';
import { AppletFftRenderer } from './AppletFftRenderer';
import { useEegData } from '../../../kiosk/src/context/EegDataContext';

interface BrainWavesDisplayProps {
    containerWidth?: number;
    containerHeight?: number;
}

const BrainWavesDisplay: React.FC<BrainWavesDisplayProps> = ({
    containerWidth = 600, // Default width
    containerHeight = 400, // Default height
}) => {
    const { config, fftData } = useEegData();

    return (
        <div style={{ width: containerWidth, height: containerHeight, display: 'flex', flexDirection: 'column' }}>
            <h3>Brain Waves FFT Display</h3>
            
            {config && config.channels && config.sample_rate ? (
                <AppletFftRenderer
                    config={config}
                    fftData={fftData}
                    containerWidth={containerWidth}
                    containerHeight={containerHeight - 50} // Minimal adjustment for title
                />
            ) : (
                <p>EEG Config not available or incomplete. Cannot render FFT.</p>
            )}
        </div>
    );
};

export default BrainWavesDisplay;