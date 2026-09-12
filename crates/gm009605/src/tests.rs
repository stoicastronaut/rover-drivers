extern crate std;

use super::*;
use embedded_graphics_core::geometry::Point;
use embedded_hal::i2c::{ErrorKind, ErrorType, Operation};
use futures::{executor::block_on, task::noop_waker};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll},
    vec::Vec,
};

#[derive(Default)]
struct Bus {
    writes: Vec<(u8, Vec<u8>)>,
    fail_at: Option<usize>,
    pending_at: Option<usize>,
}
impl ErrorType for Bus {
    type Error = ErrorKind;
}
#[allow(clippy::unused_async_trait_impl, reason = "HAL async test double")]
impl I2c for Bus {
    async fn read(&mut self, _: u8, _: &mut [u8]) -> Result<(), ErrorKind> {
        panic!("display must not read")
    }
    async fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), ErrorKind> {
        let index = self.writes.len();
        self.writes.push((address, bytes.to_vec()));
        if self.pending_at == Some(index) {
            core::future::pending::<()>().await;
        }
        if self.fail_at == Some(index) {
            return Err(ErrorKind::Other);
        }
        Ok(())
    }
    async fn write_read(&mut self, _: u8, _: &[u8], _: &mut [u8]) -> Result<(), ErrorKind> {
        panic!("display must not read")
    }
    async fn transaction(&mut self, _: u8, _: &mut [Operation<'_>]) -> Result<(), ErrorKind> {
        panic!("unexpected transaction")
    }
}
#[derive(Default)]
struct Delay(Vec<u32>);
#[allow(clippy::unused_async_trait_impl, reason = "HAL async test double")]
impl DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        self.0.push(ns);
    }
}
fn pixels() -> [Pixel<BinaryColor>; 4] {
    [
        Pixel(Point::new(0, 0), BinaryColor::On),
        Pixel(Point::new(127, 63), BinaryColor::On),
        Pixel(Point::new(5, 7), BinaryColor::On),
        Pixel(Point::new(5, 8), BinaryColor::On),
    ]
}
fn frame(writes: &[(u8, Vec<u8>)]) -> Vec<u8> {
    writes
        .iter()
        .filter(|(_, b)| b[0] == 0x40)
        .flat_map(|(_, b)| b[1..].iter().copied())
        .collect()
}
fn initialization_writes() -> usize {
    let mut display = Gm009605::new(Bus::default(), Address::Primary);
    block_on(display.init(&mut Delay::default())).unwrap();
    display.release().writes.len()
}
fn assert_window(writes: &[(u8, Vec<u8>)]) {
    assert_eq!(writes[0].1, [0, 0x21, 0, 127]);
    assert_eq!(writes[1].1, [0, 0x22, 0, 7]);
}

#[test]
fn initialization_addresses_configuration_and_black_frame() {
    for address in [Address::Primary, Address::Secondary] {
        let mut bus = Bus::default();
        let mut display = Gm009605::new(&mut bus, address);
        assert!(matches!(
            block_on(display.flush()),
            Err(Error::NotInitialized)
        ));
        let mut delay = Delay::default();
        block_on(display.init(&mut delay)).unwrap();
        assert_eq!(display.size(), Size::new(128, 64));
        let _ = display.release();
        assert_eq!(delay.0, [100_000_000]);
        assert!(bus.writes.iter().all(|(a, _)| *a == address as u8));
        for cmd in [
            &[0, 0xAE][..],
            &[0, 0xA8, 63],
            &[0, 0x20, 0],
            &[0, 0x8D, 0x14],
            &[0, 0xAF],
        ] {
            assert!(bus.writes.iter().any(|(_, b)| b == cmd));
        }
        let data = frame(&bus.writes);
        assert_eq!(data, [0; 1024]);
        assert!(bus.writes.iter().all(|(_, b)| b.len() <= 17));
    }
}

