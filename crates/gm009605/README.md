# GM009605

Async, allocation-free `no_std` driver for the four-pin, 128×64 SSD1306 GM009605 OLED. It uses `embedded-hal-async`, owns the bus handle, and does not select an MCU, pins, executor, or allocator.

## Capabilities and scope

- Supports I2C addresses `0x3C` and `0x3D` with a local 1024-byte framebuffer.
- Implements `embedded_graphics_core::DrawTarget` for text, shapes, images, pixels, and clearing; out-of-bounds coordinates are clipped.
- Sends only changed frames; the unchanged framebuffer produces no I2C traffic.

No raw controller API, rotation configuration, font engine, or MCU wrapper is exposed. Fonts and drawing primitives come from `embedded-graphics`.

## Firmware dependency and quickstart

```toml
[dependencies]
rover-drivers = { git = "https://github.com/stoicastronaut/rover-drivers", default-features = false, features = ["gm009605"] }
embedded-graphics = "0.8"
```

```rust,ignore
use rover_drivers::gm009605::{Address, Error, Gm009605};
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
    display.clear(BinaryColor::Off).unwrap();
    Text::new("Rover ready", Point::new(0, 10), MonoTextStyle::new(&FONT_6X10, BinaryColor::On))
        .draw(&mut display)
        .unwrap();
    display.flush().await
}
```

## API and lifecycle behavior

`new` performs no I/O. `init` waits 100 ms for power-up, configures the controller, and displays the current framebuffer. Drawing is RAM-only and is permitted before initialization, but `flush` requires successful `init`.

Each changed flush transfers all 1024 bytes (about 26 ms at 400 kHz, excluding scheduling); updates are not visually atomic. Failed or cancelled `flush` operations leave the frame pending, so retry once the HAL bus is usable. A failed/cancelled `init` requires `init` again. Reinitialization preserves and resends the framebuffer, including after panel power loss. `release` returns the owned I2C handle.

## Hardware notes

Connect GND, 3.3 V VCC, SDA, and SCL (sometimes labelled SCK), with pull-ups at 3.3 V and a bus rate no higher than 400 kHz. Confirm the module pin order and SSD1306 controller: this driver does not support SH1106 or SPI modules, and the display has no readable identity register. See [the driver evaluation](../../docs/gm009605-driver-evaluation.md) for validation details.
