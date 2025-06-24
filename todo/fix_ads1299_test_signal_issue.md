# Plan to Fix ADS1299 Test Signal Issue

## Problem

The Kiosk UI is displaying a constant voltage value of approximately `2.25V` for all EEG channels. This is because the ADS1299 driver is configured to read an internal DC test signal instead of the actual EEG electrode inputs.

## Root Cause Analysis

1.  **Symptom:** The frontend receives a constant floating-point value of `~2.25`.
2.  **Investigation:** The data conversion function `ch_raw_to_voltage` in `crates/sensors/src/ads1299/helpers.rs` was analyzed. The output value corresponds to the maximum positive raw ADC value (`0x7FFFFF`), indicating positive rail saturation.
3.  **Confirmation:** The register configuration in `crates/sensors/src/ads1299/registers.rs` revealed the issue. The `config2_reg` is initialized with the `DC_TEST` flag, which sets the channel input multiplexer to read an internal test signal.

    *   `pub const config2_reg: u8 = 0xD0 | DC_TEST;`
    *   `pub const DC_TEST: u8 = 3 << 0;`

## Solution

The fix is to remove the `DC_TEST` flag from the `config2_reg` initialization in `crates/sensors/src/ads1299/registers.rs`. This will configure the channels to use the "Normal electrode input" setting.

### Implementation Steps

1.  Modify the `config2_reg` definition in `crates/sensors/src/ads1299/registers.rs`.
2.  Rebuild and redeploy the backend daemon.
3.  Verify that the Kiosk UI displays dynamic EEG signals.

```mermaid
graph TD
    subgraph "Problem State"
        A[registers.rs] --> B{config2_reg = 0xD0 | DC_TEST};
        B --> C[driver.rs initializes chip];
        C --> D[ADC channels read internal test signal];
        D --> E[Kiosk UI shows static ~2.25V];
    end

    subgraph "Solution State"
        F[registers.rs] --> G{config2_reg = 0xD0};
        G --> H[driver.rs initializes chip];
        H --> I[ADC channels read normal electrode input];
        I --> J[Kiosk UI shows dynamic EEG data];
    end

    A --> F;