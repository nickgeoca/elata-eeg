> This is not FDA approved. For research only.

## Setup Guide
After the product is assembled with the touch screen, clone then start the kiosk application.
```bash
git clone https://github.com/Elata-Biosciences/elata-eeg
cd elata-eeg
chmod +x install.sh
bash install.sh
```

## Dev Usage
#### Change Code
```bash
# Stop kiosk mode
bash stop.sh

# Term 1, sensors
cd crates/sensors; cargo build
# Term 2, device daemon
cd crates/device; cargo build; cargo run
# Term 3, kiosk
cd kiosk; npm run dev
```

#### Rebuild Production
```bash
# Stop
bash stop.sh

# ...<Change code here> ...

# Rebuild code base and run kiosk mode
bash rebuild.sh
```


# Open EEG Project

**Table of Contents**
1. [Overview](#overview)
2. [Hardware Components](#hardware-components)
3. [Software Stack](#software-stack)
4. [System Architecture](#system-architecture)
5. [Setup & Installation](#setup--installation)
6. [Building & Running](#building--running)
7. [Usage Instructions](#usage-instructions)
8. [Development Notes](#development-notes)
9. [Troubleshooting](#troubleshooting)
10. [Contributing](#contributing)
11. [License](#license)

---


## Overview
**Open EEG Project** aims to build a **low-cost, open-source EEG machine** for precision psychiatry and neuroscience research. We use a Raspberry Pi (Model 5 recommended) plus a Texas Instruments ADS1299-based amplifier board to acquire 8 channels (or more) of EEG signals at high resolution.  
- **Goal**: Democratize EEG hardware, encourage collaboration, and maintain a fully open, right-to-repair design.  
- **Status**: In early prototyping. Currently able to read/write registers over SPI; next step is real-time data capture and signal processing.

---

## Hardware Components

1. **Raspberry Pi 5**  
   - CPU: Quad-core Arm Cortex  
   - SPI pins on the 40-pin header  
   - 5-inch or 7-inch HDMI touchscreen (optional but recommended)

2. **ADS1299 EEGFE Board**  
   - 8-channel, 24-bit analog front-end for EEG  
   - Connects to Pi via SPI  
   - TI’s official evaluation module or custom board

3. **EEG Electrodes**  
   - Wet electrodes (Ag/AgCl or Gold Cup)  
   - Conductive paste/gel (e.g., Ten20)

4. **Cables & Adapters**  
   - Dupont jumper wires (2.54 mm pitch) for SPI and power connections  
   - Possible use of additional shielding or ferrite beads

---

## Software Stack

- **Operating System**: Raspberry Pi OS (Debian-based)
- **Rust Toolchain**: Installed via [rustup](https://rustup.rs)
- **Frontend**: Next.js Kiosk application with WebSocket communication
- **Backend**: Rust workspace with sensor and device crates
- **Hardware Interface**: `rppal` crate for Raspberry Pi GPIO/SPI/I2C/Serial
- **Plugin System**: Modular DSP and UI plugins
- **Version Control**: Git + GitHub
- **License**: GPLv3 (Strong Copyleft)

---

## System Architecture (v2.0)

The system is built around a flexible, asynchronous pipeline architecture defined in the `crates/pipeline` crate. This design allows for modular, reusable processing stages that can be chained together to form complex data processing workflows.

For a detailed breakdown of the architecture, see the [Pipeline Implementation Plan (v2)](todo/implementation_plan_v2.md).

```
 [Sensor Driver]
       |
       | Packet<i32> + Arc<SensorMeta>
       v
+----------------+
| ToVoltage Stage|
+----------------+
       |
       | Packet<f32> + Arc<SensorMeta>
       v
+----------------+
|  Filter Stage  |
+----------------+
       |
       | Packet<f32> + Arc<SensorMeta>
       v
+----------------+
|   Sink Stage   |
| (e.g., CSV, WS)|
+----------------+
```

**Key Concepts:**

-   **`Packet<T>`**: The fundamental unit of data transfer, containing a batch of samples of type `T`.
-   **`SensorMeta`**: A self-describing metadata struct that travels with each packet, providing information about the data source (e.g., sample rate, voltage reference). It is wrapped in an `Arc` for efficient sharing.
-   **`Stage<In, Out>`**: An async trait representing a single processing step in the pipeline. Stages are chained together manually or by a pipeline builder.
-   **Decoupling**: The architecture decouples hardware-specific code (in sensor drivers) from generic data processing logic (in stages), promoting modularity and testability.

---

## Setup & Installation

1. **Assemble Hardware**
   - Mount the ADS1299 board so it’s accessible to the Pi’s GPIO pins.
   - Connect SPI pins:  
     - `MOSI` (GPIO10) → ADS1299 `SDI`  
     - `MISO` (GPIO9)  → ADS1299 `SDO`  
     - `SCLK` (GPIO11) → ADS1299 `SCLK`  
     - `CS`   (GPIO8)  → ADS1299 `CS`  
     - `GND`  → ADS1299 `GND`  
     - `5V`   → ADS1299 `VDD`, etc. (Check board specs)
   - Attach EEG electrodes to the ADS1299 board inputs.

2. **Install Raspberry Pi OS**
   - Use [Raspberry Pi Imager](https://www.raspberrypi.com/software/) on your PC/Mac.
   - Enable SSH in `raspi-config` or via Pi OS Configuration.

3. **Install Rust & Dependencies**
   - `sudo apt update && sudo apt install build-essential curl`
   - `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
   - `source $HOME/.cargo/env`

4. **Clone This Repo**
   ```bash
   git clone git@github.com:<your-username>/open_eeg.git
   cd open_eeg

## Building & Running

1. **Build the workspace**
   ```bash
   # Build all crates
   cargo build
   
   # Or build individual crates
   cd crates/sensors && cargo build
   cd crates/device && cargo build
   ```

2. **Run the device daemon**
   ```bash
   cd crates/device
   cargo run
   ```

3. **Run the kiosk (separate terminal)**
   ```bash
   cd kiosk
   npm install
   npm run dev
   ```

4. **Expected Behavior**
   - Device daemon starts and initializes the sensor hardware
   - Plugin manager loads (currently basic implementation)
   - Kiosk provides web interface at http://localhost:3000
   - WebSocket communication between daemon and kiosk

## Usage Instructions

1. Electrode Placement
   - Place electrodes on the scalp according to your desired montage (e.g., 10-20 system).
   - Use conductive paste. Ensure good contact and consistent ground/reference electrodes.

2. Real-Time Data Acquisition
   - Currently, the code is in “demo” mode, reading raw samples and printing them to stdout or logs.
   - Future versions will include real-time filtering, a ring buffer, and potential streaming to a local GUI or remote client.

3. Data Logging / Output
   - By default, logs are printed. For CSV logging, we’ll add functionality soon (planned in src/logging.rs).

## Development Notes

- SPI Speed: Currently set to 1_000_000 (1 MHz). You can tweak in src/main.rs if your hardware can handle higher rates.
- ADS1299 Register Map: See TI’s official datasheet. We plan to implement a dedicated driver module in src/ads1299.rs.
- Filtering: A basic IIR or FIR filter is planned. We may integrate a DSP crate for advanced filtering (band-pass, notch, etc.).

## Troubleshooting

1. No SPI Response
   - Check wiring for MOSI/MISO swapped.
   - Ensure dtparam=spi=on in /boot/config.txt (and reboot).

2. Compile Errors
   - Update Rust: rustup update.
   - Confirm Cargo.toml dependencies match in your code.

3. Noise / Poor Signal
   - Add shielding to electrode cables.
   - Keep Pi power supply stable, use quality 5 V, 3 A (or more) adapter.
   - Use short wires for SPI to reduce EMI.


## Contributing

We welcome pull requests, feature suggestions, and bug reports!

- Fork the repository.
- Create a new branch for your feature or fix.
- Open a PR against the main branch.


## License

This project is licensed under the GPLv3. By contributing, you agree that your contributions will be licensed under GPLv3 as well.

For more details, see the [LICENSE](LICENSE) file.
