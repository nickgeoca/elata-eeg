# EEG Kiosk Signal Graph & Applet Debugging Notes (Paused)

**Date:** 2025-05-29

**Task Context:**
Investigate and fix the "crazier" EEG signal graph display in the Kiosk frontend. This issue appeared after backend changes were made to incorporate FFT data processing and update the WebSocket binary packet format. The "Brain Waves" applet (first_applet.md) was also reported as not working.

**Primary Issue Identified:**
The root cause of the corrupted signal graph was in [`kiosk/src/components/EegDataHandler.tsx`](./kiosk/src/components/EegDataHandler.tsx:1). When the `fftFlag` in an incoming WebSocket binary message was set to `1` (indicating FFT data is present), but the `onFftData` callback was not provided (e.g., because the main signal graph view was active, not the FFT-specific applet view), the code would skip parsing the FFT data block. This resulted in an incorrect `bufferOffset`, causing the subsequent raw EEG sample parsing logic to read from the wrong part of the buffer (i.e., misinterpreting FFT data as raw samples).

**Fixes Applied to [`kiosk/src/components/EegDataHandler.tsx`](./kiosk/src/components/EegDataHandler.tsx:1):**
1.  **Modified line 137:**
    *   **Original:** `if (fftFlag === 1 && onFftData) {`
    *   **Changed to:** `if (fftFlag === 1) {`
    *   **Reason:** This ensures that the code block responsible for parsing FFT data (and thus correctly advancing the `bufferOffset`) is always entered if the `fftFlag` is `1`, irrespective of whether the `onFftData` callback is currently active.

2.  **Modified line 162 (original line number):**
    *   **Original:** `onFftData(i, Array.from(powerSpectrum)); // Pass channel index and power spectrum`
    *   **Changed to:**
        ```typescript
        if (onFftData) {
          onFftData(i, Array.from(powerSpectrum)); // Pass channel index and power spectrum
        }
        ```
    *   **Reason:** This conditionally calls `onFftData` only if the callback prop is actually provided. This resolved a TypeScript error (`Cannot invoke an object which is possibly 'undefined'`) that arose after the first change and ensures the FFT data is only passed along if a component is expecting it.

**Expected Outcome of Fixes:**
*   The main EEG signal graph in the Kiosk should now display correctly because the raw sample data will be parsed from the correct offset in the WebSocket message buffer.
*   The "Brain Waves" applet should also benefit, as the underlying data parsing for FFT data is now more robust. If its `onFftData` handler is active, it should receive the correctly parsed power spectrum.

**Secondary Concern (Reviewed & Appears OK based on provided diff):**
*   **Initial Concern:** Potential loss of `fft_error` (an error string originating from FFT processing in the `driver` crate) before it reaches the Kiosk.
*   **Review Findings:** The git diff provided by the user for backend changes indicates that:
    *   In [`driver/src/eeg_system/mod.rs`](./driver/src/eeg_system/mod.rs:1), `fft_error` is correctly placed into the `error` field of `ProcessedData`.
    *   In [`daemon/src/driver_handler.rs`](./daemon/src/driver_handler.rs:1), `ProcessedData.error` is cloned into `EegBatchData.error` (specifically, the line `error: data.error.clone(),`).
    *   In [`daemon/src/server.rs`](./daemon/src/server.rs:1), the `create_eeg_binary_packet` function uses `EegBatchData.error` to set the `error_flag` in the binary packet sent to the Kiosk.
    *   The Kiosk's [`EegDataHandler.tsx`](./kiosk/src/components/EegDataHandler.tsx:1) already has logic to check this `errorFlag` and display any associated error message.
*   **Conclusion:** The error propagation path for `fft_error` seems to be correctly handled by the changes in the provided diff.

**Applet Status (`todo/first_applet.md`):**
The work done directly addresses a data integrity issue that would affect both the main signal graph and the data pipeline for any applet consuming FFT data. The "Brain Waves" applet should now have a more reliable data stream.

**Next Steps (Upon Resuming Task):**
1.  **Verify Frontend Fix:** Run the application.
    *   Confirm that the main EEG signal graph displays correctly and no longer looks "crazier".
    *   Ensure no new console errors appear in the Kiosk related to data parsing.
2.  **Test "Brain Waves" Applet:**
    *   Switch to the "Brain Waves" applet view (or the view that consumes `onFftData`).
    *   Verify that it receives and displays FFT data as expected.
    *   Confirm that the applet works as intended (as much as its current implementation allows).
3.  **Test FFT Error Propagation (If Possible):**
    *   If there's a way to reliably trigger an `fft_error` in the `driver` (e.g., by feeding it invalid data for FFT if that's handled by an error return), test if this error message correctly appears in the Kiosk UI. This would fully confirm the error propagation path.
4.  **Address any further issues** based on testing.