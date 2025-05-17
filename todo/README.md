### TODO MVP
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

### TODO
Priorites
- Remove printlns?
- Fix readmes
- MVP push. add /boot/firmware/config.txt line? dtparam=spi=on
- Manual - include the http://raspberrypi.local:3000/ thing
 - Blink test doc

Lowest Priority
- rename the board? Animal name (elk, emu, eel, ezo)?
- low power type mode (update graph in circular motion. Increase batch size)
- security.md (Security Hardening over wifi)
- Change board name elata-e1 and add firmware versioning in the filename
- Incoroporate positive channel lead-off detection? Maybe useful in the csv file for data analysis. (see LOFF_STATP register)
- Clean up driver object. may be better if we can use the board as the obj and pass the filtering into it or something
- Dry vs wet electrodes- make py notebook?
- Signal quality and filtering. AVSS vs GND vs DGND vs AGND. 5v powering AVDD
 - AVSS=AGND (JP25). DGND=AGND (Figure 58)... SPI uses AGND... AGND/AVSS/DGND - Analog ground
 - Filtering adds delays and possible phase distortions. Could impact BCI/Nuerofeedback
- UI can change session name too (need to have keyboard popup then). Batch size configurable in browser?
- What's a good way to do study/session naming in the file? How do we key subject/their-data to the file recorded?
- multiple eegs, makes it hard to do over WiFi
- increment session in fname during the day. so expirimeent number can be keyed to notes. or add keyboard.