# BMP388

An asynchronous, `no_std` I2C driver for the Bosch BMP388 barometric pressure
sensor, built on `embedded-hal-async` 1.0.

## Features

- I2C addresses `0x76` and `0x77`
- identity checking and bounded soft-reset readiness waits
- factory calibration loading and Bosch-compatible floating-point compensation
- typed pressure/temperature oversampling, output data rate, and IIR filtering
- fresh one-shot forced measurements
- continuous normal-mode measurements
- explicit pressure in pascals and temperature in degrees Celsius
- no MCU-specific dependencies

FIFO, interrupts, sensor time, SPI, and altitude calculations are outside the
current v1 scope.

## Usage

Add the crate directly:

```toml
[dependencies]
bmp388 = "0.1"
embedded-hal-async = "1.0"
```

Or enable it through the workspace catalog:

```toml
[dependencies]
rover-drivers = { version = "0.1", default-features = false, features = ["bmp388"] }
```

Initialize the sensor and request a fresh forced measurement:

```rust,ignore
use bmp388::{Address, Bmp388};

let mut bmp388 = Bmp388::new(i2c, Address::Primary);
bmp388.init(&mut delay).await?;

let sample = bmp388.measure_forced(&mut delay).await?;
log::info!(
    "temperature={} C pressure={} Pa",
    sample.temperature_celsius,
    sample.pressure_pa,
);
```

`Address::Primary` selects `0x76` (SDO low), while `Address::Secondary` selects
`0x77` (SDO high). The enum tells the driver which device address to use; it
does not electrically change the breakout's address.

## Configuration

Use `Bmp388::with_config` before initialization, or call `configure` while the
sensor is not in normal mode:

```rust,ignore
use bmp388::{Address, Bmp388, Config, IirFilter, OutputDataRate, Oversampling};

let config = Config {
    pressure_oversampling: Oversampling::X8,
    temperature_oversampling: Oversampling::X2,
    output_data_rate: OutputDataRate::Hz25,
    iir_filter: IirFilter::Coefficient3,
};

let mut bmp388 = Bmp388::with_config(i2c, Address::Primary, config);
bmp388.init(&mut delay).await?;
```

The driver rejects normal-mode configurations whose pressure and temperature
conversion time cannot fit inside the selected output-data-rate period.

For continuous measurements:

```rust,ignore
bmp388.start_normal_mode().await?;

loop {
    let sample = bmp388.read_normal_measurement(&mut delay).await?;
    // Use sample.pressure_pa and sample.temperature_celsius.
}
```

Call `stop_normal_mode` before applying a new configuration. `read_latest` is
available when an application explicitly wants the current register contents
without waiting for freshness; `read_raw` exposes the uncompensated ADC values
for advanced use.

## Integration notes

- The application supplies the I2C peripheral and an async `DelayNs`
  implementation; neither is tied to a particular microcontroller.
- `init` waits for startup, checks chip ID `0x50`, performs a soft reset, reads
  the 21-byte calibration block, and writes the selected configuration.
- Soft reset invalidates cached calibration. Call `init` again before requesting
  compensated output.
- Pressure compensation depends on the temperature-derived `t_lin` value, so
  the driver always reads and compensates pressure and temperature together.
- Validate pull-ups, SDO wiring, and the breakout PCB revision during hardware
  bring-up.

The implementation follows the Bosch BMP388 datasheet and BMP3 Sensor API
floating-point compensation equations.
