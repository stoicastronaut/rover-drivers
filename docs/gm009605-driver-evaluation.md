# GM009605 Rust driver evaluation

Evaluated 2026-09-12 using published crate source and repository/release metadata.
The user confirmed the four-pin SSD1306 128×64 module. GM009605 is a module
marking; support here is specifically for that controller and interface.
[A working GM009605 project](https://github.com/asleepatwork/esp8266-oled-gm009605)
also identifies this configuration.

## Candidates

| Candidate | Maintenance evidence | Robustness and suitability |
| --- | --- | --- |
| [`ssd1306` 0.10.0](https://github.com/rust-embedded-community/ssd1306) | Latest release 2025-03-22; latest default-branch commit 2025-06-27 (`79cb629f`); repository not archived. Established community ownership, but no recent release or commit demonstrates active maintenance in September 2026. | `no_std`, Embedded HAL 1.0 async support, bounded I2C chunks, controller configuration and graphics integration. Best reusable foundation. Published buffered mode has a retry issue described below. |
| [`ssd1306-embassy-async` 0.1.0](https://crates.io/crates/ssd1306-embassy-async) | First/current release 2026-05-23. Recent publication, but only one release; ongoing repository maintenance was not established. | Async I2C, typed initialization state, bounds-checked pixels, full-frame flush and native HAL errors. Source asserts on mismatched buffer size, exposes raw data access, and requires `embassy-futures` plus full `embedded-graphics`. Less release history and more runtime coupling than needed. |
| [`ssd1306-i2c` 0.1.5](https://github.com/marvinrobot42/ssd1306-i2c) | Latest release/default-branch commit 2024-07-03; repository not archived. No recent activity found. | Embedded HAL 1.0, but blocking I2C rather than this repository's async model; manifest also includes an `embuild` build dependency. No advantage over the community crate for this task. |

Metadata sources: [community releases](https://github.com/rust-embedded-community/ssd1306/releases),
[community commits](https://github.com/rust-embedded-community/ssd1306/commits/master/),
[Embassy crate source](https://docs.rs/crate/ssd1306-embassy-async/0.1.0/source/src/lib.rs),
[I2C crate manifest](https://docs.rs/crate/ssd1306-i2c/0.1.5/source/Cargo.toml).
These are maintenance indicators, not guarantees of future support or a full audit.

## Decision and recovery behavior

Depend on upstream `ssd1306` 0.10 with only its `async` feature; do not fork or
copy its command implementation. Its MIT/Apache-2.0 licensing permits reuse.
The local wrapper fixes the panel geometry, orientation and supported addresses.

In [0.10.0 buffered graphics source](https://docs.rs/crate/ssd1306/0.10.0/source/src/mode/buffered_graphics.rs),
`flush` clears dirty bounds before awaiting commands/data. A failed or cancelled
transfer can therefore leave the next flush with nothing to send. This driver
uses upstream basic mode and owns its framebuffer. The pending flag clears only
after successful completion. Every attempt restores the complete address window.

[Upstream I2C transport](https://docs.rs/crate/display-interface-i2c/0.5.0/source/src/lib.rs)
sends 16 data bytes per transaction and maps HAL errors to `BusWriteError`.
We retain that limitation explicitly instead of claiming native error fidelity.
The firmware must provide bounded bus timeouts and physical recovery.

The wrapper has four inherent methods: `new`, `init`, `flush`, and `release`.
`DrawTarget` and `OriginDimensions` supply drawing, clearing and dimensions.
No allocator, chip feature, executor or platform-only crate is needed.
Full-frame updates cost bandwidth, but avoid fragile dirty-region recovery.

## Validation

Five host tests exercise both addresses, initialization commands, power-up delay,
black startup, page/bit layout, clipping, clearing, unchanged-frame suppression,
failures at both window commands and multiple data positions, cancellation,
initialization retry, and reinitialization preserving the image.
The README's text-rendering example is compiled as a doctest.

Workspace tests, strict Clippy, and a catalog-only `gm009605` build for
`riscv32imac-unknown-none-elf` pass. This verifies an ESP32-C6-compatible target,
not a complete firmware image or Xtensa toolchain. The HAL-independent trait
boundary supports other ESP32 chips with compatible async I2C implementations.
No physical module was available for testing.
