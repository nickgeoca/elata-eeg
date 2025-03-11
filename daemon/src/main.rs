use eeg_driver::{AdcConfig, EegSystem, DriverType};
use tokio::sync::broadcast;
use warp::ws::{Message, WebSocket};
use warp::Filter;
use serde::{Serialize, Deserialize};
use futures_util::{StreamExt, SinkExt};

#[derive(Clone, Serialize)]
struct EegData {
    channels: Vec<f32>,
    timestamp: u64,
}

#[derive(Clone, Serialize)]
struct EegBatchData {
    channels: Vec<Vec<f32>>,  // Each inner Vec represents a channel's data for the batch
    timestamp: u64,           // Timestamp for the start of the batch
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Increase channel capacity but not too much to avoid excessive buffering
    let (tx, _) = broadcast::channel::<EegBatchData>(32);  // Reduced from 1024
    let tx_ws = tx.clone();

    // Create the ADC configuration
    let config = AdcConfig {
        sample_rate: 250,
        channels: vec![0, 1, 2, 3],
        gain: 4.0,
        board_driver: DriverType::Mock,
        batch_size: 32,
    };

    println!("Starting EEG system...");
    
    // Create and start the EEG system
    let (mut eeg_system, mut data_rx) = EegSystem::new(config.clone()).await?;
    eeg_system.start(config.clone()).await?;

    println!("EEG system started. Waiting for data...");

    // Clone the config for the config WebSocket route
    let config_for_ws = config.clone();
    
    // Set up WebSocket routes
    let eeg_ws_route = warp::path("eeg")
        .and(warp::ws())
        .and(warp::any().map(move || tx_ws.subscribe()))
        .map(|ws: warp::ws::Ws, mut rx: broadcast::Receiver<EegBatchData>| {
            ws.on_upgrade(move |socket| handle_websocket(socket, rx))
        });
        
    let config_ws_route = warp::path("config")
        .and(warp::ws())
        .and(warp::any().map(move || config_for_ws.clone()))
        .map(|ws: warp::ws::Ws, config: AdcConfig| {
            ws.on_upgrade(move |socket| handle_config_websocket(socket, config))
        });
    
    let ws_routes = eeg_ws_route.or(config_ws_route);

    println!("WebSocket server starting on:");
    println!("- ws://localhost:8080/eeg (EEG data)");
    println!("- ws://localhost:8080/config (Configuration)");

    // Spawn WebSocket server
    let server_handle = tokio::spawn(warp::serve(ws_routes).run(([127, 0, 0, 1], 8080)));

    // Process EEG data
    let processing_handle = tokio::spawn(async move {
        let mut count = 0;
        let mut last_time = std::time::Instant::now();
        let mut last_timestamp = None;
        
        while let Some(data) = data_rx.recv().await {
            // Create smaller batches to send more frequently
            // Split the incoming data into chunks of 32 samples
            let batch_size = 32;
            let num_channels = data.data.len();
            let samples_per_channel = data.data[0].len();
            
            for chunk_start in (0..samples_per_channel).step_by(batch_size) {
                let chunk_end = (chunk_start + batch_size).min(samples_per_channel);
                let mut chunk_channels = Vec::with_capacity(num_channels);
                
                for channel in &data.data {
                    chunk_channels.push(channel[chunk_start..chunk_end].to_vec());
                }
                
                let chunk_timestamp = data.timestamp + (chunk_start as u64 * 4000); // Adjust timestamp for each chunk
                
                let eeg_batch_data = EegBatchData {
                    channels: chunk_channels,
                    timestamp: chunk_timestamp / 1000, // Convert to milliseconds
                };
                
                if let Err(e) = tx.send(eeg_batch_data) {
                    println!("Warning: Failed to send data chunk to WebSocket clients: {}", e);
                }
            }
            
            count += data.data[0].len();
            last_timestamp = Some(data.timestamp);
            
            if let Some(last_ts) = last_timestamp {
                let delta_us = data.timestamp - last_ts;
                let delta_ms = delta_us as f64 / 1000.0;  // Convert to milliseconds for display
                if delta_ms > 5.0 {
                    println!("Large timestamp gap detected: {:.2}ms ({} µs)", delta_ms, delta_us);
                    println!("Sample count: {}", count);
                    println!("Expected time between batches: {:.2}ms", (32_000.0 / 250.0)); // For 32 samples at 250Hz
                }
            }
            
            // Print stats every 250 samples (about 1 second of data at 250Hz)
            if count % 250 == 0 {
                let elapsed = last_time.elapsed();
                let rate = 250.0 / elapsed.as_secs_f32();
                println!("Processing rate: {:.2} Hz", rate);
                println!("Total samples processed: {}", count);
                println!("Sample data (first 5 values from first channel):");
                println!("  Channel 0: {:?}", &data.data[0][..5]);
                last_time = std::time::Instant::now();
            }
        }
    });

