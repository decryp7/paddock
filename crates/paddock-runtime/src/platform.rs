use std::fmt;

/// The operating systems a runtime is pinned for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OsKind {
    Windows,
    Macos,
    Linux,
}

/// The processor architectures a runtime is pinned for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arch {
    X86_64,
    Aarch64,
}

/// A catalogue key: `<windows|macos|linux>-<x86_64|aarch64>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Platform {
    pub os: OsKind,
    pub arch: Arch,
}

impl Platform {
    /// The platform this build runs on; `None` on one no runtime is pinned for (another OS or architecture).
    pub fn current() -> Option<Platform> {
        Platform::from_consts(std::env::consts::OS, std::env::consts::ARCH)
    }

    /// The platform of Rust's `std::env::consts::{OS, ARCH}` values.
    pub fn from_consts(os: &str, arch: &str) -> Option<Platform> {
        let os = match os {
            "windows" => OsKind::Windows,
            "macos" => OsKind::Macos,
            "linux" => OsKind::Linux,
            _ => return None,
        };
        let arch = match arch {
            "x86_64" => Arch::X86_64,
            "aarch64" => Arch::Aarch64,
            _ => return None,
        };
        Some(Platform { os, arch })
    }

    /// Parses a catalogue key.
    pub fn parse(key: &str) -> Option<Platform> {
        let (os, arch) = key.split_once('-')?;
        Platform::from_consts(os, arch)
    }

    /// The catalogue key.
    pub fn key(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let os = match self.os {
            OsKind::Windows => "windows",
            OsKind::Macos => "macos",
            OsKind::Linux => "linux",
        };
        let arch = match self.arch {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
        };
        write!(f, "{os}-{arch}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_round_trips_and_others_are_refused() {
        for os in ["windows", "macos", "linux"] {
            for arch in ["x86_64", "aarch64"] {
                let key = format!("{os}-{arch}");
                assert_eq!(Platform::parse(&key).map(|p| p.key()), Some(key));
            }
        }
        for bad in ["", "linux", "plan9-x86_64", "linux-mips", "linux_x86_64"] {
            assert_eq!(Platform::parse(bad), None, "{bad}");
        }
        assert_eq!(
            Platform::current(),
            Platform::from_consts(std::env::consts::OS, std::env::consts::ARCH)
        );
    }
}
