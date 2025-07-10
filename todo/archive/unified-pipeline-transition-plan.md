> [!WARNING]
> # DEPRECATED: Outdated Architectural Vision
> This document describes an architectural vision for a dynamic, ABI-driven plugin system that is NOT currently implemented in the codebase. The current implementation uses a pure-Rust, inventory-based static plugin registration system. This document is retained for historical context but should not be used as a reference for the current architecture.

# Unified Pipeline Architecture: Implementation Plan (ABI-First)

## 1. Executive Summary
This document outlines the implementation plan for the new **ABI-driven** hybrid sensor pipeline. The previous "transition plan" involving bridge stages is now **obsolete**. We are pursuing a direct implementation of the final architecture, centered on a stable Application Binary Interface (ABI) that allows for a dynamic, robust, and high-performance plugin ecosystem. The goal is to enable developers to add new pipeline stages by simply cloning a repository and building it, without any changes to the host application.

## 2. Core Principle: The Stable ABI
The cornerstone of this architecture is the `pipeline-abi` crate. It defines a stable C-compatible contract, the `StageDescriptor`, which every plugin must expose. This allows the host's `PluginManager` to dynamically load, verify, and integrate plugins at runtime.

This approach eliminates the need for a gradual migration and allows us to build the final, desired system from the outset.

## 3. Implementation Strategy: Direct Adoption
The implementation will follow the phases outlined in the `IMPLEMENTATION_CHECKLIST.md`, which prioritizes the ABI-first model.

*   **Phase 1: The Stable ABI & Plugin System:**
    1.  **`pipeline-abi` Crate:** Create the dedicated, versioned crate defining the `StageDescriptor`, `DataPlaneStage` trait, and the `register_stage` function signature.
    2.  **`PluginManager`:** Implement the host-side service to load dynamic libraries, validate their ABI version via the `StageDescriptor`, and populate a `StageRegistry`. All calls into plugin code will be wrapped in `std::panic::catch_unwind`.
    3.  **Developer Tooling:** Create `cargo xtask` scripts and `cargo generate` templates to standardize plugin creation.

*   **Phase 2: Initial Vertical Slice (ABI-Native):**
    1.  **Core Stages as Plugins:** Implement essential stages like `AcquisitionStage`, `ToVoltageStage`, and `WebSocketSink` as ABI-compliant plugins. This proves the model and provides foundational capabilities.
    2.  **Configuration:** The `GraphBuilder` will be updated to read a JSON configuration that references stages by their unique string IDs (e.g., `"com.mycorp.fft"`). It will use the `PluginManager`'s registry to look up the corresponding constructor and instantiate the graph.

*   **Phase 3: UI Integration & Full Adoption:**
    1.  **UI Discovery:** Implement a `/api/plugins` endpoint on the server that serves `plugin.toml` manifests.
    2.  **Dynamic UI:** The Kiosk frontend will fetch from this endpoint and use dynamic `import()` to load the UI bundles for available plugins, removing all hardcoded imports.
    3.  **Deprecation:** Once the new system is fully functional, the legacy `PipelineStage` trait, its associated runtime logic, and any unused plugin loaders will be removed from the codebase.

## 4. Key Architectural Decisions

*   **No Bridge Stages:** The `ToDataPlane` and `FromDataPlane` stages will not be implemented. The legacy and new systems will coexist during development but will not interact in a running pipeline.
*   **Plugin-Owned Metadata:** All stage metadata (ID, version, constructor) is owned by the plugin and exposed via the `StageDescriptor`. The host has no compile-time knowledge of specific plugins.
*   **Fault Isolation:** The `PluginManager`'s use of `catch_unwind` is critical for stability, ensuring that a faulty plugin disables itself without crashing the entire application.

This direct implementation plan is cleaner, avoids the technical debt of a temporary migration, and accelerates development towards the final, robust, and extensible architecture.