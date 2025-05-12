> This is not FDA approved. For research only.

## Simple Description
This..

## BOM
- Electrodes - https://shop.openbci.com/products/openbci-gold-cup-electrodes?variant=37345591591070
- ADS1299 board
- Pi 5
- Header Wires
- eeg head band - https://www.fri-fl-shop.com/products/19-channel-eeg-headband

## Electro Placement Guide
- Check Electrode Contact: Good contact (gel, minimal motion) helps reduce large offset drifts. Scotch tape can help hold electrodes in place but can also allow some micro motion.
- Give the System Time to Settle: Right after you place electrodes, the offset is often larger. Let the system stabilize for ~30 s or so before recording.

  Active Electrodes (4 sites): Whatever scalp locations you want to record from (e.g., Fp1, Fp2, O1, O2, or T3, T4, etc., depending on your experiment).
    Reference Electrode:
        Linked-Ears Reference: A common practice is to link the left and right earlobes (or mastoids) together via a y‐cable, then feed that combined signal into SRB1 (the reference). This helps balance left/right ear impedances and can reduce some artifacts.
        Or simply pick one ear/mastoid to use as reference for all channels if linking ears is not convenient.
    Bias (Ground) Electrode: A single bias electrode can still be placed on (for example) an earlobe, mastoid, or an Fpz location (low on the forehead, near the hairline).

Rationale:

    All channels can share the same reference electrode (or the same “linked” pair) so signals are measured consistently.
    Using a single bias electrode for all channels simplifies the setup and is standard practice.

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
### TODO MVP
@Kyle we need a run of this, end to end on a fresh Pi 5. Should we be paying people to try it?

- Code the touch screen
- 3-d printed case
- BOM... (e.g. header to lead cables)
- 60hz problem. It's way too strong, so either:
  - Use a battery
  - Possibly fix with a $0.30 capacitor accross 5v and gnd
  - Use a [USB line filter](https://www.amazon.com/iFi-iSilencer-Eliminator-Suppressor-Adapter/dp/B084C24W8L?crid=2U7DZVT2POA2W&sprefix=audio%2Bpower%2Bsupply%2Bnoise%2Bfilter%2Busb%2B%2Caps%2C135&sr=8-4&th=1)
- Install Instructions
  - Purchase BOM
  - Assemble prototype
  - Pi 5 Setup: WiFi, git clone, bash install.sh, etc
  - either need 
    1) Keyboard to set WiFi password, etc
    2) Pull SD card. Set WiFi and SSH
    3) Touch screen? Will that work out of the box?
  - Configure Chrome/Firefox to detect local network
- Managing Expectations
  - Putting on the leads is tricky. There is goo and typically lab staff put it on the subject
  - One time install of the cables (SPI, power, etc) is tricky. Probably takes a weekend to get the EEG setup.
  - Our data analysis is still limited. After you record with the EEG, how do you know your sleep quality?
  - The EEG is prototype grade (Hdmi cable in 3-d printed case, battery hanging out of it, etc)
  - 


### TODO Code
Priorites
- Remove printlns?
- Fix readmes
- MVP push. add /boot/firmware/config.txt line? dtparam=spi=on
- Manual - include the http://raspberrypi.local:3000/ thing

Lowest Priority
- Production ready task items
- Health Checks: Add mechanisms to verify that services are running correctly after they're started.
- Network Configuration: Add support for configuring network settings, especially if a static IP is needed.
- Security Hardening: Consider additional security measures for a production system.
- X11 (LDXE-pi) to Wayland (labwc) was rocky
- Firmware versioning in the filename to account for expirimenetal errors from firmware
- Incoroporate positive channel lead-off detection? Maybe useful in the csv file for data analysis. (see LOFF_STATP register)
- integrate erro handlign
- need to enable local device discovery if you want to download recordings
- Change board name. ads1299-elata-v0.1
- UI, switch to mock mode?
- Add normalization for the GUI and pass voltage range per channel
- Clean up driver object. may be better if we can use the board as the obj and pass the filtering into it or something
- Add to config.json? the fitler parameters?
- - Then maybe add an HTTP route to download.
- Add WS to stop EEG?
- Handle voltage scaling. Should it be passed as normalized values -1 to 1 from the daemon?
- Fix up the eeg graph. x axis. y axis. signal spacing, etc
- Blink test doc
- GUI switch to mock mode. Enable disable channels? no changes during recording
- Add data exploration repo? seperate from this one?
- discuss battery vs plugin. how much does 60hz affect sginal qaulity?
- mock driver thru UI and have it run on computer in mock mode for web dev'ing it
- test low esr capacitor on power supply
- install script
  - Also some Pi's seem to use Wayland by default and others X11. They switched to wayland on Pi 5 last year.
  - Impact on Pi 5 Behavior (Revised): Yes, the script makes significant changes. Crucially, while the default Raspberry Pi OS (Bookworm) on a Pi 5 uses Wayland (with the Wayfire compositor) for its graphical session, this script overrides that default and forces the system to use X11. It does this by:
- Should we add 60hz detection? Like your too close to a wall socket
- Dry vs wet electrodes- make py notebook?
- Improve board handling in GUI.
- Add studies thing via git clone
- Data quality accessment- impedance testing, 60/50hz noise measure. step thru (don't need graphing either)
- Look at signal quality. AVSS vs GND vs DGND vs AGND. 5v powering AVDD
 - AVSS=AGND (JP25). DGND=AGND (Figure 58)
 - SPI uses AGND
 - AGND/AVSS/DGND - Analog ground
 - Stimulus response- Problem: There is a delay between pi 5 recording data and mac emitting stimulus. Solution: have the daemon and webclient (Pi 5 and Mac) sync clocks (NTP/PTP)
- Change sample rate and channel count in the browser. session name too (need to have keyboard popup then). Batch size configurable in browser?
- name the age or something in the file name? how do we key subject/their-data to the file recorded?
 - session number on screen, then on pen and paper "session #35, age 24, name Bob Dylan, etc"
  - then multiple eegs, makes it hard to do
  - maybe study number in config.json and eeg number in config.json... then write that to file
  - file name = "study4_eeg2_recording24_datetime_..."

## What Can EEG Analysis Tell You?
1) Frequency Bands: EEG signals are commonly decomposed into frequency bands (delta, theta, alpha, beta, gamma). Changes in band power can indicate different cognitive or physiological states (e.g., alpha often relates to relaxation or idling states, beta to active concentration).

2) Event-Related Potentials (ERP): By time-locking your data to specific events or stimuli, you can observe averaged “peaks and troughs” that give insight into the brain’s typical response or processing speed.

3) Connectivity & Network Analysis: Measures such as coherence, phase-locking value, and Granger causality allow you to infer how different brain regions might interact or synchronize during tasks.

4) Clinical or Research Insights: In clinical settings, EEG can help investigate sleep disorders, epileptiform activity, or abnormal brain rhythms. In research/BCI contexts, EEG can be used for controlling external devices, studying cognitive workload, etc.

5) Classification/Decoding: Machine learning models can classify mental states (e.g., different motor imagery tasks, alert vs. drowsy states). This is widely used in brain-computer interface (BCI) applications.

## Analsyis Software
1) MNE-Python is by far the most comprehensive and widely adopted for general-purpose EEG processing and analysis in Python.

2) PyEEG is good for feature extraction and complexity measures.

3) MOABB and Braindecode cater more to BCI/deep-learning-driven EEG classification tasks.

4) PyEDFlib is essential if you need robust EDF/BDF file handling.
