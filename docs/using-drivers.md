# Using Rover drivers

The portable `no_std` crates live under `crates/`. Firmware selects drivers
through the `rover-drivers` catalog, which has no default features.

## Depend on the catalog

For a firmware repository beside this checkout:

```toml
[dependencies]
rover-drivers = { path = "../rover-drivers/crates/rover-drivers", default-features = false, features = ["gm009605"] }
embedded-graphics = "0.8"
```

Once the desired revision is available on the remote, a Git dependency also works:

```toml
[dependencies]
rover-drivers = { git = "https://github.com/stoicastronaut/rover-drivers", default-features = false, features = ["gm009605"] }
```

Pin `rev` to a committed revision for reproducible firmware builds.

```rust
use rover_drivers::gm009605::{Address, Gm009605};
```

The catalog does not select a chip or own a HAL. Choose the ESP32 chip in the
firmware's `esp-hal` dependency, create its async I2C peripheral, and pass the
handle to the driver. Drivers can also accept compatible shared-bus adapters.

## Features

| Feature | Import | Device |
| --- | --- | --- |
| `bmp388` | `rover_drivers::bmp388` | Bosch pressure/temperature sensor |
| `icm20948` | `rover_drivers::icm20948` | Nine-axis IMU and die temperature, magnetometer via I2C bypass |
| `mpu6050` | `rover_drivers::mpu6050` | Accelerometer/gyroscope |
| `gm009605` | `rover_drivers::gm009605` | Four-pin SSD1306 128×64 OLED |
| `all-drivers` | All of the above | Every catalog entry |

See the [GM009605 API and drawing example](../crates/gm009605/README.md) and
[upstream driver evaluation](gm009605-driver-evaluation.md).

See the [ICM-20948 API and quickstart](../crates/icm20948/README.md) for
configuration, initialization, and the magnetometer bypass address constraint.

## Direct portable-crate use

Applications can also depend directly on an individual crate:

```toml
[dependencies]
gm009605 = { path = "../rover-drivers/crates/gm009605" }
```
