use std::error::Error;
use std::sync::{Arc, Mutex, mpsc, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use log::{info, error, debug};

use eeg_sensor::{
    create_driver, AdcConfig, AdcDriver, DriverError, DriverStatus,
};
use eeg_types::{BridgeMsg, SensorError};
use pipeline::data::Packet;

pub struct EegSystem {
    driver: Arc<Mutex<Box<dyn AdcDriver>>>,
    processing_task: Option<JoinHandle<()>>,
    sensor_thread: Option<thread::JoinHandle<Result<(), SensorError>>>,
    stop_flag: Arc<AtomicBool>,
    tx: mpsc::Sender<i32>,
}

impl EegSystem {
    /// Creates an EEG processing system without starting it
    pub fn new(
        config: AdcConfig
    ) -> Result<(Self, mpsc::Receiver<i32>), Box<dyn Error>> {
        info!("Creating new EegSystem with config");
        let driver = create_driver(config)?;
        let (tx, rx) = mpsc::channel();

        let system = Self {
            driver: Arc::new(Mutex::new(driver)),
            processing_task: None,
            sensor_thread: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            tx,
        };

        Ok((system, rx))
    }

    /// Starts the processing system with the given configuration
    pub fn start(&mut self, config: AdcConfig) -> Result<(), Box<dyn Error>> {
        self.initialize_processing(config)
    }

    /// Internal helper to initialize or reinitialize the driver and processing task
    fn initialize_processing(&mut self, _config: AdcConfig) -> Result<(), Box<dyn Error>> {
        info!("Initializing EEG processing with config");

        // Stop any existing processing task gracefully
        if self.processing_task.is_some() {
            debug!("Stopping existing processing task");
            self.stop()?;
        }
        
        self.stop_flag.store(false, Ordering::Relaxed);

        let (bridge_tx, bridge_rx) = mpsc::channel::<BridgeMsg>();

        let driver = self.driver.clone();
        let stop_flag = self.stop_flag.clone();
        self.sensor_thread = Some(thread::spawn(move || {
            let mut driver_guard = driver.lock().unwrap();
            driver_guard.acquire(bridge_tx, &stop_flag)
        }));

        let tx = self.tx.clone();
        let stop_flag = self.stop_flag.clone();

        self.processing_task = Some(thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                match bridge_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(msg) => {
                        match msg {
                            BridgeMsg::Data(packet) => {
                                for sample in packet.samples {
                                    if tx.send(sample).is_err() {
                                        eprintln!("Error sending data: receiver dropped");
                                        break;
                                    }
                                }
                            }
                            BridgeMsg::Error(e) => {
                                eprintln!("Sensor error: {}", e);
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        error!("Bridge channel disconnected.");
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // Continue loop on timeout
                    }
                }
            }
        }));

        Ok(())
    }


    /// Stop the data acquisition & gracefully cancel the background task
    pub fn stop(&mut self) -> Result<(), Box<dyn Error>> {
        debug!("Stopping EEG system");
        self.stop_flag.store(true, Ordering::Relaxed);

        if let Some(handle) = self.sensor_thread.take() {
            handle.join().unwrap()?;
        }

        if let Some(task) = self.processing_task.take() {
            task.join().map_err(|e| format!("Processing task panicked: {:?}", e))?;
        }
        
        Ok(())
    }

    /// Reconfigure the driver with new settings
    pub fn reconfigure(&mut self, config: AdcConfig) -> Result<(), Box<dyn Error>> {
        info!("Reconfiguring EEG system with new settings");
        self.stop()?;
        
        // Create new driver with updated configuration
        let new_driver = create_driver(config.clone())?;
        *self.driver.lock().unwrap() = new_driver;
        
        self.initialize_processing(config)
    }

    /// Retrieve the current driver status
    pub fn driver_status(&self) -> DriverStatus {
        self.driver.lock().unwrap().get_status()
    }

    /// Retrieve the driver's configuration
    pub fn driver_config(&self) -> Result<AdcConfig, DriverError> {
        self.driver.lock().unwrap().get_config()
    }

    /// Completely shut down the EEG system and clean up resources
    pub fn shutdown(&mut self) -> Result<(), Box<dyn Error>> {
        self.stop()?;
        self.driver.lock().unwrap().shutdown()?;
        Ok(())
    }
}

impl Drop for EegSystem {
    fn drop(&mut self) {
        if !self.stop_flag.load(Ordering::Relaxed) {
            eprintln!("Warning: EegSystem dropped without calling shutdown() first");
            eprintln!("Always call system.shutdown() before dropping the system");
        }
    }
}
