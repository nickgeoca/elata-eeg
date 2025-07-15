# Pipeline Feature Backlog

*(snapshot – July 15 2025)*

---

## 1  Implemented / Stable

* Directed‑acyclic‑graph (DAG) pipeline architecture
* Stage registry & factory pattern
* **Single‑core** synchronous runtime loop (Tokio removed for now) with pre‑computed topology sort
* Broadcast‑based **fan‑out (TEE)** per stage
* Support for multiple downstream consumers per stage
* Support for multiple independent source sensors
* Stage‑level caching (e.g., `ToVoltage` scale/offset)
* Explicit data‑contract enforcement at sinks
* Built‑in stages: **acquire**, **to\_voltage**, **filter**
* Built‑in sinks: **websocket\_sink**, **csv\_sink**
* Graceful drain & sink flush on shutdown
* Versioned, JSON‑serialisable pipeline configs
* Basic error handling & propagation
* Stage‑level locking guarantees (one mutable owner per stage)
* External event channel for introspection & UI
* Topology invalidation & recompute after mutations

## 2  In Progress or Needs Approval First

* Runtime control‑command framework (shutdown ✔, others TBD)
* **Hot‑swap pipeline configs** – scaffolding present, safe graph‑mutation WIP
* Fan‑in / cross‑sensor fusion stage(s) (`AlignAndZip` prototype)
* Performance tuning: pre‑allocation & in‑place buffer reuse
* Multicore / thread‑pool pipeline execution prototype
* Unit & stress‑test harness (simulated million‑packet flows)

## 3  Debating to Implement

* Visual pipeline editor (graphical UI)
* Runtime hot‑swap with ref‑count drain & garbage collection
* Dynamic stage attach/detach while running
* Advanced error recovery & retry policies
* Metrics & observability (Prometheus / OpenTelemetry)
* Config‑driven stage concurrency & batching
* Feature‑flag framework for experimental stages
* External plugin API (third‑party stage crates)
* Auto‑generated documentation from stage metadata

## 4  Parked / Dropped / Deferred

* Event‑bus plugin model (replaced by DAG)
* Implicit data‑flow discovery (now explicit edges)
* *(add other removed ideas here)*

---

**Legend**
*Implemented* – feature is production‑ready.
*In Progress* – code exists but not fully integrated or tested.
*Planned* – agreed to build, no code yet.
*Parked* – intentionally postponed or replaced.

> Keep this file as the *single source of truth*; update it whenever scope changes to avoid feature thrash.
