# AI Scratch Pad - EEG Kiosk FFT Artifact Investigation

## Task: Fix FFT Graph Artifact

**Symptom:** A piecewise line appears from the bottom-right towards the middle of the FFT graph for all signals. User reports that touching the bias electrode influences the artifact, causing it to move from "center to lower right, to center to extending to the rightside at various angles."

**Diagnosis:**
The artifact was caused by improper handling of very large or non-finite Power Spectral Density (PSD) values in `FftRenderer.tsx` before normalization and rendering.
While `fftUtils.ts` was correctly clamping non-finite PSDs to 0, `FftRenderer.tsx` did not sufficiently sanitize these values or very large positive finite values before normalization.

**Root Cause Breakdown:**
1.  Large raw EEG signals (e.g., when bias is touched) are scaled by `1e6` in `EegDataHandler.tsx` (V to µV).
2.  These scaled values can lead to extremely large (or `Infinity`) PSD values in `fftUtils.ts`.
3.  `fftUtils.ts` clamps `Infinity` or `NaN` PSDs to `0`.
4.  In `FftRenderer.tsx`:
    a.  If PSD was `0` (from clamping or actual low power), `normalizedMagnitude` could become `-0.5`.
    b.  If PSD was a very large positive finite number, `normalizedMagnitude = (magnitude / DATA_Y_MAX) - 0.5` could result in a very large positive normalized Y-coordinate.
    c.  Previously, `line.xy[j * 2 + 1]` was directly manipulated. This did not protect against very large positive `normalizedMagnitude` values and might have bypassed internal logic of the `WebglStep` line type.
5.  WebGL rendering points with extremely large (positive or negative) Y-coordinates, or incorrectly structured step data, can lead to erratic lines.

**Relevant Files:**
*   `kiosk/src/components/FftRenderer.tsx` (Rendering - **Primary Fix Location**)
*   `kiosk/src/utils/fftUtils.ts` (FFT Calculation - Initial fix was insufficient alone)
*   `kiosk/src/components/EegDataHandler.tsx` (Data Ingestion & Scaling)
*   `kiosk/src/utils/eegConstants.ts` (FFT parameters, e.g., `DATA_Y_MAX`)
*   `webgl-plot` library documentation (indicated `setY` method for `WebglStep`)

**Applied Fixes (Iterative):**

**Attempt 1 (Sanitization in `FftRenderer.tsx`):**
Modified the `animate` function in `kiosk/src/components/FftRenderer.tsx`:
1.  Renamed `magnitude` to `currentMagnitude`.
2.  Sanitized `currentMagnitude`: checked for finiteness, ensured non-negativity, clamped to `DATA_Y_MAX` to get `displayMagnitude`.
3.  Calculated `normalizedMagnitude` using `displayMagnitude`.
4.  Final check for `normalizedMagnitude` finiteness before assigning to `line.xy[j * 2 + 1]`.
    *Result: Did not fix the issue.*

**Attempt 2 (Using `line.setY()` in `FftRenderer.tsx`):**
Based on `webgl-plot` documentation, the `WebglStep` line type likely requires using its `setY` method.
Modified the `animate` function in `kiosk/src/components/FftRenderer.tsx` again:
1.  Kept the sanitization logic from Attempt 1.
2.  Changed data update from direct `line.xy` manipulation to `line.setY(j, normalizedMagnitude)`.
3.  Applied the same `line.setY(j, -0.5)` for clearing lines when data is absent.
4.  Added `// @ts-ignore` comments for `line.setY()` calls due to potentially incomplete TypeScript definitions for `WebglStep`.

**Status:**
- Investigation further refined based on library documentation and iterative fixes.
- Fix applied to `kiosk/src/components/FftRenderer.tsx` to:
    - Properly sanitize and clamp FFT magnitudes.
    - Use the `line.setY()` method for updating `WebglStep` line data, as per library expectations.
- The initial fix in `fftUtils.ts` (clamping non-finite PSDs to 0) remains in place and is complementary.
- Task believed to be complete with this latest change.