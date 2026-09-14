---
name: rover-driver-development
description: Develop or extend the rover-drivers workspace's portable async no_std Rust sensor and display drivers. Use for driver APIs, register I2C logic, configuration, catalog features, or driver usage documentation; not for MCU-specific firmware.
---

# Rover Driver Development

Work within the crate that owns the hardware protocol. The workspace is a collection of independent portable drivers in `crates/`; `crates/rover-drivers` is only the feature-gated consumer catalog. Keep platform-specific HAL selection and board glue out of these crates unless a `platforms/` need is explicitly established.

## Driver shape

- Preserve `#![no_std]`, Rust 2024, and the workspace lints: unsafe is forbidden and both `clippy::all` and `clippy::pedantic` are denied.
- Use `embedded-hal-async` traits for I2C and delays. Drivers own their I2C handle and expose `release(self)` when callers need it back; construction should not perform I/O.
- Prefer public typed choices (`Address`, range/filter/rate enums, `Config`) over raw register values. Keep configuration-to-register encoding private unless it is a useful stable API.
- Return a driver error enum generic over the HAL error when that preserves transport diagnostics. Map each failed I2C operation directly to the bus variant. Add explicit errors for device identity, invalid configuration, or lifecycle state when callers can act on them.
- Make initialization and lifecycle guarantees explicit. For stateful hardware, document what a failed or cancelled async operation leaves pending and whether initialization must be retried. Do not claim cancellation safety the HAL cannot guarantee.
- Derive register values and conversion formulae from the relevant data sheet; keep register constants private and named after the hardware register. Preserve specified byte order and convert readings to documented physical units.

## Workspace integration

When adding a public driver crate, give it workspace edition, license, repository, Rust-version, and lint settings. Keep its hardware-specific dependencies local. If firmware users should select it through the catalog, add an optional path dependency, a matching feature, and include it in `all-drivers` in `crates/rover-drivers/Cargo.toml`; re-export it behind the same feature in `crates/rover-drivers/src/lib.rs`.

Update the root README and `docs/using-drivers.md` when the catalog's supported drivers, feature names, or user-facing dependency/import guidance changes. Update the crate README when public behaviour or its example changes. Do not add a platform or re-export-only crate merely to organize imports.

## Before handoff

Run the focused crate tests while iterating. Before completing a change, run the workspace formatting, tests, and strict Clippy checks, plus `cargo check -p rover-drivers --features all-drivers` when catalog wiring changed. Use the companion `rover-driver-validation` skill for protocol-level test coverage.