#[test]
fn pixel_layout_clipping_clear_and_unchanged_flush() {
    let mut bus = Bus::default();
    let mut display = Gm009605::new(&mut bus, Address::Primary);
    display.draw_iter(pixels()).unwrap();
    display
        .draw_iter([
            Pixel(Point::new(-1, 0), BinaryColor::On),
            Pixel(Point::new(128, 0), BinaryColor::On),
            Pixel(Point::new(0, 64), BinaryColor::On),
            Pixel(Point::new(i32::MAX, i32::MIN), BinaryColor::On),
        ])
        .unwrap();
    block_on(display.init(&mut Delay::default())).unwrap();
    display.draw_iter(pixels()).unwrap();
    block_on(display.flush()).unwrap();
    assert!(!display.dirty);
    display.clear(BinaryColor::On).unwrap();
    block_on(display.flush()).unwrap();
    display.clear(BinaryColor::Off).unwrap();
    block_on(display.flush()).unwrap();
    block_on(display.flush()).unwrap();
    let _ = display.release();
    let data = frame(&bus.writes);
    assert_eq!(data.len(), 3 * 1024);
    let mut expected = [0; 1024];
    expected[0] = 1;
    expected[1023] = 128;
    expected[5] = 128;
    expected[133] = 1;
    assert_eq!(data[..1024], expected);
    assert_eq!(data[1024..2048], [255; 1024]);
    assert_eq!(data[2048..], [0; 1024]);
}

#[test]
fn failed_flush_retries_window_and_entire_frame() {
    // Both window commands, first data transfer, middle transfer, last transfer.
    let base = initialization_writes();
    for fail_at in [0, 1, 2, 30, 65] {
        let mut bus = Bus {
            fail_at: Some(base + fail_at),
            ..Bus::default()
        };
        let mut display = Gm009605::new(&mut bus, Address::Primary);
        block_on(display.init(&mut Delay::default())).unwrap();
        display.draw_iter(pixels()).unwrap();
        assert!(matches!(
            block_on(display.flush()),
            Err(Error::Display(
                display_interface::DisplayError::BusWriteError
            ))
        ));
        assert!(display.dirty);
        block_on(display.flush()).unwrap();
        assert!(!display.dirty);
        let _ = display.release();
        let retry = &bus.writes[base + fail_at + 1..];
        assert_window(retry);
        assert_eq!(frame(retry).len(), 1024);
        assert_eq!(frame(retry)[1023], 128);
    }
}

#[test]
fn cancelled_flush_keeps_full_frame_pending() {
    let base = initialization_writes();
    let mut bus = Bus {
        pending_at: Some(base + 12),
        ..Bus::default()
    };
    let mut display = Gm009605::new(&mut bus, Address::Primary);
    block_on(display.init(&mut Delay::default())).unwrap();
    display.draw_iter(pixels()).unwrap();
    {
        let mut future = pin!(display.flush());
        let waker = noop_waker();
        assert!(matches!(
            future.as_mut().poll(&mut Context::from_waker(&waker)),
            Poll::Pending
        ));
    }
    assert!(display.dirty);
    block_on(display.flush()).unwrap();
    let _ = display.release();
    assert_window(&bus.writes[base + 13..]);
    assert_eq!(frame(&bus.writes[base + 13..]).len(), 1024);
}

#[test]
fn initialization_failure_and_cancellation_require_reinitialization() {
    for pending in [false, true] {
        let mut bus = Bus {
            fail_at: if pending { None } else { Some(25) },
            pending_at: if pending { Some(25) } else { None },
            ..Bus::default()
        };
        let mut display = Gm009605::new(&mut bus, Address::Primary);
        display.draw_iter(pixels()).unwrap();
        let mut delay = Delay::default();
        if pending {
            let mut future = pin!(display.init(&mut delay));
            let waker = noop_waker();
            assert!(
                future
                    .as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending()
            );
        } else {
            assert!(block_on(display.init(&mut delay)).is_err());
        }
        assert!(matches!(
            block_on(display.flush()),
            Err(Error::NotInitialized)
        ));
        block_on(display.init(&mut delay)).unwrap();
        // Reinitialization after power loss must resend an otherwise clean frame.
        block_on(display.init(&mut delay)).unwrap();
        let _ = display.release();
        let data = frame(&bus.writes);
        assert_eq!(data[data.len() - 1], 128);
        assert_eq!(data[data.len() - 1024], 1);
        assert!(data.len() >= 2048);
    }
}
