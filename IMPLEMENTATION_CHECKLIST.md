# Arc-Meta Implementation Checklist (v2)

This checklist tracks the implementation of the "Arc-Meta" pipeline architecture.

- [x] `data.rs`: Implement the v2 `SensorMeta` and `PacketHeader` structs.
- [x] `stages/to_voltage.rs`: Implement the v2 `ToVoltage` stage with the sticky cache.
- [ ] `sensors/.../driver.rs`: Update a driver to produce packets with the v2 `SensorMeta`.
- [ ] `tests.rs`: Implement the property-based test for the scaling logic.
- [ ] `tests.rs`: Implement the end-to-end integration test.
- [ ] `macros.rs`: Begin design of the `stage_def!` macro.
- [ ] `README.md`: Update documentation.
