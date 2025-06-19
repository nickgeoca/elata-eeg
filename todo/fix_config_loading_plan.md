# Plan to Fix Unreliable Configuration Loading (Revised)

This plan addresses the unreliable loading of `config.json` and the associated build errors, following a simplified and more direct approach.

### 1. Fix Build Dependencies
*   **Problem**: The `cargo run` command fails with a dependency-related error.
*   **Action**: Update the dependencies in `crates/device/Cargo.toml` to resolve the build error.

### 2. Standardize on a Single `config.json`
*   **Problem**: There is confusion between `config.json` and `daemon_config.json`.
*   **Action**: The `config.json` file at the project root will be the single source of truth. The file `crates/device/daemon_config.json` will be ignored.

### 3. Simplify Configuration Loading Logic
*   **Problem**: The current `find-up` logic is complex and can accidentally find configuration files outside the project directory, causing unpredictable behavior.
*   **Action**:
    1.  Remove the `find-up` dependency and its associated logic from `crates/device/src/config.rs`.
    2.  Modify the `load_config` function to look for `config.json` *only* in the current working directory (`./config.json`).
    3.  Ensure the application panics with a clear error message if `./config.json` is not found, instructing the user to run the daemon from the project root.