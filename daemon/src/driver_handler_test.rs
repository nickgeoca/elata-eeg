#[cfg(test)]
mod tests {
    use crate::driver_handler::{CsvRecorder, EegBatchData};
    use crate::config::DaemonConfig;
    use eeg_driver::{AdcConfig, DriverType};
    use std::path::Path;
    use tempfile::tempdir;
    use std::fs;
    use std::io::Read;
    use std::sync::Arc;

    #[test]
    fn test_csv_recorder_new() {
        let config = Arc::new(DaemonConfig {
            max_recording_length_minutes: 30,
            recordings_directory: "test_recordings".to_string(),
        });
        
        let adc_config = AdcConfig {
            sample_rate: 250,
            channels: vec![0, 1, 2, 3],
            gain: 4.0,
            board_driver: eeg_driver::DriverType::Mock,
            batch_size: 32,
        };
        
        let recorder = CsvRecorder::new(250, config, adc_config);
        
        assert_eq!(recorder.is_recording, false);
        assert_eq!(recorder.file_path, None);
        // sample_rate is private, so we can't test it directly
    }
    
    #[test]
    fn test_csv_recorder_start_stop_recording() {
        // Create a temporary directory for test recordings
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let temp_path = temp_dir.path().to_str().unwrap().to_string();
        
        let config = Arc::new(DaemonConfig {
            max_recording_length_minutes: 30,
            recordings_directory: temp_path.clone(),
        });
        
        let adc_config = AdcConfig {
            sample_rate: 250,
            channels: vec![0, 1, 2, 3],
            gain: 4.0,
            board_driver: eeg_driver::DriverType::Mock,
            batch_size: 32,
            Vref: 4.5,
        };
        
        let mut recorder = CsvRecorder::new(250, config, adc_config);
        
        // Start recording
        let start_result = recorder.start_recording().expect("Failed to start recording");
        assert!(start_result.contains("Started recording to"));
        assert!(recorder.is_recording);
        assert!(recorder.file_path.is_some());
        
        let file_path = recorder.file_path.clone().unwrap();
        assert!(file_path.contains(&temp_path));
        
        // Verify the file exists
        assert!(Path::new(&file_path).exists());
        
        // Stop recording
        let stop_result = recorder.stop_recording().expect("Failed to stop recording");
        assert!(stop_result.contains("Stopped recording to"));
        assert!(!recorder.is_recording);
        assert_eq!(recorder.file_path, None);
        
        // Verify the file still exists after stopping
        assert!(Path::new(&file_path).exists());
        
        // Verify the file has the correct header
        let mut file = fs::File::open(&file_path).expect("Failed to open CSV file");
        let mut contents = String::new();
        file.read_to_string(&mut contents).expect("Failed to read CSV file");
        
        assert!(contents.contains("timestamp,channel_1,channel_2,channel_3,channel_4"));
    }
    
    #[test]
    fn test_csv_recorder_write_data() {
        // Create a temporary directory for test recordings
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let temp_path = temp_dir.path().to_str().unwrap().to_string();
        
        let config = Arc::new(DaemonConfig {
            max_recording_length_minutes: 30,
            recordings_directory: temp_path.clone(),
        });
        
        let adc_config = AdcConfig {
            sample_rate: 250,
            channels: vec![0, 1, 2, 3],
            gain: 4.0,
            board_driver: eeg_driver::DriverType::Mock,
            batch_size: 32,
            Vref: 4.5,
        };
        
        let mut recorder = CsvRecorder::new(250, config, adc_config);
        
        // Start recording
        recorder.start_recording().expect("Failed to start recording");
        
        // Create test data
        let eeg_batch_data = EegBatchData {
            channels: vec![
                vec![1.0, 2.0, 3.0],
                vec![4.0, 5.0, 6.0],
                vec![7.0, 8.0, 9.0],
                vec![10.0, 11.0, 12.0],
            ],
            timestamp: 1000000, // 1 second in microseconds
        };
        
        // Write data
        let write_result = recorder.write_data(&eeg_batch_data).expect("Failed to write data");
        assert_eq!(write_result, "Data written successfully");
        
        // Stop recording
        let file_path = recorder.file_path.clone().unwrap();
        recorder.stop_recording().expect("Failed to stop recording");
        
        // Verify the file contains the written data
        let mut file = fs::File::open(&file_path).expect("Failed to open CSV file");
        let mut contents = String::new();
        file.read_to_string(&mut contents).expect("Failed to read CSV file");
        
        // Check header and data rows
        let lines: Vec<&str> = contents.lines().collect();
        assert!(lines.len() >= 4); // Header + 3 data rows
        
        // Check header
        assert_eq!(lines[0], "timestamp,channel_1,channel_2,channel_3,channel_4");
        
        // Check first data row (timestamp + 4 channel values)
        let first_row: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(first_row.len(), 5);
        
        // Timestamp should be the base timestamp
        assert_eq!(first_row[0], "1000000");
        
        // Channel values should match our test data
        assert_eq!(first_row[1], "1");
        assert_eq!(first_row[2], "4");
        assert_eq!(first_row[3], "7");
        assert_eq!(first_row[4], "10");
    }
}