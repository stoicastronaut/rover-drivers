# BMP388

Async, `no_std` I2C driver for the Bosch BMP388 barometric pressure sensor. It uses `embedded-hal-async`, owns the bus handle, and does not select an MCU, pins, executor, or allocator.

## Capabilities and scope

- Selects I2C address `0x76` or `0x77`, verifies chip ID `0x50`, loads factory calibration, and applies typed oversampling, rate, and IIR-filter settings.
- Returns compensated pressure in pascals and temperature in degrees Celsius.
- Supports fresh forced readings and continuous normal-mode readings.

FIFO, interrupts, sensor time, SPI, and altitude calculations are outside the current scope.

## Firmware dependency and quickstart

Enable the driver from the catalog:

```toml
[dependencies]
rover-drivers = { version = "0.1", default-features = false, features = ["bmp388"] }
```

```rust,ignore
use rover_drivers::bmp388::{Address, Bmp388};

let mut sensor = Bmp388::new(i2c, Address::Primary);
sensor.init(&mut delay).await?;

let sample = sensor.measure_forced(&mut delay).await?;
// sample.temperature_celsius; sample.pressure_pa
```

`Address::Primary` is `0x76` (SDO low); `Address::Secondary` is `0x77` (SDO high). The enum selects the bus address and does not change breakout wiring.

## Configuration and measurements

Pass a `Config` to `Bmp388::with_config` before initialization. The driver rejects normal-mode configurations whose conversion time exceeds the selected output-data-rate period.

```rust,ignore
use rover_drivers::bmp388::{Address, Bmp388, Config, IirFilter, OutputDataRate, Oversampling};

let config = Config {
    pressure_oversampling: Oversampling::X8,
    temperature_oversampling: Oversampling::X2,
    output_data_rate: OutputDataRate::Hz25,
    iir_filter: IirFilter::Coefficient3,
};
let mut sensor = Bmp388::with_config(i2c, Address::Primary, config);
sensor.init(&mut delay).await?;
sensor.start_normal_mode().await?;
let sample = sensor.read_normal_measurement(&mut delay).await?;
```

Call `stop_normal_mode` before `configure`. `read_latest` returns the current data registers without waiting for freshness; `read_raw` returns uncompensated ADC values.

## API and lifecycle behavior

`new` and `with_config` perform no I/O. `init` waits for startup, verifies the device, soft-resets it, loads calibration, and leaves it in sleep mode. Compensated measurements and normal mode require successful initialization; `read_raw` does not. A soft reset invalidates cached calibration, so call `init` before another compensated reading.

I2C failures and cancelled operations can leave hardware configuration partially applied; recover the HAL bus as needed and rerun `init` before using compensated output. `release` returns the owned I2C handle.

## Hardware notes

Provide an async `I2c` bus and `DelayNs` implementation. Verify pull-ups, SDO wiring, and the breakout revision during bring-up. The implementation follows the Bosch BMP388 datasheet and BMP3 Sensor API floating-point compensation equations.
