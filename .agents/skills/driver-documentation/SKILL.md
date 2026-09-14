---
name: driver-documentation
description: Create or update rover-drivers crate READMEs and crate-level Rustdoc for portable async no_std drivers. Use for driver usage, configuration, hardware, lifecycle, and recovery documentation; not for MCU firmware guides.
---

# Rover driver documentation

Document the public behavior of a portable driver crate in its `README.md`. Make the README the crate's Rustdoc landing page with `#![doc = include_str!("../README.md")]` when this does not conflict with necessary crate-level attributes.

## README shape

Follow the established GM009605-style structure, adapting sections to the hardware without adding empty headings:

1. Title and one concise implementation-focused introduction.
2. `Capabilities and scope` for supported behavior and important non-goals.
3. `Firmware dependency and quickstart` using the `rover-drivers` catalog and its relevant feature; use catalog imports in examples.
4. `Configuration and readings` when the driver has configuration or multiple operating modes.
5. `API and lifecycle behavior`.
6. Concise `Hardware notes`.

Keep the writing practical and concise. Explain the driver contract, not how to maintain the driver or choose a board-specific HAL. State units, address selection, and meaningful implementation constraints. Do not promise support for unimplemented hardware features.

## Lifecycle and recovery

For every stateful driver, document which operations perform I/O, the required initialization sequence, state requirements for readings or output, retry or reinitialization behavior after errors and cancellation, and whether `release` returns the owned bus. Base these statements on code and tests; do not claim cancellation safety that the HAL cannot ensure.

Describe hardware setup only where it is device-specific and useful. Keep platform setup in firmware documentation rather than a portable driver README.

## When public documentation changes

Check the crate manifest's `readme`, `description`, `keywords`, and `categories` for new public crates. Update `docs/using-drivers.md` only when catalog features, supported drivers, or catalog-wide guidance changes.

Run `cargo fmt --all -- --check` and the relevant crate tests after changes.
