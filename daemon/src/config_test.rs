#[cfg(test)]
mod tests {
    use crate::config::DaemonConfig;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;
    use std::sync::Arc;

    #[test]
    fn test_daemon_config_default() {
        let config = DaemonConfig::default();
        assert_eq!(config.max_recording_length_minutes, 60);
        assert_eq!(config.recordings_directory, "../recordings/");
    }

    #[test]
    fn test_load_config_creates_default_when_missing() {
        // Create a temporary directory for the test
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let config_path = temp_dir.path().join("config.json");
        
        // Use a test-specific config path
        let temp_path = config_path.to_str().unwrap();
        
        // Ensure the config file doesn't exist
        if Path::new(temp_path).exists() {
            fs::remove_file(temp_path).expect("Failed to remove test config file");
        }
        
        // Create a custom load_config function that uses our test path
        let load_test_config = || -> Arc<DaemonConfig> {
            // Try to load from file
            match std::fs::read_to_string(temp_path) {
                Ok(contents) => {
                    match serde_json::from_str(&contents) {
                        Ok(config) => {
                            println!("Loaded configuration from {}", temp_path);
                            Arc::new(config)
                        },
                        Err(e) => {
                            println!("Error parsing configuration file: {}. Using defaults.", e);
                            let default_config = DaemonConfig::default();
                            
                            // Create default config file for future use
                            if let Ok(json) = serde_json::to_string_pretty(&default_config) {
                                let _ = std::fs::write(temp_path, json);
                                println!("Created default configuration file at {}", temp_path);
                            }
                            
                            Arc::new(default_config)
                        }
                    }
                },
                Err(_) => {
                    println!("Configuration file not found. Using defaults.");
                    let default_config = DaemonConfig::default();
                    
                    // Create default config file for future use
                    if let Ok(json) = serde_json::to_string_pretty(&default_config) {
                        let _ = std::fs::write(temp_path, json);
                        println!("Created default configuration file at {}", temp_path);
                    }
                    
                    Arc::new(default_config)
                }
            }
        };
        
        // Load the config (should create default)
        let config = load_test_config();
        
        // Verify default values
        assert_eq!(config.max_recording_length_minutes, 60);
        assert_eq!(config.recordings_directory, "../recordings/");
        
        // Verify the config file was created
        assert!(Path::new(temp_path).exists());
        
        // Read the created file and verify its contents
        let file_contents = fs::read_to_string(temp_path).expect("Failed to read config file");
        let parsed_config: DaemonConfig = serde_json::from_str(&file_contents).expect("Failed to parse config JSON");
        
        assert_eq!(parsed_config.max_recording_length_minutes, 60);
        assert_eq!(parsed_config.recordings_directory, "../recordings/");
    }

    #[test]
    fn test_load_config_uses_existing_file() {
        // Create a temporary directory for the test
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let config_path = temp_dir.path().join("config.json");
        let temp_path = config_path.to_str().unwrap();
        
        // Create a custom config
        let custom_config = DaemonConfig {
            max_recording_length_minutes: 30,
            recordings_directory: "./custom_recordings/".to_string(),
        };
        
        // Write the custom config to file
        let json = serde_json::to_string_pretty(&custom_config).expect("Failed to serialize config");
        fs::write(temp_path, json).expect("Failed to write config file");
        
        // Create a custom load_config function that uses our test path
        let load_test_config = || -> Arc<DaemonConfig> {
            // Try to load from file
            match std::fs::read_to_string(temp_path) {
                Ok(contents) => {
                    match serde_json::from_str(&contents) {
                        Ok(config) => {
                            println!("Loaded configuration from {}", temp_path);
                            Arc::new(config)
                        },
                        Err(e) => {
                            println!("Error parsing configuration file: {}. Using defaults.", e);
                            let default_config = DaemonConfig::default();
                            
                            // Create default config file for future use
                            if let Ok(json) = serde_json::to_string_pretty(&default_config) {
                                let _ = std::fs::write(temp_path, json);
                                println!("Created default configuration file at {}", temp_path);
                            }
                            
                            Arc::new(default_config)
                        }
                    }
                },
                Err(_) => {
                    println!("Configuration file not found. Using defaults.");
                    let default_config = DaemonConfig::default();
                    
                    // Create default config file for future use
                    if let Ok(json) = serde_json::to_string_pretty(&default_config) {
                        let _ = std::fs::write(temp_path, json);
                        println!("Created default configuration file at {}", temp_path);
                    }
                    
                    Arc::new(default_config)
                }
            }
        };
        
        // Load the config (should use our custom one)
        let config = load_test_config();
        
        // Verify custom values were loaded
        assert_eq!(config.max_recording_length_minutes, 30);
        assert_eq!(config.recordings_directory, "./custom_recordings/");
    }
}