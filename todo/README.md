# 🎯 CURRENT PROJECT STATUS (December 2025)

## ✅ v0.6 Refactor - PHASE 1-4 COMPLETED
**The architectural refactor is complete and the project compiles successfully!**

**Next Developer: Start with Phase 5 in [`todo/v0.6_refactor_implementation_plan.md`](./v0.6_refactor_implementation_plan.md)**

Key accomplishments:
- ✅ Moved to workspace structure (`crates/sensors/`, `crates/device/`)
- ✅ Removed obsolete DSP logic from sensor crate
- ✅ Created plugin system foundation
- ✅ Updated README.md with new architecture
- ✅ Project compiles with `cargo check`

**Next Priority**: Implement plugin loading and data flow in `PluginManager`

---

### TODO MVP
- 3-d printed case
- BOM... (e.g. header to lead cables)
- Install Instructions
  - BOM, assembly, Software setup, (possible keyboard for wifi), configure browser to detect local network

# TODO Priorities
- Remove printlns?
- Fix readmes
- MVP push. add /boot/firmware/config.txt line? dtparam=spi=on
- Manual - include the http://raspberrypi.local:3000/ thing
 - Blink test doc

# TODO Docs
- security.md... security hardening
- [Event-Driven Refactor Plan](./event_driven_refactor_plan.md) - Comprehensive plan to refactor EEG daemon to event-driven architecture
- [Event Refactor Progress Tracker](./event_refactor_progress.md) - Session-by-session progress tracking for the event-driven refactor
- [Real-Time Filter Investigation (ADS1299 & DSP)](./realtime_filter_investigation.md) - Analysis of current filter behavior and plan for dynamic UI control.
- [Kiosk Boot Failure Investigation](./boot_failures.md) - Diagnosing "site can't be reached" error after reboot.
- [Next.js Build Error Session Notes](./next_js_build_error_session_notes.md) - Investigation of React dependency resolution issues with applet files outside kiosk project.

Lowest Priority
- rename the board? Animal name (elk, emu, eel, ezo)?
- low power type mode (update graph in circular motion. Increase batch size)
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