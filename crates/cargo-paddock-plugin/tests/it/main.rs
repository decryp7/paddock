//! The commands end to end: `new` → `build` (real cargo, on a tiny project) → `pack` → `verify` (good, tampered, wrong key) →
//! `test` against the fake host (on the test kit's fixture plugin). Temp folders only; every plugin process ends with its fake
//! host.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod flow;
mod support;
mod test_command;
