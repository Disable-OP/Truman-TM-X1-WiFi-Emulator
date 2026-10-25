//! Truman TM X1 WiFi emulator.
//!
//! Emulates the Sunplus SPHE15xx-based Truman TM X1 WiFi satellite receiver
//! from its internal 4 MiB SPI flash image. See `docs/firmware-analysis.md`
//! for the verified firmware facts this design is based on.
//!
//! Module boundaries:
//! - [`firmware`] — image identification, validation, loading
//! - [`memory`]   — virtual→physical translation, address space, regions
//! - [`devices`]  — memory-mapped device model trait and implementations
//! - [`cpu`]      — CPU backend abstraction + MIPS32r2 LE interpreter
//! - [`machine`]  — Truman TM X1 machine composition and run loop
//! - [`boot`]     — reset state and boot diagnostics
//! - [`inflate`]  — RFC 1951 DEFLATE decoder (hardware engine's algorithm)

pub mod boot;
pub mod cpu;
pub mod devices;
pub mod firmware;
pub mod inflate;
pub mod machine;
pub mod memory;
pub mod solve;

/// Emulator crate version, from Cargo.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Name of the emulated platform.
pub const TARGET_NAME: &str = "Truman TM X1 WiFi (Sunplus SPHE15xx, MIPS32r2 LE)";
