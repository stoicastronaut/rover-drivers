# rover-drivers

Reusable, `no_std` Rust drivers and embedded domain crates for the Rover
project.

## Layout

- `crates/` contains portable drivers and data-processing crates that work with
  any compatible Embedded HAL implementation. `crates/rover-drivers` is the
  feature-gated catalog crate used by firmware consumers.
- Add a `platforms/` directory only when MCU-specific shared glue is genuinely
  needed; it should not contain re-export-only crates.
- `docs/using-drivers.md` explains how firmware should depend on and import the
  drivers.

The ESP32-C6 firmware consumes `rover-drivers` with just the MPU6050, IMU, and
telemetry features it needs, rather than local copies of the individual driver
crates. Future firmware selects its drivers through the same feature interface.

See [the usage guide](docs/using-drivers.md) for dependency and import examples.
