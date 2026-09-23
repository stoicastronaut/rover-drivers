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

Firmware selects the `bmp388`, `mpu6050`, `icm20948`, and `gm009605` drivers through
catalog features. The GM009605 driver supports the four-pin SSD1306 128×64 OLED
with async I2C and `embedded-graphics`; see its [API guide](crates/gm009605/README.md)
and [upstream evaluation](docs/gm009605-driver-evaluation.md).

The [ICM-20948 driver](crates/icm20948/README.md) provides async I2C
accelerometer, gyroscope, temperature, and magnetometer readings.

See [the usage guide](docs/using-drivers.md) for dependency and import examples.

## Library versions

The workspace uses Rust 2024 with a minimum Rust version of 1.98. All four
driver crates (`bmp388`, `mpu6050`, `icm20948`, `gm009605`) and the
`rover-drivers` catalog are version `0.1.0`.

The table lists direct external dependencies. Requirements come from each
crate's `Cargo.toml`; resolved versions come from `Cargo.lock`. Requirements
allow compatible updates and are not exact pins.

| Library | Cargo requirement | Resolved version | Used by / purpose |
| --- | --- | --- | --- |
| `embedded-hal-async` | `1.0.0` (BMP388); `1.0` (others) | `1.0.0` | All four drivers: async HAL traits |
| `embedded-graphics-core` | `0.4` | `0.4.1` | GM009605: drawing traits and pixel types |
| `display-interface` | `0.5` | `0.5.0` | GM009605: display interface errors |
| `ssd1306` | `0.10.0` | `0.10.0` | GM009605: controller support; defaults disabled, `async` enabled |
| `embedded-hal` | `1.0` | `1.0.0` | All four drivers: test-only HAL bus doubles |
| `futures` | `0.3.34` | `0.3.34` | All four drivers: test-only async execution, `executor` enabled |
| `embedded-graphics` | `0.8` | `0.8.2` | GM009605: test-only drawing support |

The catalog has optional local dependencies on each driver at version `0.1.0`
and no direct external dependencies. `Cargo.lock` records transitive versions.
Update this inventory when dependency requirements or resolved versions change.