    // Wait for tasks to complete
    tokio::select! {
        _ = processing_handle => println!("Processing task completed"),
        _ = server_handle => println!("Server task completed"),
    }

    // Cleanup
    eeg_system.stop().await?;
    
    Ok(())
}

/// Creates a binary EEG packet according to the specified format:
/// [timestamp (8 bytes)] [channel1_samples...] [channel2_samples...] [channel3_samples...] [channel4_samples...]
fn create_eeg_binary_packet(eeg_batch_data: &EegBatchData) -> Vec<u8> {
    // Get timestamp in milliseconds
    let timestamp = eeg_batch_data.timestamp;
    
    // We'll use exactly 4 channels as per spec
    let num_channels = 4;
    let samples_per_channel = eeg_batch_data.channels[0].len();
    
    // Calculate buffer size: 8 bytes for timestamp + 4 bytes per float per channel
    let buffer_size = 8 + (num_channels * samples_per_channel * 4);
    let mut buffer = Vec::with_capacity(buffer_size);
    
    // Write timestamp (8 bytes) in little-endian format
    buffer.extend_from_slice(&timestamp.to_le_bytes());
    
    // Write each channel's samples (exactly 4 channels)
    // If we have more than 4 channels, use only the first 4
    // If we have fewer than 4 channels, duplicate the last channel
    for channel_idx in 0..num_channels {
        let channel_data = if channel_idx < eeg_batch_data.channels.len() {
            &eeg_batch_data.channels[channel_idx]
        } else {
            // If we don't have enough channels, use the last available channel
            &eeg_batch_data.channels[eeg_batch_data.channels.len() - 1]
        };
        
        for &sample in channel_data {
            buffer.extend_from_slice(&sample.to_le_bytes());
        }
    }
    
    buffer
}

/// Handle WebSocket connection for EEG data streaming
async fn handle_websocket(ws: WebSocket, mut rx: broadcast::Receiver<EegBatchData>) {
    let (mut tx, _) = ws.split();
    
    println!("WebSocket client connected - sending binary EEG data");
    println!("Binary format: [timestamp (8 bytes)] [channel1_samples...] [channel2_samples...] [channel3_samples...] [channel4_samples...]");
    
    let mut packet_count = 0;
    let start_time = std::time::Instant::now();
    
    while let Ok(eeg_batch_data) = rx.recv().await {
        // Create binary packet
        let binary_data = create_eeg_binary_packet(&eeg_batch_data);
        let packet_size = binary_data.len();
        let samples_count = eeg_batch_data.channels[0].len();
        
        // Send binary message
        if let Err(_) = tx.send(Message::binary(binary_data)).await {
            println!("WebSocket client disconnected");
            break;
        }
        
        packet_count += 1;
        
        // Log stats every 100 packets
        if packet_count % 100 == 0 {
            let elapsed = start_time.elapsed().as_secs_f32();
            let rate = packet_count as f32 / elapsed;
            println!("Sent {} binary packets at {:.2} Hz", packet_count, rate);
            println!("Last packet size: {} bytes", packet_size);
            println!("Samples per channel: {}", samples_count);
        }
    }
}

/// Handle WebSocket connection for configuration data
async fn handle_config_websocket(ws: WebSocket, config: AdcConfig) {
    let (mut tx, _) = ws.split();
    
    println!("Configuration WebSocket client connected");
    
    // Convert the configuration to JSON and send it
    if let Ok(config_json) = serde_json::to_string(&config) {
        if let Err(e) = tx.send(Message::text(config_json)).await {
            println!("Error sending configuration: {}", e);
        } else {
            println!("Configuration sent successfully");
            println!("Sample rate: {}", config.sample_rate);
            println!("Channels: {:?}", config.channels);
            println!("Gain: {}", config.gain);
            println!("Board driver: {:?}", config.board_driver);
            println!("Batch size: {}", config.batch_size);
        }
    } else {
        println!("Error serializing configuration");
    }
    
    // Keep the connection open but don't send any more data
    // The client can disconnect when it's done
}
