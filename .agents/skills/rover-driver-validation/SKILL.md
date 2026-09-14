---
name: rover-driver-validation
description: Test and validate changes to the rover-drivers async no_std Rust workspace. Use for I2C fake-bus tests, error and cancellation behavior, register sequences, feature wiring, and workspace quality checks.
---

# Rover Driver Validation

Test observable protocol and lifecycle behaviour without physical hardware. Each driver crate's host-side unit tests use `embedded-hal` test doubles and `futures` to exercise its `embedded-hal-async` API.

## Test what the driver promises

- Make fake I2C buses assert the exact ordered address, register bytes, operation type, and response length. Use a queue of expectations when a test needs to prove a multi-step initialization sequence.
- Cover both supported addresses where the public API exposes them; test byte order and physical-unit conversion with representative raw samples.
- Assert invalid configuration fails before hardware configuration writes when possible. Test an unexpected identity and bus failures at meaningful read and write boundaries, and verify the returned driver error preserves that distinction.
- For hardware with initialization, test I/O-free construction, successful initialization, behaviour before initialization, and `release` ownership. For retryable operations, fail an individual transport step and confirm a retry sends the required setup/window and a complete update rather than assuming partially sent state is valid.
- When a future can remain pending or be cancelled, poll it with a no-op waker, drop it, and assert the documented recovery path. This is particularly important for display frame transfers and other mutable lifecycle state.

Keep test-only `std` and executor dependencies in `dev-dependencies`; production crates remain `no_std`. A narrowly scoped Clippy allowance is acceptable only where async HAL test-double trait methods necessarily trigger it, with a concrete reason.

## Required checks

Run these from the workspace root before handoff:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p rover-drivers --features all-drivers
```

Use `cargo check --workspace` as a quick compile check while iterating. If a command fails because of an unrelated pre-existing change, report that separately and do not weaken the workspace lint policy to bypass it.
