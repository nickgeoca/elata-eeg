# TODO and Implementation Notes

This directory contains task tracking and implementation documentation for the EEG system.

## Completed Tasks

- [FFT Plugin Subscriber Awareness Implementation](./fft_plugin_subscriber_awareness_implementation.md) - Updated the brain_waves_fft plugin to skip expensive FFT calculations when no subscribers are present, providing significant CPU savings.
- [CSV Recorder Fix Implementation](./csv_recorder_implementation_plan.md) - ✅ **COMPLETED** - Event-based CSV recording functionality with real-time status feedback and elata-v1 filename format

## Active Tasks

- [CSV Recorder "Pending..." Button Fix](./csv_recorder_pending_fix_plan.md) - Fix message format mismatch causing recording button to stay in "Pending..." state
- [Recording Button Pending Fix](./recording_button_pending_fix_plan.md) - Fix stale closure issue causing recording button to show "Pending..." for 10 seconds despite successful recording start

## Completed Implementation Details

- [CSV Recorder Event Flow Diagram](./csv_recorder_event_flow_diagram.md) - Detailed architecture diagrams for the CSV recorder fix
- [CSV Recorder Fix Detailed Plan](./csv_recorder_fix_detailed_plan.md) - Original analysis and requirements

## Planned Tasks

- [Plugin Subscriber Awareness Detailed Guide](./plugin_subscriber_awareness_detailed_guide.md) - Comprehensive guide for implementing subscriber awareness across all plugins
- [Plugin Subscriber Awareness Implementation Plan](./plugin_subscriber_awareness_implementation_plan.md) - Step-by-step plan for rolling out subscriber awareness to other plugins

## Notes

When adding new tasks or implementation notes:
1. Create a new `.md` file in this directory
2. Add a corresponding entry to this README.md file
3. Use descriptive filenames that clearly indicate the task or feature