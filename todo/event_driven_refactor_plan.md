# Refactoring Plan: Event-Driven Architecture for EEG Daemon

This document outlines the step-by-step plan to refactor the EEG device daemon from a monolithic processing model to a flexible, resilient, in-process, event-driven architecture.


## 1. Goals

*   Eliminate the monolithic `process_eeg_data` function ("god function") in `driver_handler.rs`.
*   Decouple data processing logic into independent, isolated plugins.
*   Establish a robust, high-performance, in-process event bus for data distribution.
*   Enable inter-plugin communication through chained event publishing.
*   Create a scalable architecture that can support multiple sensor types in the future.
*   Improve system stability and debuggability through better task supervision and error handling.

## 2. Core Architectural Principles

We will build the system around the following core principles:

*   **`EventBus`:** A central struct for data distribution. It will use a `tokio::sync::RwLock` to manage a `Vec` of subscribers, providing a good balance of performance and implementation simplicity. *Note: If subscriber churn becomes a bottleneck, this can be evolved to a `DashMap` for more granular, lock-free reads and removals.*
*   **Back-Pressure Policy:** The bus is non-blocking. It uses `try_send` to distribute events. If a plugin's channel buffer is full (checked via `capacity() == 0`), the send is skipped for that plugin, and a metric is incremented. This ensures a slow plugin cannot stall the entire system.
*   **Zero-Copy Data Payloads:** To avoid expensive cloning, all large, fixed-size data packets will be wrapped in an `Arc<[f32]>`. This is chosen over `Arc<Vec<f32>>` to avoid the overhead of `Vec`'s capacity tracking and is more semantically correct for fixed-size data. `Arc` is used over `Box` to allow for safe, shared ownership across multiple plugin threads.
*   **`EegPlugin` Trait:** A common `async_trait` that all plugins must implement. It defines `name()`, `run()`, and a `new(config)` method. The `run` method returns a `Result<()>` for proper error propagation.
*   **Plugin-Managed State & Configuration:** Each plugin is responsible for its own state and is initialized with a dedicated configuration struct.
*   **Graceful Shutdown:** The system will use a `tokio_util::sync::CancellationToken`. This token is passed to each plugin, providing a clear, composable, and ergonomic way to signal and coordinate graceful shutdown, even for nested sub-tasks within a plugin.
*   **Task Supervision & Resilience:** Each plugin runs in a dedicated, named Tokio task. The main application will supervise these tasks. The restart policy will be configurable (e.g., `max_retries: 3`, `backoff_ms: 1000`) and will use an exponential back-off strategy. A terminal `tracing::error!` event will be logged when the system exhausts all retries for a plugin.
*   **Data Integrity:** Data packets will include a monotonic frame counter in addition to a timestamp to make the detection of data gaps trivial.
*   **Incremental Rollout:** The legacy processing path will be kept behind a feature flag during the migration to de-risk the refactor.

## 3. Step-by-Step Implementation Plan

### Step 1: Create the Event Bus and Core Types
1.  **Create `crates/device/src/event.rs`:**
    *   Define data structs like `EegPacket`, ensuring large vectors are wrapped (e.g., `samples: Arc<[f32]>`). Include a `frame_id: u64` field.
    *   Define the `SensorEvent` enum, with variants wrapping data packets in an `Arc` (e.g., `RawEeg(Arc<EegPacket>)`).
2.  **Create `crates/device/src/plugin.rs`:**
    *   Define the `EegPlugin` `async_trait`.
    *   The `run` method signature will be `async fn run(&self, bus: Arc<EventBus>, mut rx: mpsc::Receiver<SensorEvent>, shutdown_token: CancellationToken) -> anyhow::Result<()>`.
3.  **Create `crates/device/src/event_bus.rs`:**
    *   Implement the `EventBus` struct containing `subscribers: RwLock<Vec<mpsc::Sender<SensorEvent>>>`.
    *   Implement `subscribe` which adds a new sender to the list.
    *   Implement `broadcast` which gets a read lock on the subscribers and iterates using `try_send`. It will log errors and remove closed channels.

### Step 2: Refactor `main.rs` to Orchestrate Plugins
1.  **Modify `crates/device/src/main.rs`:**
    *   Instantiate the `EventBus` and the `CancellationToken`.
    *   Instantiate plugins using their respective configuration structs.
    *   For each plugin, subscribe to the bus and spawn it in a supervised task, passing the bus, its receiver, and a clone of the shutdown token.
    *   Implement the supervisor loop that monitors plugin tasks and handles restarts according to the configured policy.
    *   The main data acquisition loop will fetch data, wrap it in an `Arc`'d `SensorEvent`, and call `event_bus.broadcast()`.
    *   Trigger shutdown by calling `cancel()` on the token on Ctrl-C.

