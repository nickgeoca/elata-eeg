# Implementation Plan: Event-Driven EEG Daemon

This document provides a step-by-step guide for refactoring the EEG daemon to an event-driven architecture. The goal is to fix the "no data" error in the kiosk by properly integrating the `brain-waves-display` plugin.

## Guiding Principles for Implementation

*   **Focus on the "Why":** Each step includes a "Why?" section. Understanding the reason behind a design choice is more important than blindly copying a code snippet.
*   **Conceptual Code:** The code examples are conceptual. They show the intended structure and key types but are not a complete, copy-paste solution. The implementer will need to fill in the logic.
*   **Follow the Existing Plan:** This plan implements the architecture detailed in `todo/event_driven_refactor_plan.md`. Refer to that document for the high-level vision.

---

## Phase 1: Minimum Viable Fix (Getting Data to the Kiosk)

**Goal:** Get the `brain-waves-display` applet working by implementing the core event bus and connection management.

### Step 1.1: Create Core Event Types

*   **File to Modify:** `crates/eeg_types/src/event.rs` (or create `crates/device/src/event.rs` if more appropriate for local events).
*   **What to Do:** Define the shared event structures.
    *   Create a public `EegPacket` struct to hold raw data.
    *   Create a public `FftPacket` struct to hold the results of the FFT calculation.
    *   Create a public `SensorEvent` enum that can wrap different packet types.
*   **Conceptual Code:**
    ```rust
    use std::sync::Arc;

    // Represents a batch of raw data from the sensor
    pub struct EegPacket {
        pub timestamp_ms: u64,
        pub frame_id: u64,
        pub samples: Arc<[f32]>, // Zero-copy for multiple consumers
    }

    // Represents the output of the FFT plugin
    pub struct FftPacket {
        pub source_frame_id: u64,
        pub fft_data: Arc<[f32]>, // Zero-copy
    }

    #[derive(Clone)] // Must be cloneable to be sent across channels
    pub enum SensorEvent {
        RawEeg(Arc<EegPacket>),
        Fft(Arc<FftPacket>),
        // Add other event types here in the future
    }
    ```
*   **Why?** Defining these shared types in `eeg_types` allows different crates (the daemon, plugins) to communicate using a common language. Using `Arc<[T]>` is critical for performance, as it allows multiple plugins to access the same data without copying it.

### Step 1.2: Implement the `EventBus`

*   **File to Create:** `crates/device/src/event_bus.rs`
*   **What to Do:** Implement the central `EventBus`. It should have methods to `subscribe` (which returns a `Receiver`) and `broadcast` (which sends to all subscribers).
*   **Conceptual Code:**
    ```rust
    use tokio::sync::{mpsc, RwLock};
    use crate::event::SensorEvent; // Or eeg_types::event::SensorEvent
    use std::sync::Arc;

    pub struct EventBus {
        subscribers: RwLock<Vec<mpsc::Sender<SensorEvent>>>,
    }

    impl EventBus {
        pub fn new() -> Self { /* ... */ }

        pub async fn subscribe(&self) -> mpsc::Receiver<SensorEvent> {
            let (tx, rx) = mpsc::channel(128); // Bounded channel for back-pressure
            self.subscribers.write().await.push(tx);
            rx
        }

        pub async fn broadcast(&self, event: SensorEvent) {
            // Iterate over subscribers and use try_send.
            // Handle closed channels by removing them from the list.
        }
    }
    ```
*   **Why?** This creates the central nervous system of our application. The `RwLock` allows many plugins to receive events concurrently. Using `try_send` ensures that one slow or crashed plugin cannot block the entire system.

### Step 1.3: Create the `ConnectionManager`

*   **File to Create:** `crates/device/src/connection_manager.rs`
*   **What to Do:** Implement a basic `ConnectionManager` that bridges the `EventBus` to WebSockets.
*   **Conceptual Code:**
    ```rust
    use tokio::sync::mpsc;
    use std::sync::Arc;
    use crate::event_bus::EventBus;
    use crate::event::SensorEvent;

    pub struct ConnectionManager {
        event_bus: Arc<EventBus>,
    }

    impl ConnectionManager {
        pub fn new(event_bus: Arc<EventBus>) -> Self { /* ... */ }

        // This task will run for the lifetime of the application
        pub async fn run(self) {
            let mut rx = self.event_bus.subscribe().await;
            println!("ConnectionManager is running.");

            while let Some(event) = rx.recv().await {
                match event {
                    SensorEvent::Fft(fft_packet) => {
                        // TODO: Find the right WebSocket client(s) and send the data.
                        // For now, we can just print it to confirm it's received.
                        println!("ConnectionManager received FFT data for frame {}", fft_packet.source_frame_id);
                    }
                    _ => {} // Ignore other events for now
                }
            }
        }
    }
    ```
