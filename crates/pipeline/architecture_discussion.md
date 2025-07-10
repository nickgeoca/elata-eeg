# Pipeline Architecture: Metadata Propagation Decision Log

## 1. Problem Statement

A robust and scalable pipeline requires a mechanism to provide configuration and context to its various stages. This metadata (e.g., sensor sample rate, voltage reference, calibration constants) must be available to any stage that needs it. The system must handle:

*   **Multiple, heterogeneous sensors** running concurrently.
*   **Live configuration changes** from a GUI or other controller.
*   **Minimal performance overhead** (latency, bandwidth, CPU).
*   **High maintainability** and clear separation of concerns.

---

## 2. Explored Options & Deliberation

### Option A: Local State per Stage

*   **Mechanism:** Each pipeline stage holds its own configuration in local fields (e.g., `struct Filter { cutoff: f32 }`). A central controller (like a GUI) is responsible for calling updater methods on each stage (`filter.set_cutoff(50.0)`).
*   **Initial Appeal:** This model is simple and intuitive for a single, static pipeline.
*   **Critical Flaw:** This pattern fails catastrophically when multiple data sources are present. If packets from two different sensors (with different sample rates) flow through the same filter stage, the stage has no way of knowing which `cutoff` to apply to which packet. Its local state can be updated by the controller for one sensor, leading to silent data corruption for the other. The data and the metadata needed to process it are decoupled, creating a race condition.

### Option B: Self-Describing Data Packets (The "Arc-Meta" Pattern)

*   **Mechanism:** Instead of stages holding state, the data packet itself carries its own configuration metadata. This is achieved by including a shared, immutable reference to a metadata struct in each packet's header.
    ```rust
    pub struct PacketHeader {
        // ... other fields
        pub meta: Arc<SensorMeta>,
    }
    ```
*   **How it Solves the Flaw:** Each packet is now self-contained. A stage inspects the `meta` from the packet it is currently processing. It doesn't matter if packets from different sensors are interleaved; the correct metadata is always attached to the correct data.
*   **Performance:** Using `Arc` (Atomic Reference Counter) is a zero-copy strategy. The `SensorMeta` struct is allocated once. Each packet only carries a lightweight (16-byte) smart pointer. Cloning a packet is extremely cheap, as it only copies the pointer and increments a reference count.

---

## 3. Refinement: Driver Responsibility

A key question arose: *where* should the initial raw-to-physical-unit conversion happen?

### Sub-Option B1: "Smart Driver"

*   **Mechanism:** The sensor driver itself performs the `raw -> volts` conversion. It would output a tuple of `(Vec<f32>, Arc<SensorMeta>)`.
*   **Pros:** One less stage in the pipeline.
*   **Cons:**
    *   Ties physical conversion logic to the hardware driver crate.
    *   Makes it impossible to replay or re-process the original raw data.
    *   Harder to unit-test the conversion logic in isolation.

### Sub-Option B2: "Thin Driver" + Universal `ToVoltage` Stage (Recommended)

*   **Mechanism:** The driver's sole responsibility is to acquire raw data and package it with the correct metadata. It outputs a `Packet<i32>` containing the raw samples and the `Arc<SensorMeta>`. The very first stage in the pipeline is a dedicated, universal `ToVoltage` stage.
*   **Pros:**
    *   **Decoupling:** The driver knows about hardware; the pipeline knows about processing. Clean separation.
    *   **Maintainability:** All scaling logic lives in one, easily testable place. Adding a new sensor often only requires adding a `match` arm to the `ToVoltage` stage.
    *   **Replayability & Debugging:** The original, untouched raw data is available at the start of the pipeline, which is invaluable for debugging, logging, and future analysis.

---

## 4. Final Decision

The chosen architecture is **Option B2: Thin Driver + Universal `ToVoltage` Stage using the `Arc<SensorMeta>` pattern.**

