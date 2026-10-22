//! Cleanup I/O (ADR 0005): resolving roots, scanning, re-checking and deleting to the Recycle Bin or the quarantine.
//! The core decides what may be offered and deleted ([`dyno_core::cleanup`]); this module carries it out on the real
//! file system, behind small ports so every refusal is tested in temp folders.

pub mod centre;
pub mod guard;
pub mod packs;
pub mod quarantine;
pub mod roots;
pub mod run;
pub mod scan;
#[cfg(test)]
pub mod testing;
pub mod trash;
pub mod usage;
pub mod walk;
