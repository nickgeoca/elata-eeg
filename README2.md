TL;DR: This..

## Simple Description
This..

## Setup Guide
After the product is assembled with the touch screen, clone then start the kiosk application.
```bash
git clone https://github.com/Elata-Biosciences/elata-eeg
cd elata-eeg
chmod +x setup.sh
bash setup.sh
```

## Behavior
- 
- 

## Dev Usage
term 1
`cd driver; cargo build`
term 2
`cd daemon; cargo build; cargo run`
term 3
`npm run dev`

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
- After kiosk is solid, make sure all the files in setup.sh (e.g. /home/elata/.config/labwc/autostart) mathc what we got working in the acutal files. 