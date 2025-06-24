# Plan to Fix ADS1299 Daisy-Chain Configuration Issue

## Problem

The Kiosk UI is displaying a constant voltage value of approximately `2.25V` for all EEG channels. The root cause is a misconfiguration in the ADS1299 driver that enables daisy-chain mode for a single-device setup.

## Root Cause Analysis

1.  **Symptom:** The frontend receives a constant floating-point value of `~2.25`, which corresponds to a saturated positive ADC reading.
2.  **Initial Investigation:** Initial theories about test signals or incorrect multiplexer settings were ruled out based on user feedback and further analysis.
3.  **Correct Diagnosis:** The `CONFIG1` register in `crates/sensors/src/ads1299/registers.rs` is initialized with the value `0x90`. This sets the `DAISY_EN` bit (bit 7) to `1`, enabling daisy-chain mode.
4.  **Conflict:** The driver's data acquisition function, `read_data_from_spi`, is implemented to parse data from a single device. When the hardware is in daisy-chain mode, it produces data in a different format than the driver expects. This format mismatch leads to data being parsed incorrectly, resulting in the observed static values.

## Solution

The fix is to disable daisy-chain mode by changing the default value for `config1_reg` in `crates/sensors/src/ads1299/registers.rs`. The `DAISY_EN` bit should be set to `0`.

### Implementation Steps

1.  Modify the `config1_reg` definition in `crates/sensors/src/ads1299/registers.rs` from `0x90` to `0x10`. This change only affects the `DAISY_EN` bit.
2.  Rebuild and redeploy the backend daemon.
3.  Verify that the Kiosk UI displays dynamic EEG signals.

```mermaid
graph TD
    subgraph "Problem State"
        A[registers.rs] --> B{config1_reg = 0x90 (DAISY_EN=1)};
        B --> C[driver.rs initializes chip];
        C --> D[ADC outputs in daisy-chain format];
        D --> E[Driver reads in single-device format];
        E --> F[Data misinterpretation leads to static values];
    end

    subgraph "Solution State"
        G[registers.rs] --> H{config1_reg = 0x10 (DAISY_EN=0)};
        H --> I[driver.rs initializes chip];
        I --> J[ADC outputs in single-device format];
        J --> K[Driver reads correctly];
        K --> L[Kiosk UI shows dynamic EEG data];
    end

    A --> G;