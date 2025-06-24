# Plan to Fix Mock Driver Data Rate

## Problem

The mock EEG driver in `crates/sensors/src/mock_eeg/driver.rs` is generating data at a much lower rate (2Hz) than the expected 31.25Hz.

## Root Cause

The data generation loop in the mock driver incorrectly calculates the sleep duration between batches. The calculation does not account for the number of channels, causing the sleep time to be significantly longer than intended.

**Incorrect Calculation:**
```rust
let sleep_time = (1000 * batch_size as u64) / config.sample_rate as u64;
```

## Proposed Solution

Modify the sleep time calculation in `crates/sensors/src/mock_eeg/driver.rs` to correctly factor in the number of channels.

**Corrected Calculation:**
```rust
let sleep_time = (1000 * batch_size as u64) / (config.sample_rate as u64 * config.channels.len() as u64);
```

## Implementation Steps

1.  Apply the code change to `crates/sensors/src/mock_eeg/driver.rs`.
2.  Rebuild the project using `cargo build`.
3.  Run the application to verify that the data rate is now correct.