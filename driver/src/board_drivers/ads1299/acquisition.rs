//! Acquisition logic for the ADS1299 driver.

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use log::{info, warn, debug, error};
use std::collections::VecDeque;

use crate::board_drivers::types::{AdcConfig, AdcData, DriverStatus, DriverError, DriverEvent};
use super::helpers::{ch_raw_to_voltage, current_timestamp_micros, read_data_from_spi};
use super::registers::{CMD_SDATAC, CMD_RDATAC, CMD_START, CMD_STOP};
use super::spi::{SpiDevice, InputPinDevice, send_command_to_spi};

/// Data passed from the interrupt handler to the processing task
pub enum InterruptData {
    /// Sample data with sample count
    Data(Vec<i32>, u64),
    /// Error message
    Error(String),
}

/// Type alias for SPI device
pub type SpiType = Box<dyn SpiDevice>;

/// Type alias for DRDY pin
pub type DrdyPinType = Box<dyn InputPinDevice>;

/// Type alias for ADS1299 configuration
pub type Ads1299Config = AdcConfig;

/// Start data acquisition from the ADS1299.
///
/// This method validates the driver state, initializes the ADS1299 chip,
/// and spawns a background task that reads data from the chip.
pub async fn start_acquisition(
    inner_arc: Arc<Mutex<super::driver::Ads1299Inner>>,
    tx: mpsc::Sender<DriverEvent>,
    interrupt_running: Arc<AtomicBool>,
    mut spi: Option<SpiType>,
    mut drdy_pin: Option<DrdyPinType>,
) -> Result<(Option<JoinHandle<()>>, Option<JoinHandle<()>>, SpiType, DrdyPinType), DriverError> {
    // Check if already running
    {
        let inner = inner_arc.lock().await;
        if inner.running {
            return Err(DriverError::ConfigurationError(
                "Acquisition already running".to_string(),
            ));
        }
    }

    // Initialize chip
    initialize_chip(&mut spi, &inner_arc).await?;
    
    // Set start time and update status
    let start_time = current_timestamp_micros()?;
    {
        let mut inner = inner_arc.lock().await;
        inner.running = true;
        inner.status = DriverStatus::Running;
        inner.base_timestamp = Some(start_time);
        inner.sample_count = 0;
    }
    
    // Notify status change
    notify_status_change(&inner_arc, &tx).await?;
    
    // Check DRDY pin state
    if let Some(drdy_pin_ref) = &drdy_pin {
        let drdy_state = if drdy_pin_ref.is_high() { "HIGH" } else { "LOW" };
        debug!("DRDY pin state before starting conversion: {}", drdy_state);
        if !drdy_pin_ref.is_high() {
            warn!("DRDY pin is LOW before starting conversion, which is unexpected. This may indicate a hardware issue.");
        }
    }
    
    // Start conversion
    start_conversion(&mut spi)?;
    debug!("Conversion started");
    
    // Set up continuous data mode
    if let Some(spi_ref) = spi.as_mut() {
        if let Err(e) = send_command_to_spi(spi_ref, CMD_SDATAC) {
            error!("Failed to send SDATAC command: {:?}", e);
            return Err(e);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        if let Err(e) = send_command_to_spi(spi_ref, CMD_RDATAC) {
            error!("Failed to send RDATAC command: {:?}", e);
            return Err(e);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    }
    
    debug!("Starting acquisition using hardware interrupts in continuous data mode");
    
    // Get configuration
    let config = {
        let inner = inner_arc.lock().await;
        inner.config.clone()
    };
    
    let batch_size = config.batch_size;
    let num_channels = config.channels.len();
    
    info!(
        "Starting acquisition with batch size: {}, sample rate: {} Hz",
        batch_size, config.sample_rate
    );
    info!("Buffering {} samples before sending batches", batch_size);
    
    // Create channel for communication between interrupt handler and processing task
    let (data_tx, data_rx) = mpsc::channel::<InterruptData>(batch_size);
    
    // Set interrupt running flag
    interrupt_running.store(true, Ordering::SeqCst);
    
    // Take ownership of hardware interfaces
    let drdy_pin_unwrapped = drdy_pin.take().ok_or(DriverError::NotInitialized)?;
    let spi_unwrapped = spi.take().ok_or(DriverError::NotInitialized)?;
    
    // Spawn tasks
    let (interrupt_task, processing_task) = spawn_interrupt_and_processing_tasks(
        config,
        batch_size,
        num_channels,
        inner_arc.clone(),
        tx.clone(),
        data_tx,
        data_rx,
        interrupt_running.clone(),
        drdy_pin_unwrapped,
        spi_unwrapped,
    );
    
    info!("Ads1299Driver acquisition started");
    Ok((Some(interrupt_task), Some(processing_task), spi_unwrapped, drdy_pin_unwrapped))
}

/// Stop data acquisition from the ADS1299.
///
/// This method signals the acquisition task to stop, waits for it to complete,
/// and updates the driver status.
pub async fn stop_acquisition(
    inner_arc: Arc<Mutex<super::driver::Ads1299Inner>>,
    tx: &mpsc::Sender<DriverEvent>,
    interrupt_running: &Arc<AtomicBool>,
    interrupt_task: &mut Option<JoinHandle<()>>,
    task_handle: &mut Option<JoinHandle<()>>,
    spi: &mut Option<SpiType>,
) -> Result<(), DriverError> {
    // Signal the acquisition task to stop
    {
        let mut inner = inner_arc.lock().await;
        
        if !inner.running {
            debug!("Stop acquisition called, but acquisition was not running");
            return Ok(());
        }
        
        inner.running = false;
        debug!("Signaled acquisition task to stop");
    }
    
    // Signal the interrupt thread to stop
    if interrupt_running.load(Ordering::SeqCst) {
        debug!("Signaling interrupt thread to stop");
        interrupt_running.store(false, Ordering::SeqCst);
        
        // Wait for the interrupt thread to complete
        if let Some(handle) = interrupt_task.take() {
            match handle.await {
                Ok(_) => debug!("Interrupt thread completed successfully"),
                Err(e) => warn!("Interrupt thread terminated with error: {:?}", e),
            }
        }
    }
    
    // Exit continuous data mode and stop conversion
    if let Some(spi_ref) = spi.as_mut() {
        // First exit RDATAC mode
        if let Err(e) = send_command_to_spi(spi_ref, CMD_SDATAC) {
            warn!("Failed to send SDATAC command during stop_acquisition: {:?}", e);
            // Continue anyway to try to stop conversion
        }
        
        // Small delay after sending command
        std::thread::sleep(std::time::Duration::from_millis(5));
        
        // Then stop conversion
        stop_conversion(spi_ref)?;
    }
    
    // Wait for the task to complete
    if let Some(handle) = task_handle.take() {
        match handle.await {
            Ok(_) => debug!("Acquisition task completed successfully"),
            Err(e) => warn!("Acquisition task terminated with error: {}", e),
        }
    }
    
    // Update driver status
    {
        let mut inner = inner_arc.lock().await;
        inner.status = DriverStatus::Stopped;
        inner.sample_count = 0;
        // Keep the base_timestamp as it is - we'll set a new one when acquisition starts again
    }
    
    // Notify about the status change
    notify_status_change(&inner_arc, tx).await?;
    info!("Ads1299Driver acquisition stopped");
    Ok(())
}

/// Initialize the ADS1299 chip with the current configuration.
async fn initialize_chip(
    spi: &mut Option<SpiType>,
    inner_arc: &Arc<Mutex<super::driver::Ads1299Inner>>,
) -> Result<(), DriverError> {
    // Implementation will be added in the driver.rs file
    // This is a placeholder for now
    Ok(())
}

/// Start conversion on the ADS1299.
fn start_conversion(spi: &mut Option<SpiType>) -> Result<(), DriverError> {
    // Send START command (0x08)
    if let Some(spi_ref) = spi.as_mut() {
        send_command_to_spi(spi_ref, CMD_START)?;
    }
    Ok(())
}

/// Stop conversion on the ADS1299.
fn stop_conversion(spi: &mut dyn SpiDevice) -> Result<(), DriverError> {
    // Send STOP command (0x0A)
    send_command_to_spi(spi, CMD_STOP)?;
    Ok(())
}

/// Internal helper to notify status changes over the event channel.
///
/// This method sends a status change event to any listeners.
async fn notify_status_change(
    inner_arc: &Arc<Mutex<super::driver::Ads1299Inner>>,
    tx: &mpsc::Sender<DriverEvent>,
) -> Result<(), DriverError> {
    // Get current status
    let status = {
        let inner = inner_arc.lock().await;
        inner.status
    };
    
    debug!("Sending status change notification: {:?}", status);
    
    // Send the status change event
    tx
        .send(DriverEvent::StatusChange(status))
        .await
        .map_err(|e| DriverError::Other(format!("Failed to send status change: {}", e)))
}

/// Spawn the interrupt handler and async processing task
fn spawn_interrupt_and_processing_tasks(
    config: Ads1299Config,
    batch_size: usize,
    num_channels: usize,
    inner_arc: Arc<Mutex<super::driver::Ads1299Inner>>,
    tx: mpsc::Sender<DriverEvent>,
    data_tx: mpsc::Sender<InterruptData>,
    mut data_rx: mpsc::Receiver<InterruptData>,
    interrupt_running: Arc<AtomicBool>,
    mut drdy_pin: DrdyPinType,
    mut spi: SpiType,
) -> (JoinHandle<()>, JoinHandle<()>) {
    use rppal::gpio::Trigger;

    // Spawn a Tokio task for async interrupt polling and batching
    let interrupt_task = tokio::spawn(async move {
        let mut sample_count = 0;
        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: usize = 5;
        let mut ring_buffer: VecDeque<(Vec<i32>, u64)> = VecDeque::with_capacity(batch_size * 2);

        // Poll interval for DRDY pin (short for low latency)
        let poll_interval = Duration::from_millis(10);

        loop {
            if !interrupt_running.load(Ordering::SeqCst) {
                break;
            }
            // Poll DRDY pin (simulate async interrupt)
            match drdy_pin.poll_interrupt(true, Some(std::time::Duration::from_millis(10))) {
                Ok(Some(event)) if event.trigger == Trigger::FallingEdge => {
                    match read_data_from_spi(&mut spi, num_channels) {
                        Ok(samples) => {
                            consecutive_errors = 0;
                            ring_buffer.push_back((samples, sample_count));
                            sample_count += 1;
                            // If enough samples for a batch, send them
                            if ring_buffer.len() >= batch_size {
                                let mut batch = Vec::with_capacity(batch_size);
                                for _ in 0..batch_size {
                                    if let Some((s, c)) = ring_buffer.pop_front() {
                                        batch.push((s, c));
                                    }
                                }
                                // Flatten batch and send to async processor
                                for (samples, count) in batch {
                                    if let Err(e) = data_tx.try_send(InterruptData::Data(samples, count)) {
                                        error!("Sample dropped: channel full in interrupt handler: {}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Error reading data in interrupt handler: {:?}", e);
                            consecutive_errors += 1;
                            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                error!("Critical error: {} consecutive SPI failures", consecutive_errors);
                                let _ = data_tx.try_send(InterruptData::Error(format!(
                                    "Critical error: {} consecutive SPI failures", consecutive_errors
                                )));
                                break;
                            }
                        }
                    }
                }
                Ok(Some(event)) => {
                    warn!("Unexpected interrupt event: {:?}", event);
                }
                Ok(None) => {
                    // No data ready, just continue
                }
                Err(e) => {
                    error!("Error polling for interrupt: {:?}", e);
                    sleep(Duration::from_millis(10)).await;
                }
            }
            sleep(poll_interval).await;
        }
        debug!("Async interrupt handler task terminated");
        if let Err(e) = drdy_pin.clear_interrupt() {
            error!("Failed to clear interrupt: {:?}", e);
        }
    });

    // Spawn a Tokio task to process the data from the interrupt handler
    let processing_task = tokio::spawn(async move {
        // Get base timestamp
        let base_timestamp = {
            let inner = inner_arc.lock().await;
            inner.base_timestamp.expect("Base timestamp should be set")
        };
        
        // Main acquisition loop
        // Buffer to accumulate raw samples and timestamps before creating AdcData
        // Each inner Vec will store batch_size samples for a single channel
        let mut raw_sample_buffer: Vec<VecDeque<i32>> = vec![VecDeque::with_capacity(batch_size); num_channels];
        let mut voltage_sample_buffer: Vec<VecDeque<f32>> = vec![VecDeque::with_capacity(batch_size); num_channels];
        let mut timestamps: VecDeque<u64> = VecDeque::with_capacity(batch_size);
        
        debug!("Starting Tokio task to process data from interrupt handler");
        
        while let Some(interrupt_msg) = data_rx.recv().await {
            match interrupt_msg {
                InterruptData::Data(raw_samples, sample_count) => {
                    // Normal data path
                    // Check if we should continue running
                    let should_continue = {
                        let inner = inner_arc.lock().await;
                        inner.running
                    };

                    if !should_continue {
                        // Send any remaining samples in the buffer before breaking
                        if !timestamps.is_empty() {
                            debug!("Sending final batch of {} samples before stopping", timestamps.len());

                            // Create AdcData with the accumulated samples
                            let data = AdcData {
                                timestamp: *timestamps.last().unwrap_or(&0),
                                raw_samples: raw_sample_buffer.iter().map(|v| v.iter().copied().collect()).collect(),
                                voltage_samples: voltage_sample_buffer.iter().map(|v| v.iter().copied().collect()).collect(),
                            };

                            if let Err(e) = tx.send(DriverEvent::Data(vec![data])).await {
                                error!("Ads1299Driver event channel closed: {}", e);
                            }
                        }
                        break;
                    }

                    // Calculate timestamp
                    let sample_interval = (1_000_000 / config.sample_rate) as u64;
                    let timestamp = base_timestamp + sample_count * sample_interval;
                    timestamps.push_back(timestamp);

                    // Process raw samples and add to buffers (transposed structure)
                    for (i, &raw) in raw_samples.iter().enumerate() {
                        if i < num_channels {
                            // Add raw sample to buffer
                            raw_sample_buffer[i].push_back(raw);
                            
                            // Convert raw sample to voltage and add to buffer
                            let channel_idx = config.channels[i];
                            let voltage = ch_raw_to_voltage(raw, config.Vref, config.gain);
                            voltage_sample_buffer[i].push_back(voltage);
                        }
                    }

                    // Send the batch when we've collected batch_size samples
                    if timestamps.len() >= batch_size {
                        debug!("Sending batch of {} samples (sample_count: {})",
                            timestamps.len(), sample_count);

                        // Create AdcData with the accumulated samples
                        let data = AdcData {
                            timestamp: *timestamps.last().unwrap_or(&0),
                            raw_samples: raw_sample_buffer.iter().map(|v| v.iter().copied().collect()).collect(),
                            voltage_samples: voltage_sample_buffer.iter().map(|v| v.iter().copied().collect()).collect(),
                        };

                        if let Err(e) = tx.send(DriverEvent::Data(vec![data])).await {
                            error!("Ads1299Driver event channel closed: {}", e);
                            break;
                        }

                        // Clear buffers for next batch
                        for channel in &mut raw_sample_buffer {
                            channel.clear();
                        }
                        for channel in &mut voltage_sample_buffer {
                            channel.clear();
                        }
                        timestamps.clear();
                    }
                }
                InterruptData::Error(msg) => {
                    error!("Received critical hardware error signal from interrupt thread: {}", msg);
                    if let Err(e) = tx.send(DriverEvent::Error(msg)).await {
                        error!("Failed to send error event: {}", e);
                    }
                    // Update driver status
                    let mut inner = inner_arc.lock().await;
                    inner.status = DriverStatus::Error;
                    break; // Exit the processing loop
                }
            }
        }
        
        debug!("Tokio processing task terminated");
    });
    
    (interrupt_task, processing_task)
}