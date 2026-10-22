//! Operating systems and CPU architectures Pitcrew manages runners on.

use serde::{Deserialize, Serialize};

/// An operating system. Linux is planned (see README); the release logic already knows its asset names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    /// Windows.
    Windows,
    /// macOS.
    Macos,
    /// Linux.
    Linux,
}

/// A CPU architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    /// x86-64.
    Amd64,
    /// 64-bit ARM.
    Arm64,
}

/// An OS and architecture pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Platform {
    /// The operating system.
    pub os: Os,
    /// The architecture.
    pub arch: Arch,
}

impl Platform {
    /// The platform this binary was built for, or `None` on one Pitcrew does not support.
    pub fn current() -> Option<Platform> {
        let os = match std::env::consts::OS {
            "windows" => Os::Windows,
            "macos" => Os::Macos,
            "linux" => Os::Linux,
            _ => return None,
        };
        let arch = match std::env::consts::ARCH {
            "x86_64" => Arch::Amd64,
            "aarch64" => Arch::Arm64,
            _ => return None,
        };
        Some(Platform { os, arch })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_platform_is_supported() {
        // CI runs on Windows, macOS and Linux, x86-64 or ARM64.
        assert!(Platform::current().is_some());
    }
}
