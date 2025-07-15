use std::error::Error;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use log::{info, error, debug};

use eeg_sensor::{
    create_driver, AdcConfig, AdcDriver, DriverError, DriverStatus,
};
use eeg_types::{BridgeMsg, SensorError};
use eeg_types::Packet;

pub struct EegSystem {
    driver: Arc<tokio::sync::Mutex<Box<dyn AdcDriver>>>,
    processing_task: Option<JoinHandle<()>>,
    sensor_thread: Option<thread::JoinHandle<Result<(), SensorError>>>,
    stop_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<i32>,
    cancel_token: CancellationToken,
}

impl EegSystem {
    /// Creates an EEG processing system without starting it
    pub async fn new(
        config: AdcConfig
    ) -> Result<(Self, mpsc::Receiver<i32>), Box<dyn Error>> {
        info!("Creating new EegSystem with config");
        let driver = create_driver(config)?;
        let (tx, rx) = mpsc::channel(1024);
        let cancel_token = CancellationToken::new();

        let system = Self {
            driver: Arc::new(tokio::sync::Mutex::new(driver)),
            processing_task: None,
            sensor_thread: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            tx,
            cancel_token,
        };

        Ok((system, rx))
    }

    /// Starts the processing system with the given configuration
    pub async fn start(&mut self, config: AdcConfig) -> Result<(), Box<dyn Error>> {
        self.initialize_processing(config).await
    }

    /// Internal helper to initialize or reinitialize the driver and processing task
    async fn initialize_processing(&mut self, _config: AdcConfig) -> Result<(), Box<dyn Error>> {
        info!("Initializing EEG processing with config");

        // Cancel any existing processing task gracefully
        if self.processing_task.is_some() {
            debug!("Cancelling existing processing task");
            self.cancel_token.cancel();
            self.cancel_token = CancellationToken::new();
        }

        let (std_tx, std_rx) = std::sync::mpsc::channel::<BridgeMsg>();
        let (bridge_tx, mut bridge_rx) = mpsc::channel::<BridgeMsg>(1024);

        let driver = self.driver.clone();
        let stop_flag = self.stop_flag.clone();
        self.sensor_thread = Some(thread::spawn(move || {
            let mut driver_guard = driver.blocking_lock();
            driver_guard.acquire(std_tx, &stop_flag)
        }));

        tokio::spawn(async move {
            while let Ok(msg) = std_rx.recv() {
                if bridge_tx.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let tx = self.tx.clone();
        let cancel_token = self.cancel_token.clone();

        self.processing_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        break;
                    }
                    Some(msg) = bridge_rx.recv() => {
                        match msg {
                            BridgeMsg::Data(packet) => {
                                for sample in packet.samples {
                                    if tx.send(sample).await.is_err() {
                                        eprintln!("Error sending data");
                                        break;
                                    }
                                }
                            }
                            BridgeMsg::Error(e) => {
                                eprintln!("Sensor error: {}", e);
                            }
                        }
                    }
                    else => break,
                }
            }
        }));

        Ok(())
    }


    /// Stop the data acquisition & gracefully cancel the background task
    pub async fn stop(&mut self) -> Result<(), Box<dyn Error>> {
        debug!("Stopping EEG system");
        self.stop_flag.store(true, Ordering::Relaxed);

        if let Some(handle) = self.sensor_thread.take() {
            handle.join().unwrap()?;
        }

        if let Some(task) = self.processing_task.take() {
            debug!("Cancelling processing task");
            self.cancel_token.cancel();
            task.await?;
        }
        
        Ok(())
    }

    /// Reconfigure the driver with new settings
    pub async fn reconfigure(&mut self, config: AdcConfig) -> Result<(), Box<dyn Error>> {
        info!("Reconfiguring EEG system with new settings");
        self.stop().await?;
        
        // Create new driver with updated configuration
        let new_driver = create_driver(config.clone())?;
        self.driver = Arc::new(tokio::sync::Mutex::new(new_driver));
        
        self.initialize_processing(config).await
    }

    /// Retrieve the current driver status
    pub async fn driver_status(&self) -> DriverStatus {
        self.driver.lock().await.get_status()
    }

    /// Retrieve the driver's configuration
    pub async fn driver_config(&self) -> Result<AdcConfig, DriverError> {
        self.driver.lock().await.get_config()
    }

    /// Completely shut down the EEG system and clean up resources
    pub async fn shutdown(&mut self) -> Result<(), Box<dyn Error>> {
        const SHUTDOWN_TIMEOUT_MS: u64 = 1000;
        
        let shutdown_future = async {
            self.stop().await?;
            self.driver.lock().await.shutdown().map_err(|e| Box::new(e) as Box<dyn Error>)
        };
        
        match tokio::time::timeout(
            Duration::from_millis(SHUTDOWN_TIMEOUT_MS),
            shutdown_future
        ).await {
            Ok(result) => result,
            Err(_) => {
                error!("Shutdown timed out");
                Err(Box::new(DriverError::Other("Shutdown timed out".into())))
            }
        }
    }
}

impl Drop for EegSystem {
    fn drop(&mut self) {
        // Since we can't use .await in Drop, we'll just log a warning
        eprintln!("Warning: EegSystem dropped without calling shutdown() first");
        eprintln!("Always call system.shutdown().await before dropping the system");
    }
}
