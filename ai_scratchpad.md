# Revised Plan for Brain Waves Applet FFT Fix (Rust/WASM Approach)

**Goal:** Fix the FFT artifact in the `brain_waves` applet by performing FFT on larger, properly windowed data segments using Rust DSP compiled to WebAssembly (WASM), ensuring applet independence.

**Phase 1: Reversion and Cleanup**

1.  **Revert Incorrect JavaScript FFT Changes:**
    *   **`kiosk/src/utils/fftUtils.ts`**:
        *   Re-comment the `import FFT from 'fft.js';`.
        *   Re-comment the `calculateFft`, `applyHannWindow`, and `getNextPowerOfTwo` functions.
    *   **`applets/brain_waves/ui/BrainWavesDisplay.tsx`**:
        *   Remove the import of `calculateFft` from `kiosk/src/utils/fftUtils.ts`.
        *   Remove the JavaScript-based FFT processing logic (data accumulation for JS FFT, `channelBuffersRef`, `lastProcessTimeRef`, calls to the JS `calculateFft`).
        *   The `BrainWavesAppletResponse` interface should be temporarily adjusted to expect the *original* PSD structure from the daemon (as it was before my daemon changes), or simply expect raw data but without any client-side JS FFT processing logic. This is a temporary state before WASM integration.
        *   The `fftDataRef` will temporarily not be populated correctly by `BrainWavesDisplay.tsx` until the WASM solution is in place.
    *   **`kiosk/tsconfig.json`**:
        *   Revert the `"include"` path changes. Remove `../applets/**/*.ts` and `../applets/**/*.tsx` from the `include` array.

2.  **User Actions (To be performed by the user after Phase 1):**
    *   Delete the file `kiosk/src/utils/fftUtils.ts`.
    *   Remove the `"fft.js": "^4.0.4"` dependency from `kiosk/package.json`.
    *   Run `npm install` or `yarn install` in the `kiosk` directory to update `package-lock.json` or `yarn.lock`.

**Phase 2: Rust DSP and WASM Implementation**

3.  **Enhance Rust `elata_dsp_brain_waves_fft` Crate (`dsp/brain_waves_fft/src/lib.rs`):**
    *   Modify the `process_eeg_data` function (or create a new one for WASM) to:
        *   Accept a slice of raw voltage samples (`Vec<f32>` or `&[f32]`) representing a single channel's data window.
        *   Accept the sample rate.
        *   Implement a windowing function (e.g., Hann window) to be applied to the input data.
        *   Perform FFT on the windowed data.
        *   Calculate the Power Spectral Density (PSD).
        *   Return the PSD (`Vec<f32>`) and the corresponding frequency bins (`Vec<f32>`).
    *   Consider integrating a well-tested crate like `welch_sde` or `rustfft` directly for robust FFT and PSD calculations if the current implementation is insufficient.
    *   Ensure the output is suitable for the `AppletFftRenderer.tsx`.

4.  **Compile `elata_dsp_brain_waves_fft` to WebAssembly (WASM):**
    *   Add necessary dependencies like `wasm-bindgen`.
    *   Create a WASM-compatible public function (e.g., annotated with `#[wasm_bindgen]`) that wraps the enhanced FFT processing logic. This function will be callable from JavaScript.
    *   Build the crate as a WASM module (e.g., using `wasm-pack`). The output `.wasm` and generated JavaScript binding files will be used by the applet.
    *   The WASM module should ideally be placed in a location accessible to the applet, perhaps within the `applets/brain_waves/ui/` directory or a shared `applets/pkg/` directory.

**Phase 3: Frontend Integration (Applet)**

5.  **Update Applet Frontend (`applets/brain_waves/ui/BrainWavesDisplay.tsx`):**
    *   **WebSocket Data:**
        *   Ensure the `BrainWavesAppletResponse` interface correctly reflects that it receives raw voltage samples (`channels: number[][]`) from the daemon (this matches the daemon change already made).
    *   **WASM Integration:**
        *   Load and instantiate the compiled WASM module.
        *   Create new `channelBuffersRef` (similar to what was attempted for JS FFT) to accumulate incoming raw samples for each channel from the WebSocket.
    *   **FFT Processing:**
        *   When enough data is accumulated in a buffer for a channel (e.g., 2000ms window):
            *   Call the exported WASM function, passing the accumulated data window and sample rate.
            *   Receive the PSD and frequency bins from the WASM function.
        *   Use a sliding window approach (e.g., process every 1000ms hop).
    *   **Data for Renderer:**
        *   Populate `fftDataRef.current` with the PSD data received from the WASM module for each channel.
        *   The `AppletFftRenderer.tsx` will also need the frequency bins. This might require passing them alongside the PSD or having `AppletFftRenderer.tsx` calculate them based on sample rate and FFT size (if the WASM module provides the FFT size).
        *   Trigger `setFftDataVersion` to update `AppletFftRenderer.tsx`.

6.  **Update `applets/brain_waves/ui/AppletFftRenderer.tsx`:**
    *   Ensure it can correctly use the frequency bins provided or calculated based on the output of the WASM FFT processing. If the number of bins or frequency resolution changes due to the new WASM processing, adjustments to x-axis scaling or label generation might be needed.

**Expected Outcome:**
The Brain Waves applet will perform FFT on larger, windowed data segments using Rust code compiled to WASM, leading to a more accurate and stable FFT plot, resolving the visual artifacts. The applet will be more self-contained regarding its core DSP logic.