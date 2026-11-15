//! `dev-launch`: the launcher behind `./dev` (shell-to-rust WP-1, ADR 0022 amendment 2026-11-15).
//!
//! Standard library only. [`hash`] is the one copy of the input hash that `paddock-dev` (`dev self hash`) also calls;
//! [`launch`] is what `./dev` does: find the repository, hash the tool's inputs, take the tool from the cache, a download or a
//! local build, and run it.

pub mod cache;
pub mod download;
pub mod hash;
pub mod launch;
pub mod proc;
pub mod sha256;
pub mod toolchain;
