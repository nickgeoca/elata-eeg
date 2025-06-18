# Plan: Dynamic Plugin Loading for EEG Daemon (v2 - Production Hardened)

This document outlines a revised, production-ready plan to transition the EEG daemon to a dynamic plugin architecture. This plan incorporates critical feedback on ABI stability, safety, and configuration management.

## 1. Goals

*   Achieve a "git clone, compile, run" workflow for new plugins.
*   Ensure a **stable and safe** Application Binary Interface (ABI) between the daemon and plugins, resilient to compiler versions and build configurations.
*   Provide a clear, versioned contract for plugin authors.
*   Prevent panics within a plugin from crashing the entire daemon.

## 2. Core Architectural Principles

*   **C-ABI VTable:** We will **not** pass a Rust trait object directly across the FFI boundary. Instead, we will define a C-style struct of function pointers (a "vtable") that maps to the `EegPlugin` trait's methods. The plugin will return an opaque pointer (`*mut c_void`) to its state, and the daemon will interact with it solely through the vtable. This guarantees ABI stability.
*   **`cdylib` Crate Type:** All plugins will be compiled as `cdylib` to produce minimal, C-compatible shared libraries.
*   **Opaque Handles & Context:** The daemon will pass configuration and handles (like the event bus) to the plugin via a `PluginContext` struct. The plugin will return an opaque pointer to its internal state.
*   **Panic Boundaries:** All calls from the daemon into plugin code will be wrapped in `std::panic::catch_unwind` to ensure a plugin panic results in a controlled error, not a daemon crash.
*   **Explicit Lifetimes & Drop Order:** The `PluginManager` will explicitly manage the lifetime of loaded libraries, ensuring plugins are dropped *before* their corresponding library code is unloaded from memory.
*   **Strict SemVer:** The `eeg_types` crate, which defines the data structures passed between the daemon and plugins (e.g., `SensorEvent`), must be strictly version-locked.

## 3. Step-by-Step Implementation Plan

### Step 1: Update `eeg_types` with the Stable ABI Definition

1.  **Define the VTable and Plugin Representation:**

    **File:** `crates/eeg_types/src/plugin.rs`
    ```rust
    // Add to top of file
    use std::os::raw::c_void;

    // The C-compatible vtable that mirrors the EegPlugin trait's methods.
    // All functions take the opaque state pointer as their first argument.
    #[repr(C)]
    pub struct EegPluginVTable {
        pub initialize: unsafe extern "C" fn(state: *mut c_void) -> i32, // 0 for success, -1 for error
        pub run: unsafe extern "C" fn(state: *mut c_void, bus: Arc<dyn EventBus>, shutdown_token: CancellationToken) -> i32,
        pub cleanup: unsafe extern "C" fn(state: *mut c_void),
        pub name: unsafe extern "C" fn(state: *const c_void) -> *const std::os::raw::c_char,
    }

    // The struct that the plugin will return. It contains the opaque state
    // and the vtable of functions to operate on that state.
    #[repr(C)]
    pub struct EegPluginFFI {
        pub state: *mut c_void,
        pub vtable: &'static EegPluginVTable,
        // Function to deallocate the state.
        pub drop: unsafe extern "C" fn(state: *mut c_void),
    }
    
    // The signature for the plugin's main entry point.
    // It takes a context from the daemon and returns the FFI-safe plugin representation.
    pub type PluginCreateFunc = unsafe extern "C" fn(ctx: *const PluginContext) -> *mut EegPluginFFI;

    // Context passed from the daemon to the plugin during creation.
    #[repr(C)]
    pub struct PluginContext {
        // For now, we can leave this empty and expand later.
        // A good addition would be a logger handle.
    }
    ```

### Step 2: Create the `PluginManager`