### Step 3 & 4: Convert Existing Logic into Plugins
1.  **Refactor `CsvRecorder` and `BasicVoltageFilter`** into their own plugin crates.
2.  Implement the `EegPlugin` trait for each.
3.  The `run` method will contain the core logic, listening for events and respecting the shutdown signal.
4.  The `BasicVoltageFilterPlugin` will publish new `FilteredEeg` events back to the bus.

### Step 5: Deprecate Old Code
1.  Gate the old `process_eeg_data` function call behind the new feature flag.
2.  Once the new system is validated, remove the old function and the feature flag.

## 4. Observability Strategy

Observability is not an afterthought. We will build it in from day one.
*   **Structured Logging:** Use the `tracing` crate across all components. Each plugin task will be spawned with a name and a unique span ID, allowing us to follow a piece of data through the entire pipeline.
*   **Metrics:** Expose a Prometheus endpoint using `hyper`. We will track key metrics, including:
    *   `events_processed_total` (counter, per plugin)
    *   `events_dropped_total` (counter, per plugin)
    *   `plugin_event_queue_depth` (gauge, per plugin)
    *   `plugin_restarts_total` (counter, per plugin)

## 5. Testing Strategy

A multi-layered testing strategy will be implemented to ensure correctness and robustness.
*   **Unit Tests:**
    *   Test the `EventBus` in isolation: verify `subscribe`, `broadcast`, and that it correctly handles full buffers and dead subscribers.
*   **Integration Tests:**
    *   Spin up a test instance with a mock data generator plugin, a filter plugin, and a logger plugin.
    *   Assert that events flow correctly between them and that data is transformed as expected.
    *   Test the shutdown sequence to ensure all plugins terminate gracefully.
*   **Load Tests:**
    *   Create a synthetic data producer that can generate events at high frequency (e.g., 512 Hz x 8 channels).
    *   Use this to validate the back-pressure mechanism, measure latency, and check for memory leaks under sustained load.

## 6. Conceptual Code Example