*   **Why?** This isolates all network logic from the core data processing plugins. It subscribes to the event bus just like any other plugin, ensuring a clean and consistent architecture.

### Step 1.4: Create the `Plugin` Trait and Refactor `brain-waves-display`

*   **File to Create:** `crates/device/src/plugin.rs`
*   **What to Do:** Define a simple `Plugin` trait. Then, move the logic from `plugins/brain-waves-display/backend/src/main.rs` into a new struct that implements this trait.
*   **Conceptual Code:**
    ```rust
    // In crates/device/src/plugin.rs
    use async_trait::async_trait;
    use std::sync::Arc;
    use crate::event_bus::EventBus;

    #[async_trait]
    pub trait Plugin: Send + Sync {
        fn name(&self) -> &'static str;
        async fn run(self: Arc<Self>, event_bus: Arc<EventBus>);
    }

    // In a new file, e.g., crates/device/src/plugins/brain_waves.rs
    // (You will need to move the FFT logic here)
    pub struct BrainWavesPlugin;
    #[async_trait]
    impl Plugin for BrainWavesPlugin {
        fn name(&self) -> &'static str { "brain_waves" }
        async fn run(self: Arc<Self>, event_bus: Arc<EventBus>) {
            let mut rx = event_bus.subscribe().await;
            println!("BrainWavesPlugin is running.");
            while let Some(event) = rx.recv().await {
                if let SensorEvent::RawEeg(eeg_packet) = event {
                    // 1. Perform FFT calculation on eeg_packet.samples
                    // 2. Create a new FftPacket
                    // 3. Publish it back to the bus
                    // event_bus.broadcast(SensorEvent::Fft(Arc::new(new_fft_packet))).await;
                }
            }
        }
    }
    ```
*   **Why?** This brings the plugin logic "in-process". It no longer needs to be a separate executable. It becomes a concurrent task within the daemon, allowing for high-speed data sharing via the `EventBus`.

### Step 1.5: Update `main.rs` and `server.rs`

*   **Files to Modify:** `crates/device/src/main.rs`, `crates/device/src/server.rs`
*   **What to Do:**
    1.  In `main.rs`, instantiate the `EventBus`, `BrainWavesPlugin`, and `ConnectionManager`. Spawn them all as supervised `tokio` tasks.
    2.  Modify the existing data acquisition loop in `main.rs` to publish `SensorEvent::RawEeg` events to the `EventBus` instead of using the old `driver_handler`.
    3.  In `server.rs`, add a new `warp` route for `/applet/brain_waves/data`. When a client connects to this route, you must pass their WebSocket connection (`ws`) to the `ConnectionManager`. This is a tricky part; the `ConnectionManager` will need a way to accept new client connections. An `mpsc` channel is a good way to do this.
*   **Why?** This is the final step that connects everything together. It starts all the components and wires the external WebSocket route to the internal `ConnectionManager`, completing the data path.

---

## Phase 2: Robustness (Making the System Stable)

**Goal:** Address the key feedback points to make the Phase 1 implementation production-ready.

*   **Implement Per-Client Back-Pressure:** Modify the `ConnectionManager` to give each connected WebSocket client its own bounded `mpsc::Sender`. If a client's buffer is full, drop data only for that client and log a warning. This prevents one slow client from affecting others.
*   **Implement Selective Event Routing:** Modify the `ConnectionManager` to use a `HashMap`. The key would be an "event topic" (like a string, e.g., `"fft_data"`) and the value would be a `Vec` or `HashSet` of client IDs subscribed to that topic. This avoids sending every event to every client.

---

## Phase 3: Long-Term Health (Scaling and Security)

**Goal:** Implement the remaining architectural improvements for future growth.

*   **Add Security Hooks:** Introduce an authentication/authorization layer in the `ConnectionManager` to secure specific routes as needed.
*   **Improve Task Supervision:** Implement the full supervisor pattern with configurable retries and exponential back-off for all spawned tasks (plugins and managers).
*   **Performance Monitoring:** If, and only if, metrics show the `ConnectionManager` becoming a bottleneck, investigate sharding it into a pool of worker tasks.