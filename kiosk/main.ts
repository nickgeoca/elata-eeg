// Connect to WebSocket
const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws';
const host = window.location.hostname;
const port = window.location.port || (window.location.protocol === 'https:' ? '443' : '80');
const ws = new WebSocket(`${protocol}://${host}:${port}`);

// Handle incoming data
ws.onmessage = (event) => {
    const data = JSON.parse(event.data);
    if (data.channel_data) {
        // Handle EEG data chunk
        updateDisplay(data.channel_data);
        console.log(`Received data chunk #${data.sequence_number}`);
    } else {
        // Handle command response
        updateStatus(data.message);
        console.log(data.message);
    }
};

function updateDisplay(channelData: number[]) {
    const display = document.getElementById('eegDisplay');
    if (display) {
        // Update visualization here
        display.innerHTML = `Current EEG Values: ${channelData.join(', ')}`;
    }
}

function updateStatus(message: string) {
    const status = document.getElementById('status');
    if (status) {
        status.textContent = message;
    }
}