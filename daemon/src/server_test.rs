#[cfg(test)]
mod tests {
    use crate::server::create_eeg_binary_packet;
    use crate::driver_handler::EegBatchData;

    #[test]
    fn test_create_eeg_binary_packet() {
        // Create test data with 2 channels, 3 samples each
        let eeg_batch_data = EegBatchData {
            channels: vec![
                vec![1.0, 2.0, 3.0],
                vec![4.0, 5.0, 6.0],
            ],
            timestamp: 1234567890, // Example timestamp
        };

        // Generate binary packet
        let binary_packet = create_eeg_binary_packet(&eeg_batch_data);

        // Expected structure:
        // - 8 bytes for timestamp (little-endian)
        // - 4 bytes per float * 3 samples * 4 channels = 48 bytes
        // Total: 56 bytes

        // Verify packet size
        assert_eq!(binary_packet.len(), 8 + (4 * 3 * 4));

        // Verify timestamp
        let timestamp_bytes = &binary_packet[0..8];
        let timestamp = u64::from_le_bytes([
            timestamp_bytes[0], timestamp_bytes[1], timestamp_bytes[2], timestamp_bytes[3],
            timestamp_bytes[4], timestamp_bytes[5], timestamp_bytes[6], timestamp_bytes[7],
        ]);
        assert_eq!(timestamp, 1234567890);

        // Verify first channel data (first 3 floats after timestamp)
        let ch1_sample1_bytes = &binary_packet[8..12];
        let ch1_sample1 = f32::from_le_bytes([
            ch1_sample1_bytes[0], ch1_sample1_bytes[1], 
            ch1_sample1_bytes[2], ch1_sample1_bytes[3],
        ]);
        assert_eq!(ch1_sample1, 1.0);

        // Verify second channel data (first sample)
        let ch2_sample1_bytes = &binary_packet[8 + (3 * 4)..8 + (3 * 4) + 4];
        let ch2_sample1 = f32::from_le_bytes([
            ch2_sample1_bytes[0], ch2_sample1_bytes[1], 
            ch2_sample1_bytes[2], ch2_sample1_bytes[3],
        ]);
        assert_eq!(ch2_sample1, 4.0);

        // Verify that channels are duplicated when fewer than 4 are provided
        // The 3rd channel should be a duplicate of the 2nd channel
        let ch3_sample1_bytes = &binary_packet[8 + (3 * 4) * 2..8 + (3 * 4) * 2 + 4];
        let ch3_sample1 = f32::from_le_bytes([
            ch3_sample1_bytes[0], ch3_sample1_bytes[1], 
            ch3_sample1_bytes[2], ch3_sample1_bytes[3],
        ]);
        assert_eq!(ch3_sample1, 4.0); // Should match channel 2's first sample
    }

    #[test]
    fn test_create_eeg_binary_packet_with_four_channels() {
        // Create test data with exactly 4 channels
        let eeg_batch_data = EegBatchData {
            channels: vec![
                vec![1.0, 2.0],
                vec![3.0, 4.0],
                vec![5.0, 6.0],
                vec![7.0, 8.0],
            ],
            timestamp: 987654321,
        };

        // Generate binary packet
        let binary_packet = create_eeg_binary_packet(&eeg_batch_data);

        // Verify packet size (8 bytes timestamp + 4 channels * 2 samples * 4 bytes)
        assert_eq!(binary_packet.len(), 8 + (4 * 2 * 4));

        // Verify timestamp
        let timestamp_bytes = &binary_packet[0..8];
        let timestamp = u64::from_le_bytes([
            timestamp_bytes[0], timestamp_bytes[1], timestamp_bytes[2], timestamp_bytes[3],
            timestamp_bytes[4], timestamp_bytes[5], timestamp_bytes[6], timestamp_bytes[7],
        ]);
        assert_eq!(timestamp, 987654321);

        // Verify each channel's first sample
        for channel in 0..4 {
            let offset = 8 + (channel * 2 * 4); // 2 samples per channel
            let sample_bytes = &binary_packet[offset..offset + 4];
            let sample = f32::from_le_bytes([
                sample_bytes[0], sample_bytes[1], sample_bytes[2], sample_bytes[3],
            ]);
            assert_eq!(sample, (channel * 2 + 1) as f32);
        }
    }
}