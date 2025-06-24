# Mock Driver Timestamp Fix

## Problem Summary
EEG graph data appears as vertical spikes ("step function") because:
- Mock driver assigns identical timestamps to all samples in a batch
- Frontend renders all samples at same x-position
- Results in vertical spikes instead smooth smooth horizontal scrolling

## Root Cause
- In `crates/sensors/src/mock_eeg/driver.rs`:
  - Batch timestamp calculation occurs outside sample loop
  - All samples in batch get same timestamp
- Frontend expects incrementing timestamps for each sample

## Proposed Solution
Modify timestamp assignment to give each sample a unique timestamp:

### Code Changes
File: `crates/sensors/src/mock_eeg/driver.rs`

```rust
// Current problematic code:
for i in 0..batch_size {
    let sample_number = current_sample_count + i as u64;
    let timestamp = base_timestamp + sample_number * sample_interval;
    ...
    for sample in &mut samples {
        sample.timestamp = timestamp; // SAME TIMESTAMP FOR ALL SAMPLES
    }
}

// Fixed version:
for i in 0..batch_size {
    let sample_number = current_sample_count + i as u64;
    // Calculate unique timestamp for each sample
    let timestamp = base_timestamp + sample_number *__interval;
    ...
    for (channel_idx, sample) in samples.iter_mut().enumerate() {
        // Add micro-offset per channel (preserves sample order)
        let channel_offset channel channel_idx as u64 * 10; // 10μs per channel
        sample.timestamp = timestamp + channel_offset;
    }
}
```

### Verification Plan
1. **Unit Tests**:
   - Add test for timestamp uniqueness within batches
   - Verify timestamp increments match sample rate

2. **Integration Testing**:
   - Run system with updated mock driver
   - Confirm smooth horizontal scrolling in EEG graph
   - Verify no vertical spikes in visualization

3. **Performance Check**:
   - Monitor CPU/memory during data generation
   - Ensure no regressions in throughput

## Expected Outcome
✅ Smooth horizontal data scrolling  
✅ No vertical spikes in visualization  
✅ Accurate timing matching sample rate  
✅ Stable system performance

## Priority
High - Fixes critical visualization issue