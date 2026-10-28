//! The operating systems Dyno has OS-specific knowledge of (which startup entries belong to the OS, which programs
//! are unusual).

use serde::{Deserialize, Serialize};

/// An operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Os {
    /// Windows.
    Windows,
    /// macOS.
    Macos,
    /// Linux.
    Linux,
}
