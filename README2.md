TL;DR: This..

## Simple Description
This..

## Setup Guide
After the product is assembled with the touch screen, clone then start the kiosk application.
```bash
git clone https://github.com/Elata-Biosciences/elata-eeg
cd elata-eeg
chmod +x install.sh
bash install.sh
```

## Behavior
- 
- 

## Dev Usage
#### Change Code
```bash
# Stop kiosk mode
bash stop.sh

# Term 1, driver
cd driver; cargo build
# Term 2, daemon
cd daemon; cargo build; cargo run
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

## TODO
- Interent security check review
- Production ready task items
  1) Mock Driver Issue: Update the daemon's main.rs to use a real hardware driver instead of the mock driver. Currently, it's using DriverType::Mock on line 29, but the real hardware driver (DriverType::Ads1299) is not fully implemented yet.
  2) Error Handling: Add more robust error handling for critical operations in the script, beyond just using set -e.
  3) Health Checks: Add mechanisms to verify that services are running correctly after they're started.
  4) Network Configuration: Add support for configuring network settings, especially if a static IP is needed.
  5) Security Hardening: Consider additional security measures for a production system.
  6) add screen rotate... wlr-randr --output HDMI-A-2 --transform 270
- X11 (LDXE-pi) to Wayland (labwc) was rocky
- After kiosk is solid, make sure all the files in install.sh (e.g. /home/elata/.config/labwc/autostart) mathc what we got working in the acutal files. 
- Realtime hardware update? pros and cons