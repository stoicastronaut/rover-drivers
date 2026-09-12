#![no_std]

//! Feature-gated catalog of Rover embedded drivers.
//!
//! This crate has no default features. Enable a driver in `Cargo.toml` and
//! import it from this crate. For example:
//!
//! ```toml
//! rover-drivers = { version = "0.1", default-features = false, features = ["bmp388"] }
//! ```

#[cfg(feature = "bmp388")]
pub use bmp388;

#[cfg(feature = "mpu6050")]
pub use mpu6050;
