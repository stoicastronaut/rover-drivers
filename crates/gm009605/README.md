# GM009605

Async, allocation-free `no_std` driver for the **four-pin, 128×64 SSD1306
GM009605 OLED**. Intended for ESP32 firmware using `esp-hal` async I2C;
no chip, pins, executor, or allocator are selected by this crate.

The implementation uses `ssd1306` 0.10's controller commands and transport,
with a local 1024-byte framebuffer to make transfer retries reliable.
See [the driver evaluation](../../docs/gm009605-driver-evaluation.md).

## Minimal API

- `Gm009605::new(i2c, Address::Primary)` constructs without I/O (`0x3C`;
  `Address::Secondary` selects `0x3D`).
- `init(&mut delay).await` waits for power-up, configures the controller and
  displays the current framebuffer (initially black).
- `DrawTarget` supports text, shapes, images, pixels and `clear(BinaryColor::Off)`.
  These operations only change RAM. Coordinates outside 128×64 are clipped.
- `flush().await` sends changed content. Failed or cancelled transfers remain
  pending; retry after the HAL bus is usable. An unchanged frame does no I/O.
- `release()` returns the bus handle.

No raw controller access, font engine, rotation configuration, or MCU wrapper
is exposed. Fonts and shapes come from the application's `embedded-graphics`.

## Firmware dependency

Use the catalog package from another repository:

```toml
[dependencies]
rover-drivers = { git = "https://github.com/stoicastronaut/rover-drivers", default-features = false, features = ["gm009605"] }
embedded-graphics = "0.8"
```

The Git dependency works once these changes are committed and available on the
referenced remote branch. For local development use
`path = "../rover-drivers/crates/rover-drivers"` instead of `git`.

A checked Rust example (direct crate import inside this crate's doctests;
firmware imports `rover_drivers::gm009605::{Address, Error, Gm009605}`):

```no_run
use gm009605::{Address, Error, Gm009605};
use embedded_hal_async::{delay::DelayNs, i2c::I2c};
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    text::Text,
};

async fn show_status(i2c: impl I2c, delay: &mut impl DelayNs) -> Result<(), Error> {
    let mut display = Gm009605::new(i2c, Address::Primary);
    display.init(delay).await?;
    display.clear(BinaryColor::Off).unwrap(); // Infallible: RAM only.
    Text::new("Rover ready", Point::new(0, 10),
        MonoTextStyle::new(&FONT_6X10, BinaryColor::On))
        .draw(&mut display).unwrap();
    display.flush().await?;
    Ok(())
}
```

For `esp-hal` 1.0, construct the firmware's I2C peripheral with `I2c::new`,
assign board-appropriate pins with `with_sda` / `with_scl`, then call
`into_async()`. Pass this handle and an async delay (for example
`embassy_time::Delay`) to the example. The application selects its ESP32 chip,
initializes Embassy's time driver if used, and configures I2C bus timeouts.
[Espressif's async I2C API](https://docs.espressif.com/projects/rust/esp-hal/1.0.0/esp32c6/esp_hal/i2c/master/struct.I2c.html)
documents setup and transfer cancellation. A shared async bus adapter can be
passed instead of the whole peripheral.

## Hardware and behavior

Connect GND, 3.3 V VCC, SDA and SCL (sometimes labeled SCK); keep I2C pull-ups
at 3.3 V. Use a bus rate no higher than 400 kHz. Confirm the module's pin order.
This driver supports the confirmed SSD1306 version, not SH1106 or SPI modules.
The display has no readable identity register to validate its controller.

Each changed flush sends all 1024 bytes, trading bandwidth for simple recovery
(roughly 26 ms at 400 kHz, excluding scheduling). It uses upstream's small I2C
chunks. Display RAM can become partially visible while transferring; this is
not double buffering. Upstream initialization enables the panel before the
first frame is uploaded, so a brief power-up artifact is possible.

A failed/cancelled `init` requires another `init`. Reinitialization preserves
and resends the framebuffer, including after display power loss. Errors expose
upstream `DisplayError`; transport failures lose the HAL-specific error detail
and report `BusWriteError`. Timeouts and physical bus recovery belong to the HAL.

Host tests and an ESP32-C6-compatible RISC-V `no_std` build verify software
behavior. Physical ESP32/display validation is still required: check startup,
text and corner pixels, clearing, and disconnect/reconnect recovery.