```rust
// This is the conceptual code for the final, robust event-driven architecture.
// It demonstrates the core principles: CancellationToken for shutdown, Arc for zero-copy,
// RwLock for low-contention, and an optimized, non-blocking broadcast loop.

use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use async_trait::async_trait;
use anyhow::Result;

// --- 1. Core Data Structures (with Arc for zero-copy) ---

#[derive(Debug)]
pub struct EegPacket {
    pub timestamp: u64,
    pub frame_id: u64,
    pub samples: Arc<[f32]>, // Use Arc<[T]> for fixed-size, shared data
}

#[derive(Debug)]
pub struct FilteredEegPacket {
    pub timestamp: u64,
    pub source_frame_id: u64,
    pub filtered_samples: Arc<[f32]>,
}

#[derive(Clone, Debug)]
pub enum SensorEvent {
    RawEeg(Arc<EegPacket>),
    FilteredEeg(Arc<FilteredEegPacket>),
}

// --- 2. The Plugin Trait (with CancellationToken) ---

#[async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;
    async fn run(
        &self,
        bus: Arc<EventBus>,
        mut receiver: mpsc::Receiver<SensorEvent>,
        shutdown_token: CancellationToken,
    ) -> Result<()>;
}

// --- 3. The Event Bus (non-blocking, concurrent) ---

pub struct EventBus {
    subscribers: RwLock<Vec<mpsc::Sender<SensorEvent>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self { subscribers: RwLock::new(Vec::new()) }
    }

    pub async fn subscribe(&self) -> mpsc::Receiver<SensorEvent> {
        let (sender, receiver) = mpsc::channel(128);
        self.subscribers.write().await.push(sender);
        receiver
    }

    pub async fn broadcast(&self, event: SensorEvent) {
        let subscribers = self.subscribers.read().await;
        let mut dead_indices = Vec::new();

        for (i, sender) in subscribers.iter().enumerate() {
            // Optimization: Check capacity first to avoid a clone if the channel is full.
            if sender.capacity() == 0 {
                tracing::warn!(plugin_index = i, "Plugin buffer full. Dropping event.");
                continue;
            }

            // try_send is instantaneous and will fail if the channel is full or closed.
            if let Err(e) = sender.try_send(event.clone()) {
                if let mpsc::error::TrySendError::Closed(_) = e {
                    dead_indices.push(i);
                }
            }
        }

        if !dead_indices.is_empty() {
            let mut subs = self.subscribers.write().await;
            for i in dead_indices.iter().rev() {
                subs.remove(*i);
            }
        }
    }
}

// --- 4. Example Plugin ---

pub struct FilterPlugin;
#[async_trait]
impl Plugin for FilterPlugin {
    fn name(&self) -> &'static str { "filter" }
    async fn run(
        &self,
        bus: Arc<EventBus>,
        mut receiver: mpsc::Receiver<SensorEvent>,
        shutdown_token: CancellationToken,
    ) -> Result<()> {
        tracing::info!("[{}] running.", self.name());
        loop {
            tokio::select! {
                // Combine shutdown signal with recv()
                biased; // Prioritize shutdown
                _ = shutdown_token.cancelled() => {
                    tracing::info!("[{}] received shutdown signal.", self.name());
                    break; // Exit the loop
                }
                Some(event) = receiver.recv() => {
                    if let SensorEvent::RawEeg(raw_packet) = event {
                        // Process...
                        let filtered_packet = Arc::new(FilteredEegPacket {
                            timestamp: raw_packet.timestamp,
                            source_frame_id: raw_packet.frame_id,
                            filtered_samples: raw_packet.samples.iter().map(|s| s * 2.0).collect::<Vec<f32>>().into(),
                        });
                        bus.broadcast(SensorEvent::FilteredEeg(filtered_packet)).await;
                    }
                }
            }
        }
        Ok(())
    }
}

// --- 5. The Main Application Logic (with supervision and CancellationToken) ---

async fn run_system() {
    let bus = Arc::new(EventBus::new());
    let shutdown_token = CancellationToken::new();

    let plugins: Vec<Box<dyn Plugin>> = vec![Box::new(FilterPlugin)];

    for plugin in plugins {
        let bus_clone = Arc::clone(&bus);
        let receiver = bus.subscribe().await;
        let shutdown_clone = shutdown_token.clone();
        tokio::task::Builder::new()
            .name(plugin.name())
            .spawn(async move {
                // The supervisor would have more complex logic here (retries, back-off)
                if let Err(e) = plugin.run(bus_clone, receiver, shutdown_clone).await {
                    tracing::error!(plugin = plugin.name(), "Plugin failed: {}", e);
                }
            });
    }

    // Main data acquisition loop
    let mut frame_counter = 0;
    let shutdown_clone = shutdown_token.clone();
    tokio::spawn(async move {
        loop {
            let raw_packet = Arc::new(EegPacket {
                timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64,
                frame_id: frame_counter,
                samples: vec![1.0, 2.0, 3.0].into(),
            });
            bus.broadcast(SensorEvent::RawEeg(raw_packet)).await;
            frame_counter += 1;
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    });

    // Wait for Ctrl-C
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Ctrl-C received, initiating shutdown.");
        }
        _ = shutdown_token.cancelled() => {
            // Another part of the system could have triggered shutdown
        }
    }

    shutdown_token.cancel();
    // Give plugins a moment to shut down before exiting
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    tracing::info!("System shut down.");
}
```


## Feedback
1 EventBus details
Issue	Why it matters	Quick fix
Cloning even on full channel — you still clone event after the capacity() check if the channel races to full between the check and try_send.	Rare, but under load you’ll pay copy cost for drops.	Re-order: if let Err(e) = sender.try_reserve(1) (nightly) or accept the tiny race; note it in a comment so future devs know.
Vec re-alloc churn when many plugins connect/disconnect (e.g., multiple UI dashboards).	Vec reallocation forces copy of all Senders.	Add a small stretch goal: switch to VecDeque or DashMap when churn > N/second (metric).
2 Task supervision

Log & restart loop is mentioned but not shown. A tiny helper makes it trivial:

```rust
async fn supervise<F>(name: &'static str, max_retries: u8, mut maker: F) -> !
where
    F: FnMut() -> Pin<Box<dyn Future<Output = Result<()>> + Send>>,
{
    let mut attempts = 0;
    loop {
        if let Err(e) = maker().await {
            attempts += 1;
            tracing::error!(plugin = name, %e, "crashed (attempt {attempts})");
            if attempts > max_retries {
                tracing::error!(plugin = name, "giving up after {max_retries}");
                break;
            }
            tokio::time::sleep(backoff(attempts)).await;
        } else { break }
    }
}
```

3 Cancellation ergonomics

CancellationToken::child_token() lets a plugin spawn sub-tasks that are auto-cancelled; mention that in the plugin-author docs so they don’t roll their own channels.
4 Metrics naming nit

Prometheus prefers units in names:

eeg_events_processed_total
eeg_plugin_queue_depth
eeg_frame_latency_seconds

Add a *_seconds histogram for raw→filtered latency; priceless when tuning.
