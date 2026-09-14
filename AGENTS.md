# Repository Guidelines

## Project Structure & Module Organization

This Cargo workspace provides reusable, asynchronous `no_std` embedded Rust
drivers. Each driver is an independent crate under `crates/`:

- `crates/bmp388/` — Bosch BMP388 pressure sensor driver.
- `crates/mpu6050/` — MPU6050 IMU driver.
- `crates/gm009605/` — GM009605/SSD1306 OLED driver, with its tests in
  `src/tests.rs`.
- `crates/rover-drivers/` — feature-gated catalog crate for firmware users.

Keep hardware-neutral code in driver crates; add `platforms/` only for shared,
MCU-specific glue. Put integration guidance in `docs/`, crate-specific usage
notes in each crate's `README.md`, and reference material in `data_sheets/`.

## Build, Test, and Development Commands

- `cargo check --workspace` checks all crates quickly.
- `cargo test --workspace` runs host-side unit tests, including async HAL test
  doubles.
- `cargo clippy --workspace --all-targets -- -D warnings` enforces the
  workspace's strict Clippy policy.
- `cargo fmt --all -- --check` verifies formatting; run `cargo fmt --all` to
  apply it.

Before opening a change, run formatting, tests, and Clippy from the repository
root. To validate catalog wiring, also run `cargo check -p rover-drivers
--features all-drivers`.

## Coding Style & Naming Conventions

Use Rust 2024 and standard `rustfmt` formatting (four-space indentation).
The workspace forbids `unsafe` and denies both `clippy::all` and
`clippy::pedantic`; use narrowly scoped `#[allow(..., reason = "...")]` only
when an embedded-HAL implementation or test double requires it. Prefer typed
configuration enums, `Result`-returning async APIs, and rustdoc on public API.
Use `snake_case` for functions/modules, `PascalCase` for types and enums, and
`SCREAMING_SNAKE_CASE` for registers and other constants.

## Testing Guidelines

Keep unit tests close to the driver: use an internal `#[cfg(test)]` module or
`src/tests.rs`. Name tests after observable behavior, such as
`initialization_requires_reinitialization_after_failure`. Test I2C transfers,
register values, error propagation, and retry/cancellation behavior with fake
HAL buses; do not require physical hardware for the workspace test suite.

## Commit & Pull Request Guidelines

Recent history uses concise imperative subjects and Conventional Commit-style
scopes (for example, `feat(gm009605): add async SSD1306 OLED driver`). Keep a
commit focused on one driver or catalog/docs change. Pull requests should state
the hardware/API impact, list validation commands, link the relevant issue,
and update crate or usage documentation when public behavior changes. Include
screenshots only for display-output changes where they clarify the result.
