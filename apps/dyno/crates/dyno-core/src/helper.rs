//! The operating systems the privileged helper serves (ADR 0006). The helper makes no change to the machine: it answers
//! read-only questions and installs or removes itself.

use serde::{Deserialize, Serialize};

/// The operating systems the helper runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HelperHost {
    /// Windows.
    Windows,
    /// macOS.
    Macos,
    /// Linux.
    Linux,
}
