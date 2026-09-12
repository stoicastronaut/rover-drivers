#![no_std]
#![doc = include_str!("../README.md")]

use core::convert::Infallible;
use embedded_graphics_core::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::BinaryColor,
};
use embedded_hal_async::{delay::DelayNs, i2c::I2c};
use ssd1306::{
    I2CDisplayInterface, Ssd1306Async,
    command::AddrMode,
    mode::BasicMode,
    prelude::{DisplayRotation, DisplaySize128x64, I2CInterface},
};

/// The two supported seven-bit I2C addresses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Address {
    /// Factory address on most modules.
    #[default]
    Primary = 0x3C,
    /// Address selected by the module's address jumper.
    Secondary = 0x3D,
}

/// Initialization or transfer failure.
#[derive(Debug)]
pub enum Error {
    /// Call `init` successfully before flushing.
    NotInitialized,
    /// Upstream transport/controller error. I2C errors become `BusWriteError`;
    /// the upstream interface does not retain the HAL-specific error value.
    Display(display_interface::DisplayError),
}

/// Fixed 128×64, unrotated, monochrome display with a 1024-byte framebuffer.
///
/// Owns an async I2C bus handle (which may be a shared-bus adapter or borrow).
/// Drawing only changes RAM; call [`Self::flush`] to send it to the OLED.
pub struct Gm009605<I2C> {
    controller: Ssd1306Async<I2CInterface<I2C>, DisplaySize128x64, BasicMode>,
    buffer: [u8; 1024],
    initialized: bool,
    dirty: bool,
}

impl<I2C: I2c> Gm009605<I2C> {
    /// Construct without performing I/O.
    #[must_use]
    pub fn new(i2c: I2C, address: Address) -> Self {
        Self {
            controller: Ssd1306Async::new(
                I2CDisplayInterface::new_custom_address(i2c, address as u8),
                DisplaySize128x64,
                DisplayRotation::Rotate0,
            ),
            buffer: [0; 1024],
            initialized: false,
            dirty: true,
        }
    }

    /// Wait 100 ms for module power-up, configure the OLED and send the framebuffer.
    ///
    /// Call after power is stable. Drawing before initialization is supported.
    /// Reinitialization preserves the image and resends it after a panel power loss.
    /// There is no hardware reset pin or readable identity on this module.
    /// If this future fails or is cancelled, retry `init` before `flush`.
    ///
    /// # Errors
    /// Returns [`Error::Display`] on a controller/transport failure.
    pub async fn init(&mut self, delay: &mut impl DelayNs) -> Result<(), Error> {
        self.initialized = false;
        self.dirty = true;
        delay.delay_ms(100).await;
        self.controller
            .init_with_addr_mode(AddrMode::Horizontal)
            .await
            .map_err(Error::Display)?;
        self.send_frame().await?;
        self.dirty = false;
        self.initialized = true;
        Ok(())
    }

    /// Send the complete framebuffer if it changed, otherwise perform no I/O.
    ///
    /// Errors and cancellation leave the frame pending. Retry after the bus is
    /// usable; each attempt resets the column/page window and resends all pixels.
    /// Cancellation safety of the physical bus depends on the HAL. After panel
    /// power loss, call `init` instead. Updates are not visually atomic.
    ///
    /// # Errors
    /// Returns [`Error::NotInitialized`] before successful initialization, or
    /// [`Error::Display`] on a controller/transport failure.
    pub async fn flush(&mut self) -> Result<(), Error> {
        if !self.initialized {
            return Err(Error::NotInitialized);
        }
        if self.dirty {
            self.send_frame().await?;
            self.dirty = false;
        }
        Ok(())
    }

    /// Return the owned bus handle without changing the panel's displayed image.
    #[must_use]
    pub fn release(self) -> I2C {
        self.controller.release().release()
    }

    async fn send_frame(&mut self) -> Result<(), Error> {
        self.controller
            .set_draw_area((0, 0), (128, 64))
            .await
            .map_err(Error::Display)?;
        self.controller
            .draw(&self.buffer)
            .await
            .map_err(Error::Display)
    }
}

impl<I2C> OriginDimensions for Gm009605<I2C> {
    fn size(&self) -> Size {
        Size::new(128, 64)
    }
}

impl<I2C> DrawTarget for Gm009605<I2C> {
    type Color = BinaryColor;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            let (Ok(x), Ok(y)) = (usize::try_from(point.x), usize::try_from(point.y)) else {
                continue;
            };
            if x >= 128 || y >= 64 {
                continue;
            }
            let byte = &mut self.buffer[x + (y / 8) * 128];
            let mask = 1 << (y % 8);
            let value = if color == BinaryColor::On {
                *byte | mask
            } else {
                *byte & !mask
            };
            self.dirty |= value != *byte;
            *byte = value;
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        let value = if color == BinaryColor::On { 0xFF } else { 0 };
        self.dirty |= self.buffer.iter().any(|byte| *byte != value);
        self.buffer.fill(value);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
