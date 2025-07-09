# Unified EEG Pipeline — Holistic Integration Roadmap

## 1  Current State Snapshot

* **Two worlds in one repo:** the *legacy* `PipelineRuntime` / `PipelineStage` graph that passes `PipelineData (Arc<[T]>)`, and the *new* **Hybrid Data‑Plane** built around `Packet<T>`, `MemoryPool`, and lock‑free queues.
* **Stage duplication:** key stages (`filter`, `gain`, `csv_sink`, etc.) exist both under `crates/pipeline/src/stages/` (new) and in `plugins/` (old), with different trait bounds.
* **Plugin loader & UI hooks** are designed but *not wired* (no `PluginManager`, dynamic import, or ABI enforcement yet).
* **Acquisition chain** still feeds the old graph; `ads1299` and `elata_emu_v1` have no `DataPlane` bridge.

## 2  High‑Level Integration Strategy

1. **Co‑existence first, cut over later.** Add *bridge* stages (`ToDataPlane`, `FromDataPlane`) so both runtimes run side‑by‑side. This lets you migrate a slice at a time and benchmark in prod.
2. **Isolate unsafe code.** Finalise `MemoryPool`+`Packet` in a standalone module with loom/miri tests before other work: the whole architecture leans on this.
3. **Hard‑gate plugins through a stable ABI.** Land the `pipeline‑abi` crate *early* so every plugin and host binary compiles against the same struct layouts.
4. **Single source of config truth.** Move all magic numbers (queue sizes, pool counts, stage params) to the new JSON schema so both graphs read from one file.

## 3  Phase Roadmap (60‑90 days)

| Phase               | Goal                     | Key Deliverables                                                                                                                           |
| ------------------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| **P0 (Week 0‑2)**   | Safety floor & bridges   | • `MemoryPool` + tests<br>• `ToDataPlane` & `FromDataPlane`<br>• `HybridRuntime` skeleton with one thread                                  |
| **P1 (Week 3‑4)**   | Sensor ingress           | • `AcquisitionStage` wrapping `ads1299` driver<br>• `ToVoltageStage` producing `Packet<VoltageEeg>`                                        |
| **P2 (Week 5‑6)**   | Minimal vertical slice   | • Port `GainStage`, `WebSocketSink`, `CsvSink` to new trait<br>• Frontend bridge: existing UI reads Data‑Plane WS stream                   |
| **P3 (Week 7‑10)**  | Plugin system online     | • `pipeline‑abi` 1.0<br>• `PluginManager` with `libloading` + version check + `catch_unwind`<br>• `cargo xtask build‑plugins` and template |
| **P4 (Week 11‑13)** | Full migration & cleanup | • Refactor remaining legacy stages<br>• Remove bridge stages, old traits, dead code                                                        |

## 4  Immediate Next‑Actions

1. **Lock the API surface.** Draft the `AbiVersion`, `StageRegistrar`, `DataPlaneStage` traits and land them behind `#[cfg(feature="unstable-abi")]`; iterate until stable.
2. **Finish `MemoryPool` prototype** (crossbeam `ArrayQueue` + drop safety) and benchmark with synthetic 4 kSps×32‑ch load.
3. **Build `AcquisitionStage`** that:

   * Pulls 400‑sample frames from ADS1299 ISR.
   * Writes into `Packet<RawEeg>`, sets `batch_size` header, pushes to queue.
4. **Write bridge stages** to funnel existing `PipelineData::EegPacket` into/out of `Packet<*>`.
5. **Add tracing spans** (`acquire`, `send`, `recv`) so you can eyeball latency via `tokio‑console`.

## 5  Plugin & UI Hardening Checklist

* `plugin.toml` schema v1 (name, version, \[library], \[ui]).
* Static asset layout: `plugins/foo/bin/<triple>/libfoo.so`, `plugins/foo/ui/bundle.js`.
* Kiosk route `/static/plugins/:id/*` and discovery endpoint `/api/plugins`.
* React dynamic `import()`; each bundle exports `default` component.

## 6  Risk & Mitigation

| Risk                     | Impact              | Mitigation                                                |
| ------------------------ | ------------------- | --------------------------------------------------------- |
| UB in `unsafe` pool code | Data corruption     | Loom tests + strict invariants comments                   |
| Plugin panic             | Pipeline crash      | Wrap every call in `catch_unwind`, disable plugin on fail |
| ABI drift                | Loader segfault     | Major/minor check at `register_factories()`               |
| Queue sizing wrong       | Latency/buffer drop | Config‑driven + runtime `tracing` metrics                 |

## 7  Open Decision Points

* **Queue backend:** `rtrb` vs `crossbeam`. Prototype both, pick based on perf + API ergonomics.
* **Python / Wasm guest stages:** embed CPython now or defer until Data‑Plane stabilises?
* **Hot‑reload:** file‑watcher in dev only or prod too? Balance convenience vs. stability.

## 8  Ownership & Next Review

* **Responsible:** @core‑pipeline‑team
* **Dependencies:** ADS1299 driver stabilization, kiosk UI refactor.
* **Checkpoint:** Re‑evaluate after Phase P1 demo (Week 4).

## 9  Gaps & Show‑Stoppers to Watch

| Area                             | Concern                                                                                                  | Suggested Mitigation                                                                                                                  |
| -------------------------------- | -------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| **ISR jitter → queue underrun**  | High‑rate ADS1299 IRQ bursts could starve the Data‑Plane thread if it shares a core with Tokio reactors. | Pin the Data‑Plane thread to an isolated core (`taskset`/`pthread_attr_setaffinity_np`) and benchmark worst‑case latency with `perf`. |
| **MemoryPool exhaustion**        | Pool sizing guess wrong → dropped frames or hard block.                                                  | Add runtime metrics (`pool.free`, `queue.len`) and a guard that logs + expands pool size when free < 5 %.                             |
| **Plugin binary trust**          | Loading un‑signed `.so` from internet could inject malware.                                              | Accept only plugins signed with a project key or run user plugins in Wasmtime sandbox.                                                |
| **Cross‑platform builds**        | Contributors on macOS/Windows may fail to cross‑compile CDylib for Pi (aarch64‑linux‑gnu).               | Provide pre‑configured `cross.toml` + GitHub Action that outputs artefacts for all targets.                                           |
| **Schema drift in config JSON**  | Stage params renamed → legacy configs fail silently.                                                     | Version the JSON schema; validate on load and emit explicit error list.                                                               |
| **CI coverage**                  | Loom/Miri tests can be slow and flaky → skipped.                                                         | Separate “fast” unit tests vs nightly “deep” concurrency suite; gate merges on both.                                                  |
| **Frontend dynamic import race** | UI may render before plugin bundle loads, showing blank panels.                                          | Add suspense fallback + retry logic; cache manifest on SW.                                                                            |

## 10  Nice‑to‑Haves (Not Blocking MVP)

* **Live re‑layout editor** for pipelines with drag‑and‑drop (could be GrapesJS in kiosk).
* **Time‑travel debugger**: record Packet headers + hash to replay a segment offline.
* **Auto‑tuner** that sweeps queue capacities & pool counts to hit latency target.
* **Pipeline presets**: YAML fragments for common tasks (e.g., "alpha‑band filter → FFT → heatmap").

---

*Last updated: 2025-07-09*
