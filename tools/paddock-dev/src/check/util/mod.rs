//! The shared helpers of the checks, ports of `conformance/src/util/*.mjs` (WP-10 batch 1). Each has a parity test against the
//! JavaScript module (`tests/it/check_util.rs`). Helpers with one consumer (`png` for C24, `keychain-graph` for C99,
//! `rust-test-items` for C35, `shared-enum-matches`, `mini-toml` for C85) are ported with their check in a later batch.

pub mod full_checkout;
pub mod md_tables;
pub mod scope;
pub mod version;
pub mod windows_bash;
pub mod workflows;
