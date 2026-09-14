# MPU6050

Async, `no_std` I2C driver for the MPU6050 accelerometer and gyroscope. It uses `embedded-hal-async`, owns the bus handle, and does not select an MCU, pins, executor, or allocator.

## Capabilities and scope

- Selects I2C address `0x68` or `0x69`, verifies MPU6050 identity, and accepts the known `0x70` compatible clone identity.
- Configures typed accelerometer range, gyroscope range, digital low-pass filter, and sample rate.
- Reads complete 14-byte bursts as raw register values or converts them to acceleration in m/s², angular velocity in rad/s, and temperature in °C.

Interrupts, FIFO access, motion detection, clock-source selection, and offset calibration are outside the current scope.

## Firmware dependency and quickstart

Enable the driver from the catalog:

```toml
[dependencies]
rover-drivers = { version = "0.1", default-features = false, features = ["mpu6050"] }
```

```rust,ignore
use rover_drivers::mpu6050::{Address, Config, Mpu6050};

let mut sensor = Mpu6050::new(i2c, Address::Primary, Config::default());
sensor.init().await?;

let sample = sensor.read_sample().await?;
// sample.accel_m_s2; sample.gyro_rad_s; sample.temperature_c
```

`Address::Primary` is `0x68`; `Address::Secondary` is `0x69`. The enum selects the bus address and does not change the module's AD0 wiring.

## Configuration and readings

Pass `Config` to `Mpu6050::new`. `Config::default()` selects ±4 g, ±500 °/s, a 44 Hz DLPF, and 200 Hz sampling. Valid sample rates are exact divisors of 1,000 Hz from 1 to 1,000 Hz.

```rust,ignore
use rover_drivers::mpu6050::{AccelRange, Address, Config, Dlpf, GyroRange, Mpu6050};

let config = Config {
    accel_range: AccelRange::G8,
    gyro_range: GyroRange::Dps1000,
    dlpf: Dlpf::Hz94,
    sample_rate_hz: 250,
};
let mut sensor = Mpu6050::new(i2c, Address::Primary, config);
sensor.init().await?;
let raw = sensor.read_raw().await?;
```

`read_raw` exposes signed register values. `read_sample` applies the selected ranges and returns SI units while preserving the sensor's X, Y, and Z axes.

## API and lifecycle behavior

`new` performs no I/O. `init` validates the configured sample rate, checks the identity register, wakes the device, and writes the selected filter, rate, and ranges. Call it before readings: reads themselves are not state-guarded, so a reading before successful initialization may reflect the device's old settings.

An `init` failure or cancellation can leave a subset of registers configured; recover the HAL bus as needed and rerun `init` before using samples. `who_am_i` can be used independently for probing. `release` returns the owned I2C handle.

## Hardware notes

Provide an async I2C bus with suitable pull-ups and match AD0 to the selected address. The driver reports sensor axes as the module defines them; account for the board's physical orientation and calibrate offsets in application code.