1.  **Add `libloading` dependency** to `crates/device/Cargo.toml`.
2.  **Create `crates/device/src/plugin_manager.rs`:**

    ```rust
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use libloading::{Library, Symbol};
    use eeg_types::{EegPluginFFI, PluginCreateFunc, PluginContext};
    use tracing::{info, warn, error};

    // A wrapper struct that holds the FFI representation and the loaded library.
    // The drop order is crucial: `plugin` must be dropped before `lib`.
    struct LoadedPlugin {
        plugin: EegPluginFFI,
        _lib: Library, // The library must be kept alive.
    }

    impl Drop for LoadedPlugin {
        fn drop(&mut self) {
            // Call the plugin's drop function to deallocate its state.
            unsafe { (self.plugin.drop)(self.plugin.state) };
            info!("Dropped plugin and unloaded library.");
        }
    }

    pub struct PluginManager {
        plugins: Vec<Arc<LoadedPlugin>>,
    }

    impl PluginManager {
        pub fn new() -> Self {
            Self { plugins: Vec::new() }
        }

        pub fn load_plugins_from(&mut self, dir: impl AsRef<Path>) {
            // ... (logic to scan directory for .so/.dll files) ...
            // For each file:
            // self.load_plugin(path);
        }

        unsafe fn load_plugin(&mut self, path: &Path) {
            // The versioned symbol name, e.g., "_plugin_create_v1"
            const ENTRY_POINT: &[u8] = b"_plugin_create_v1\0";

            let lib = match Library::new(path) {
                Ok(l) => l,
                Err(e) => { /* log error */ return; }
            };

            let create_func = match lib.get::<PluginCreateFunc>(ENTRY_POINT) {
                Ok(f) => f,
                Err(e) => { /* log error */ return; }
            };
            
            let ctx = PluginContext {};
            let plugin_ptr = create_func(&ctx as *const _);
            if plugin_ptr.is_null() {
                error!("Plugin creation failed for {:?}", path);
                return;
            }

            let plugin_ffi = *Box::from_raw(plugin_ptr);

            self.plugins.push(Arc::new(LoadedPlugin {
                plugin: plugin_ffi,
                _lib: lib,
            }));
            info!("Successfully loaded plugin from {:?}", path);
        }
        
        pub fn get_plugins(&self) -> Vec<Arc<LoadedPlugin>> {
            self.plugins.clone()
        }
    }
    ```

### Step 3: Convert a Plugin to the New FFI Contract

1.  **Modify `Cargo.toml`:** Use `cdylib`.

    **File:** `plugins/csv_recorder/Cargo.toml`
    ```toml
    [lib]
    crate-type = ["cdylib"]
    ```

2.  **Implement the FFI layer in `lib.rs`:**

    **File:** `plugins/csv_recorder/src/lib.rs`
    ```rust
    // This is the actual plugin implementation struct
    struct CsvRecorderPluginImpl { /* ... fields ... */ }

    // --- FFI glue code ---
    
    // These are the C-ABI compatible functions that will be put in the vtable.
    unsafe extern "C" fn plugin_name(state: *const c_void) -> *const c_char {
        // Cast the opaque state back to the concrete type
        let plugin = &*(state as *const CsvRecorderPluginImpl);
        // For now, returning a static string is easiest.
        "csv_recorder".as_ptr() as *const c_char
    }
    
    // ... (implement similar wrapper functions for initialize, run, cleanup) ...

    // The static vtable instance.
    static PLUGIN_VTABLE: EegPluginVTable = EegPluginVTable {
        name: plugin_name,
        // ...
    };

    // The exported entry point with a versioned name.
    #[no_mangle]
    pub extern "C" fn _plugin_create_v1(ctx: *const PluginContext) -> *mut EegPluginFFI {
        let plugin_impl = CsvRecorderPluginImpl { /* ... */ };
        let state = Box::new(plugin_impl);

        let ffi_plugin = Box::new(EegPluginFFI {
            state: Box::into_raw(state) as *mut c_void,
            vtable: &PLUGIN_VTABLE,
            drop: |state| {
                // This closure will be called to deallocate the plugin's state.
                let _ = Box::from_raw(state as *mut CsvRecorderPluginImpl);
            },
        });

        Box::into_raw(ffi_plugin)
    }
    ```

### Step 4: Update `main.rs` to Use the FFI-Safe Supervisor

1.  **Modify the `supervise_plugin` function:** It will now take the `LoadedPlugin` struct and use the vtable. It must also include `catch_unwind`.

    **File:** `crates/device/src/main.rs`
    ```rust
    use std::panic::catch_unwind;

    async fn supervise_plugin(
        plugin: Arc<LoadedPlugin>,
        // ...
    ) {
        // ...
        let run_result = catch_unwind(|| {
            // Call the 'run' function from the vtable
            unsafe { (plugin.plugin.vtable.run)(plugin.plugin.state, bus.clone(), shutdown_token.clone()) }
        }).await;

        match run_result {
            Ok(Ok(0)) => { /* success */ }
            Ok(Err(e)) => { /* plugin returned an error */ }
            Err(_) => { /* plugin panicked */ }
        }
        // ...
    }
    ```

This revised plan is significantly more complex but addresses the critical safety and stability concerns of the FFI boundary. It represents a production-grade approach to a dynamic plugin system in Rust.