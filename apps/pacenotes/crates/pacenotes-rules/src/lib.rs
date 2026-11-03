#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! Pacenotes' review rules (package C1, on the joint rules model `docs/research/2026-10-joint-rules-model.md`).
//!
//! - [`shared_copy`]: the half of the model that `paddock-rules` will own (rule ids, fix sentences, finding fingerprints, waivers
//!   and suppressions, baselines, reports, SARIF and rdjson). **Temporary:** it moves to the shared crate in C3a and a test refuses
//!   to let both exist.
//! - [`pattern`]: structural, ast-grep-style patterns over tree-sitter, deterministic and model-free.
//! - [`rule_test`]: a rule's own good and bad examples are its test.

pub mod pattern;
pub mod rule_test;
pub mod shared_copy;
