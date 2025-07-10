> [!WARNING]
> # DEPRECATED: Outdated Architectural Vision
> This document describes an architectural vision for a dynamic, ABI-driven plugin system that is NOT currently implemented in the codebase. The current implementation uses a pure-Rust, inventory-based static plugin registration system. This document is retained for historical context but should not be used as a reference for the current architecture.

# Architecture Plan: A Unified, Dynamic Plugin System

........ feedback

### 1  Direct answers to your three questions

| ❓                       | Short answer                                                                                                                                           | Practical detail                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Git-clone workflow**  | Yes—clone the plugin repo *inside* `plugins/`, run `cargo build --release`, and the host will discover the resulting `lib<name>.so` and `plugin.toml`. | 1. The plugin repo itself is a *Cargo workspace* containing a `crate` that targets `cdylib`.<br>2. `build.rs` (or your Makefile) should copy the finished `.so` (and any `ui/dist/*`) up one level so the tree looks like:<br>  `plugins/brain_fft/{plugin.toml,libbrain_fft.so,ui/dist/...}`<br>3. The user only changes the pipeline JSON to refer to the new `stage_type` declared by the plugin’s `StageFactory`.                                                                                                                        |
| **Python DSP plugins**  | Possible, two approaches:                                                                                                                              | <br>**A. ScriptStage in host** – embed CPython once, load a `.py` file that exposes `process(samples: np.ndarray) -> None`.  Easiest for data-scientists; incurs per-packet GIL + copy cost unless you feed a `memoryview`.<br>**B. PyO3/Maturin hybrid** – author writes Python, you wrap it with `pyo3::prelude::*` and compile to a Rust `cdylib` that satisfies your ABI.  No interpreter embedding inside the host; good perf but still a Rust build step.                                                                              |
| **Any other thoughts?** | Mostly polish:                                                                                                                                         | \* Provide a `cargo xtask build-plugins` that (a) walks `plugins/*/Cargo.toml`, (b) runs `cargo build --release`, (c) copies artefacts to the right place, (d) warns on ABI mismatch.<br>\* Add `#[no_mangle] extern "C" fn abi_version() -> AbiVersion` in each plugin so the host can *cheaply* query version without fully linking.<br>\* Ship a `cargo generate eeg-plugin` template with:<br>  - ready-made `plugin.toml`<br>  - `StageFactory` skeleton<br>  - optional `ui/` and GitHub Actions that publish a pre-built release zip. |

---

### 2  Design tweaks & clarifications

1. **Plugin directory contract**
   Reserve exactly **three** stable file names so tooling can be trivial:

   * `plugin.toml` (manifest)
   * `bin/<target-triple>/libplugin_name.{so|dll|dylib}`
   * `ui/bundle.js`

   Your `PluginManager` then does: `for entry in fs::read_dir("plugins") { read plugin.toml }`.

2. **ABI evolution policy**

   * Only break on **major** bumps.
   * Host rejects `plugin_abi.major != host_abi.major`.
   * If minor differs, log a warning but keep going.

3. **Hot-reload** (optional)
   Watch `plugins/**` with `notify` crate; on file-change, unload + reload that one plugin so devs don’t restart the pipeline.

4. **Safety**

   * Wrap *every* plugin call (`process()`, not just `register_factories`) in `catch_unwind`; disable plugin on panic, continue pipeline.
   * Consider running non-trusted plugins in a separate process or as Wasm if you’ll distribute binaries from strangers.

5. **Frontend**

   * Agree on a single global React context so plugin UI can subscribe to streams without re-implementing sockets.
   * Force all plugin bundles to export `default` React component; reduces config boilerplate.

6. **Python / Wasm road-map**

   * Start with ScriptStage (CPython).
   * When perf becomes an issue, compile critical kernels to Numpy `numba` or Rust `pyo3` extensions.
   * Longer term, Wasmtime with `memory64` lets you share the same `&mut [f32]` buffer zero-copy.

---

### 3  Contributor experience checklist

1. `git clone https://github.com/example/brain_fft plugins/brain_fft`
2. `cargo build --manifest-path plugins/brain_fft/Cargo.toml --release`
3. **No host recompile needed**—run `eeg-host run pipeline.json`
4. Open the web UI → new “Brain Waves FFT” panel appears automatically.

If you can deliver that flow, you’ve hit the “clone-build-run” ideal 👍.

---

**Anything else I need?**
Just keep a minimal, frozen `pipeline-abi` and a one-page “Write your first plugin” guide. Everything else is implementation detail.

.............

> [!WARNING]
> Still working on this. Do not implement yet ⚠️

This document outlines a robust, stable, and extensible architecture for a dynamic plugin system for the EEG pipeline. It incorporates feedback from a "red-team" analysis to address potential issues around versioning, safety, and frontend integration.

## 1. High-Level Vision

The goal is to create a system where new processing capabilities (pipeline stages) and their corresponding user interfaces can be added to the application by dropping a compiled plugin into a directory. This should not require recompiling the main host application.

This architecture is broken down into two main areas:
1.  **Backend:** A dynamic loading system for Rust-based pipeline stage plugins.
2.  **Frontend/Discovery:** A mechanism for discovering plugins and their associated UI components.

