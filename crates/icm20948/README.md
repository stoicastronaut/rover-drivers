# ICM-20948

Async, allocation-free `no_std` I2C driver for the ICM-20948 accelerometer,
gyroscope, die temperature sensor, and embedded AK09916 magnetometer. Uses
`embedded-hal-async` and owns the bus handle.

## Capabilities and scope

- Both host addresses (`0x68` and `0x69`), all accelerometer/gyroscope ranges,
  selectable low-pass filters, and independent sample-rate dividers.
- Raw signed counts and acceleration in m/s², angular velocity in rad/s,
  temperature in °C, and magnetic flux density in µT.
- Optional magnetometer at 10, 20, 50, or 100 Hz through I2C bypass, with
  data-ready, overrun, and overflow handling.

No SPI, internal auxiliary-bus master, FIFO, interrupts, DMP firmware, sensor
fusion, calibration, self-test, or low-power mode API is implemented. Filters
remain enabled; the gyro supports DLPF settings 1–6, where the datasheet
explicitly permits the sample-rate divider.

## Firmware dependency and quickstart

For a firmware checkout beside this repository:

```toml
[dependencies]
rover-drivers = { path = "../rover-drivers/crates/rover-drivers", default-features = false, features = ["icm20948"] }
embedded-hal-async = "1.0"
```

```rust,ignore
use embedded_hal_async::{delay::DelayNs, i2c::I2c};
use rover_drivers::icm20948::{Address, Config, Error, Icm20948};

async fn read_sensor<I: I2c>(i2c: I, delay: &mut impl DelayNs) -> Result<(), Error<I::Error>> {
    let mut sensor = Icm20948::new(i2c, Address::Primary, Config::default());
    sensor.init(delay).await?;
    let _motion = sensor.read_sample().await?;
    if let Some(_magnetic) = sensor.read_magnetic_sample().await? {
        // Consume magnetic.magnetic_ut and check magnetic.overrun.
        // Apply board-specific axis mapping and calibration before sensor fusion.
    }
    let _i2c = sensor.release();
    Ok(())
}
```

## Configuration and readings

`Config::default()` selects ±4 g, ±500 °/s, a 50.4 Hz accelerometer filter,
a 51.2 Hz gyro filter, divider 4 for both inertial sensors (nominally 225 Hz),
and a 100 Hz magnetometer. Change `Config` before construction; to change it
later, release the bus, construct another driver, and initialize again.

| Setting | Supported values |
| --- | --- |
| `accel_range` | ±2, ±4, ±8, ±16 g |
| `gyro_range` | ±250, ±500, ±1000, ±2000 °/s |
| `accel_dlpf` | 246, 111.4, 50.4, 23.9, 11.5, 5.7, 473 Hz |
| `gyro_dlpf` | 151.8, 119.5, 51.2, 23.9, 11.6, 5.7 Hz |
| `accel_sample_rate_divider` | 0–4095 |
| `gyro_sample_rate_divider` | 0–255 |
| `magnetometer` | `Disabled`, `Hz10`, `Hz20`, `Hz50`, `Hz100` |

Nominal inertial output rate is `1125 / (1 + divider)` Hz. Accelerometer and
gyroscope reads use one 14-byte, big-endian register burst; they return the
latest registers without waiting for fresh data and may repeat samples. The
magnetometer runs independently and is read separately, so the two readings
are not a synchronized nine-axis snapshot.

`read_magnetic_raw` and `read_magnetic_sample` return `Ok(None)` when no data is
ready. A ready sample reports skipped measurements via `overrun`. Overflow
returns `Error::MagneticOverflow` and discards that reading. Each nine-byte
little-endian magnetic burst includes ST1, six data bytes, the unused byte at
`0x17`, and ST2 to complete the hardware read sequence. Scale is 0.15 µT/count.

All readings retain their native sensor axes. **AK09916 axes differ from the
accelerometer/gyroscope axes**; apply the datasheet orientation and board
mounting transform before combining them. Acceleration includes gravity.
Temperature is die temperature, computed as `raw / 333.87 + 21`, and is not an
ambient-temperature measurement. No bias or magnetic compensation is applied.

## API and lifecycle behavior

`new` performs no I/O. `init` validates the divider before I/O, waits 100 ms
for power-up, checks ICM identity `0xEA`, resets it, waits another 100 ms, and
configures continuous low-noise operation. With magnetometer enabled it
exposes bypass, resets AK09916, waits 1 ms, checks identity `0x09`, and sets
its rate. A final 100 ms allows sensor startup; this does not guarantee a fresh
sample at every configured output rate. Use an async HAL delay implementation.

All sample methods reject calls before successful initialization. A failed or
cancelled `init`, including reinitialization of a working device, blocks sample
reads until a complete `init` succeeds. Retrying reapplies the full sequence.
Call `init` again after power loss or an external reset. `who_am_i` performs
bank selection and I/O but neither initializes nor resets the device; allow
power-up time before using it independently.

Every inertial read explicitly selects bank zero. Bank state is never cached,
so an interrupted bank write cannot poison subsequent driver operations.
Read errors preserve the original HAL error as `Error::Bus`; reads can be
retried once the HAL bus is usable. A cancelled/failed magnetic read may lose a
measurement or leave the data latch pending; retrying the complete burst reads
ST2 and releases it, even when no fresh data is reported. The driver cannot
repair a HAL peripheral left busy by cancellation.

`release` returns the owned I2C handle without I/O or powering down sensors;
**bypass remains enabled** if it was configured. Do not let other users of a
shared bus change this device's registers while the driver is active.

## Hardware notes

AD0 low selects `Address::Primary`; high selects `Address::Secondary`. The
AK09916 has a fixed `0x0C` address exposed by bypass. Only one enabled bypass
magnetometer may occupy that host-bus address; this includes two ICM-20948s at
different host addresses. `MagnetometerMode::Disabled` closes bypass and avoids
AK09916 transactions; it does not explicitly power down an already running
AK09916. Magnetic read methods then return `Error::MagnetometerDisabled`.

The bare IC requires VDD 1.71–3.6 V and **VDDIO 1.71–1.95 V**. Do not assume
3.3 V I/O tolerance: check breakout regulation and level shifting, and use
appropriate pull-ups. Use I2C at no more than 400 kHz and the datasheet wiring
for I2C mode (including nCS high).

Register sequences, scales, and limits follow TDK DS-000189, particularly
sections 3, 8, 10, 12, and 13 of the
[ICM-20948 datasheet](https://invensense.tdk.com/wp-content/uploads/2016/06/DS-000189-ICM-20948-v1.3.pdf).
The [TDK eMD user guide](https://invensense.tdk.com/wp-content/uploads/2016/06/App-Note-eMD-20x48-User-Guide.pdf)
identifies the compass address; the [AKM AK09916 datasheet](https://www.micro-processor.sg/parts-file/78-ak09916c.pdf)
specifies continuous-mode rates and mode-change timing.
