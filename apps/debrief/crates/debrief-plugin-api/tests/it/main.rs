//! The integration tests of this crate, one binary. Run one module with `cargo test --test it <name>::`.

#[cfg(unix)]
mod local_zone;
mod timestamp_pins;
