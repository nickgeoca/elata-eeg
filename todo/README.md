# Todo List & Investigation Notes

This file tracks ongoing tasks, feature plans, and the status of major investigations.

## Recently Completed

*   **✅ Graph Data Disappearing Issue (2025-06-23):** Fixed the issue where graph data was disappearing quickly due to WebSocket reconnections and inefficient data processing in the new EegDataContext architecture.
    *   **Detailed Fix:** [`./fix_graph_data_disappearing_issue.md`](./fix_graph_data_disappearing_issue.md:1)

## Active Investigations

*   **Circular Graph Freezing (Ongoing):** The Kiosk circular graph freezes, indicating a severed data stream. An initial attempt to fix this by refactoring to a pub/sub model was unsuccessful. The investigation is ongoing.
    *   **Detailed Log:** [`./circular_graph_freeze_investigation.md`](./circular_graph_freeze_investigation.md:1)
*   **EEG Data Rate Issue (Ongoing):** The backend is generating data at ~2Hz instead of the expected ~31.25Hz. The root cause has been identified as a computationally expensive `gen_realistic_eeg_data` function in the mock driver, which is unable to generate samples fast enough. The next step is to optimize this function, likely by pre-calculating a buffer of sine wave data.
    *   **Detailed Log:** [`./eeg_data_flow_debugging_log.md`](./eeg_data_flow_debugging_log.md:1)

## Completed Tasks & Old Plans
*   [Plan to Fix Mock Driver Data Rate](./fix_mock_driver_data_rate.md) - *Note: This plan was based on an incorrect diagnosis and is now superseded by the main debugging log.*
*   [Fix Kiosk Config Bug](./fix_kiosk_config_bug_plan.md)
*   [Event-Driven Refactor Plan](./event_driven_refactor_plan.md)
*   [Dynamic Plugin Loading Plan](./dynamic_plugin_loading_plan_v2.md)