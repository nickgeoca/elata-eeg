# Implementation Plan: Pipeline Refinements

This document outlines the plan to address the findings from the architectural assessment.

---

### Phase 1: Address Critical Runtime & Correctness Issues

-   [x] **Runtime:** Replace the inefficient busy-wait sleep in the runtime loop with a blocking `recv_timeout` to reduce idle CPU usage.
-   [x] **ToVoltage Stage:** Fix the fragile pointer-equality cache check. We will explore using `Arc::ptr_eq` or adding a version counter to the metadata. *(Skipped as per user feedback)*
-   [x] **ToVoltage Stage:** Add saturating casts during the `i32` to `f32` conversion to prevent overflows and `NaN` propagation. *(Skipped as per user feedback)*
-   [x] **Testing:** Write a high-throughput integration test to simulate a large volume of packets, ensuring the fixes are effective and preventing future regressions.

### Phase 2: Performance Optimizations & Architectural Hardening

-   [x] **ToVoltage Stage:** Benchmark the per-packet allocations and optimize by pre-allocating `Vec` capacity or exploring in-place conversion. *(Skipped as per user feedback)*
-   [x] **Runtime:** Refactor the two-step event fan-out to a more direct model, removing the extra channel hop.
-   [x] **Runtime:** Implement topology invalidation to ensure the pipeline correctly handles graph mutations from control commands.

### Phase 3: Long-Term Architectural Evolution

-   [ ] **Architecture:** Investigate replacing the `Box<dyn Any>` dynamic dispatch with a more performant and type-safe enum-based approach for common packet types.

---
