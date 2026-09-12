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

Firmware selects the `bmp388`, `mpu6050`, and `gm009605` drivers through
catalog features. The GM009605 driver supports the four-pin SSD1306 128×64 OLED
with async I2C and `embedded-graphics`; see its [API guide](crates/gm009605/README.md)
and [upstream evaluation](docs/gm009605-driver-evaluation.md).

See [the usage guide](docs/using-drivers.md) for dependency and import examples.