*   **Sensor drivers** are responsible for managing hardware state and producing packets of **raw data** (`i32`) along with an `Arc<SensorMeta>` describing the configuration at the moment of acquisition.
*   A dedicated **`ToVoltage` stage** is the first step in the pipeline. It uses the metadata within each packet to convert raw samples to physical units (`f32` volts).
*   All subsequent stages operate on these physical units, while still having access to the original `SensorMeta` for context (e.g., logging, parameterization).

This model provides the best balance of performance, correctness, and long-term maintainability. It explicitly prevents data corruption from multiple sources and provides a clean, extensible framework for adding new sensors and processing stages.

## Misallaneous notes
### “Arc-meta header” cheat-sheet — every question & answer in one place

| #  | Question / “what-if”                                                 | Answer / design choice                                                                                                                                                       |
| -- | -------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1  | **Why not just repeat YAML fields in every stage?**                  | Human error and drift; we need a *single source of truth*.                                                                                                                   |
| 2  | **Why not let the graph builder copy params at start-up?**           | Breaks on run-time edits and mixed-sensor graphs; debugging becomes opaque.                                                                                                  |
| 3  | **Is embedding metadata in every packet wasteful?**                  | Raw JSON copy *would* be (≈7 % size bump). Solution: wrap once in `Arc<SensorMeta>` → header holds only a shared pointer (16 B).                                             |
| 4  | **What if the config blob grows to 1 kB+?**                          | `Arc` still shares one allocation; per-packet overhead stays 16 B. Throughput hit is <1 %.                                                                                   |
| 5  | **But that’s still a pointer deref each packet—cache miss?**         | Usually L1-hot. If profiling ever shows cost, add a “sticky cache”: compare pointer, refresh scalars only when it changes (≈1 ns check).                                     |
| 6  | **What if we introduce a totally new sensor with different fields?** | `SensorMeta` carries `source_type` + `version`. Consumers `match` on that and handle new keys locally; unaffected stages keep forwarding the same `Arc`. No global refactor. |
| 7  | **What if configs become *huge* (tens of kB)?**                      | Switch to **registry hybrid**: header stores `u32 cfg_id`; global slab maps id → `Arc<SensorMeta>`. Wire-compatible; only builder & first-lookup code change.                |
| 8  | **Who updates configs at run-time?**                                 | The GUI (or any controller) sends `ControlMsg::UpdateParam`. Source stage builds a *new* `Arc<SensorMeta>`; downstream stages see the new pointer on next packet.            |
| 9  | **Does every stage now know JSON?**                                  | No. `SensorMeta` is a typed struct—`meta.vref` is a direct field access. We keep an optional `Map<String, Value>` inside for “user tags” if truly freeform keys are needed.  |
| 10 | **What about boilerplate when adding new stages?**                   | Introduce `stage_def!` macro: param struct + `update_param` + Serde derives auto-generated; new stage ≈ 5 lines.                                                             |
| 11 | **How do we avoid silent ABI drift?**                                | Add `schema_ver: u8` to `PacketHeader`; CI test fails if version mismatch.                                                                                                   |
| 12 | **Anything else to future-proof?**                                   | *Benchmark gate* in CI (fail on >1 % perf regression) and a small `pipeline-messages` crate to keep GUI/runtime control enums in sync.                                       |

---

#### Recommended baseline

```rust
#[derive(Clone)]
pub struct PacketHeader {
    pub ts_ns: u64,
    pub batch_size: u16,
    pub meta: Option<Arc<SensorMeta>>,   // shared, typed, zero-copy
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SensorMeta {
    pub schema_ver: u8,
    pub source_type: String,   // "ADS1299", "ADS2000", …
    pub vref: f32,
    pub adc_bits: u8,
    pub sample_rate: u32,
    // optional: tags: HashMap<String, Value>
}
```

---

### TL;DR for the next engineer / AI

* **Use the Arc-meta header pattern** (Questions 1-6).
* If configs ever dwarf a few kB, **flip to cfg-id + registry** (Q 7).
* GUI orchestrates live updates; pipeline remains stateless apart from packet headers (Q 8).
* Stick to typed structs, macro-generated stage boilerplate, and versioned headers to keep things safe and fast (Q 9-12).