```mermaid
graph TD
    subgraph Host Application
        A[PipelineRuntime]
        B[StageRegistry]
        C[PluginManager]
        D[Plugin Discovery API]
    end

    subgraph Filesystem
        E["plugins/"]
        F["plugin_A/plugin.A.so"]
        G["plugin_A/plugin.toml"]
        H["plugin_B/plugin.B.so"]
        I["plugin_B/plugin.toml"]
    end

    subgraph Kiosk UI (Next.js)
        J[Main UI]
        K[Dynamic Plugin UI Components]
    end
    
    subgraph Shared ABI
        ABI[("pipeline-abi crate<br/>(stable, versioned)")]
    end

    C -- Scans --> E
    C -- Loads --> F
    C -- Loads --> H
    C -- Registers Factories --> B
    A -- Uses --> B

    D -- Scans --> E
    D -- Reads --> G
    D -- Reads --> I
    J -- Fetches Plugin Info --> D
    J -- Dynamically Renders --> K

    F -- Contains --> StageFactory_A
    H -- Contains --> StageFactory_B
    
    A -- Depends on --> ABI
    F -- Depends on --> ABI
    H -- Depends on --> ABI
```

## 2. Core Principle: Stability via a Shared ABI Crate

To prevent crashes and ensure long-term stability, the contract between the host and plugins will be defined in a dedicated, versioned `pipeline-abi` crate.

*   **Purpose:** This crate will define the shared data structures (`struct`s) and function signatures (`trait`s, `extern "C"` functions) that plugins use to communicate with the host. It will be a minimal, slow-moving dependency.
*   **Versioning:** It will export a version constant. The host application will check this version at runtime against the version the plugin was compiled with, rejecting incompatible plugins.

### Example `pipeline-abi/src/lib.rs`:
```rust
// In pipeline-abi/src/lib.rs

use std::any::Any;

/// The ABI version, checked at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbiVersion { 
    pub major: u16, 
    pub minor: u16,
    pub patch: u16,
}

/// The version of the ABI defined in this crate.
pub const ABI_VERSION: AbiVersion = AbiVersion { major: 1, minor: 0, patch: 0 };

/// An opaque, vtable-like struct passed to plugins to allow them
/// to register their components with the host.
#[repr(C)]
pub struct StageRegistrar {
    /// The internal state of the registrar. Should not be touched by plugins.
    _private: [u8; 0],
}

impl StageRegistrar {
    /// The host provides this method for plugins to register a factory.
    /// The implementation will live in the host application.
    pub fn register_factory(&mut self, factory: Box<dyn Any>) {
        // This is a placeholder. The actual implementation will be
        // provided by the host application.
        unimplemented!();
    }
}

/// Every plugin's dynamic library (.so, .dll) must export this function.
/// It is the entry point for the host to initialize the plugin.
///
/// # Returns
/// `true` if the plugin successfully registered, `false` otherwise.
#[no_mangle]
pub unsafe extern "C" fn register_factories(
    registrar: &mut StageRegistrar,
    host_version: AbiVersion,
) -> bool {
    // Plugin implementation here...
    // 1. Check if host_version.major == ABI_VERSION.major
    // 2. If compatible, call registrar.register_factory(...)
    // 3. Return success or failure.
    true
}
```

## 3. Revised Backend Architecture

*   **`PluginManager`:** This new component in the host application will be responsible for:
    1.  Scanning the `plugins/` directory for library files.
    2.  Calling `libloading::Library::new()` to load each plugin.
    3.  Getting a `Symbol` for the `register_factories` function.
    4.  Calling the function with the host's ABI version and a pointer to the `StageRegistry`.

*   **Safety & Isolation:** To prevent a faulty plugin from crashing the entire application, we will use `std::panic::catch_unwind`. Each plugin's setup and execution will be wrapped. If a panic occurs, the plugin will be disabled, an error will be logged, and the host will continue running.

## 4. Revised Frontend Architecture

*   **Plugin UI Bundling:** Each plugin with a UI will be responsible for compiling its UI code (e.g., React/TSX) into a standard, self-contained JavaScript ES module bundle (e.g., `dist/index.js`).
*   **Static Serving:** The main application's web server will serve these static bundles from a dedicated route, like `/static/plugins/{plugin_name}/index.js`.
*   **Manifest-Driven UI (`plugin.toml`):** Each plugin will have a manifest file that describes its UI assets.

### Example `plugins/brain_fft/plugin.toml`:
```toml
name = "Brain Waves FFT"
version = "0.2.0"
description = "A plugin to perform FFT on EEG data and visualize the results."

# Rust library details
[library]
path = "target/release/libbrain_fft.so"

# UI component details
[ui]
# Path to the compiled JS bundle, relative to the plugin root.
bundle_path = "ui/dist/bundle.js" 
# The name of the component to import from the bundle.
component_name = "FftDisplay"
# Props the Kiosk UI needs to pass to the component.
required_props = ["stage_id", "data_stream_url"]
```

*   **Dynamic Loading:** The Kiosk UI will fetch this manifest data from a discovery API endpoint. It will then use dynamic `import()` to load the JS bundle from the static path and render the component.

## 5. Updated Operational Flow

1.  **Author**: Clones a plugin template. Implements the Rust stage logic against the `pipeline-abi` crate. Optionally, creates a React widget and configures its build step. Updates `plugin.toml`.
2.  **Build**: A script (`make build-plugin` or similar) runs `cargo build --release` and `npm run build` (if a UI exists).
3.  **Deploy**: The final `.so` library and `ui/dist/` directory are copied into the main application's `plugins/{plugin_name}/` directory.
4.  **Runtime**:
    *   **Backend**: The `PluginManager` scans the directory, performs version checks, and safely loads the plugins, populating the `StageRegistry`.
    *   **Frontend**: The Kiosk UI hits the discovery API, gets a list of plugins and their static asset paths, and dynamically wires up the UI components.

## 6. Future Considerations & Open-Source Friendliness

*   **Wasm/Python Stages:** To support non-Rust developers, a built-in "ScriptStage" could be created. This stage would load and execute a Wasm module or a Python script, providing a simpler interface for DSP experts.
*   **Plugin Template:** A `cargo generate` template would dramatically lower the barrier to entry for new plugin authors.