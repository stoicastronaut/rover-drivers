# Using Rover drivers

The portable crates live under `crates/` and use `no_std`:

- `mpu6050` provides asynchronous Embedded HAL 1.0 I2C register access and
  samples converted to SI units.
- `rover-imu` provides axis transforms, stationary gyro calibration, and
  six-axis Madgwick orientation estimation.
- `rover-telemetry` provides allocation-free version-1 JSON Lines telemetry
  serialization.

Firmware should depend on the feature-gated catalog crate and enable only the
components it uses. For an ESP32 firmware project located beside this
repository:

```toml
[dependencies]
rover-drivers = { path = "../../../rover-drivers/crates/rover-drivers", default-features = false, features = ["mpu6050", "imu", "telemetry"] }
```

Import the enabled components through that package:

```rust
use rover_drivers::{
    mpu6050::{Address, Config as MpuConfig, Mpu6050},
    imu::{ImuMeasurement, Vector3},
    telemetry::{Envelope, Message, serialize_json_line},
};
```

The catalog crate does not select a chip or own a HAL. Choose the target and
ESP32 feature in the consuming firmware's `esp-hal` dependency, create its
asynchronous I2C peripheral, and pass it to `Mpu6050`.

## Features

`rover-drivers` has no default features. Enable only the components required by
your firmware:

- `mpu6050` re-exports `mpu6050` as `rover_drivers::mpu6050`.
- `imu` re-exports `rover-imu` as `rover_drivers::imu`.
- `telemetry` re-exports `rover-telemetry` as `rover_drivers::telemetry`.
- `all-drivers` enables every catalog entry, primarily for examples and tests.

For a firmware that only reads the MPU6050:

```toml
[dependencies]
rover-drivers = { path = "../rover-drivers/crates/rover-drivers", default-features = false, features = ["mpu6050"] }
```

## Direct portable-crate use

Applications that intentionally target more than one MCU can also depend
directly on a portable package instead:

```toml
[dependencies]
mpu6050 = { path = "../rover-drivers/crates/mpu6050" }
```

Use a platform crate when it exists for the target MCU so MCU-level driver
selection stays centralized.
