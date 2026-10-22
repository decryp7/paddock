//! llama.cpp runtimes (ADR 0004): the pinned catalog of official prebuilt `llama-server` builds, build ids and the
//! automatic order, which archive entries are kept when unpacking, and the devices a build reports. Ported from
//! IMT's `imt_ai::runtimes` and `imt-ai-worker::devices` (IMT ADR 0009).

pub mod builds;
pub mod capabilities;
pub mod catalog;
pub mod pins;

pub use builds::{BuildChoice, BuildId};
pub use catalog::{current_platform, suggest, RuntimeAsset, RuntimeBuild, RuntimeCatalog};

use serde::{Deserialize, Serialize};

/// Whether a file name is safe to create in a build folder: flat, no dot-files, no `..`.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

/// Whether an archive entry is one Pitwall keeps: `llama-server`, shared libraries and licences (no other tools).
pub fn is_expected(name: &str) -> bool {
    if !safe_name(name) {
        return false;
    }
    if name == "llama-server" || name == "llama-server.exe" || name.starts_with("LICENSE") {
        return true;
    }
    let lib = name.ends_with(".dll") || name.ends_with(".dylib") || name.ends_with(".so") || name.contains(".so.");
    let other_tool = name.contains("-impl.") && !name.trim_start_matches("lib").starts_with("llama-server-impl.");
    lib && !other_tool
}

/// The flat file name of an archive path: `name` or `dir/name` (one safe folder level); `None` for anything that
/// could escape the destination (absolute paths, backslashes, drive letters, deeper paths).
pub fn flat_name(path: &str) -> Option<&str> {
    let path = path.trim_start_matches("./");
    if path.starts_with('/') || path.starts_with('\\') || path.contains('\\') || path.contains(':') {
        return None;
    }
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [name] => Some(name),
        [dir, name] if safe_name(dir) => Some(name),
        _ => None,
    }
}

/// A device a build can use, from `llama-server --list-devices`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// `Vulkan0`, `CUDA0`, `MTL0`.
    pub name: String,
    /// The adapter's description.
    pub description: String,
    /// Total memory in MiB.
    pub total_mib: u64,
    /// Free memory in MiB.
    pub free_mib: u64,
}

impl Device {
    /// The backend family: `CUDA`, `Vulkan`, `Metal`, …
    pub fn backend(&self) -> &str {
        let head = self.name.trim_end_matches(|c: char| c.is_ascii_digit());
        match head {
            "MTL" => "Metal",
            other => other,
        }
    }

    /// A real GPU (software renderers such as llvmpipe don't count).
    pub fn is_gpu(&self) -> bool {
        crate::hardware::is_real_gpu(&self.description)
    }

    /// For the UI: `CUDA0: NVIDIA GeForce RTX 4070 (11034 of 12281 MiB free)`.
    pub fn label(&self) -> String {
        format!(
            "{}: {} ({} of {} MiB free)",
            self.name, self.description, self.free_mib, self.total_mib
        )
    }
}

/// Parses the output of `--list-devices` (tolerant: lines that don't match are skipped; a CPU build prints `(none)`).
pub fn parse_list_devices(text: &str) -> Vec<Device> {
    text.lines().filter_map(parse_device_line).collect()
}

fn parse_device_line(line: &str) -> Option<Device> {
    let line = line.trim();
    let (name, rest) = line.split_once(": ")?;
    if name.is_empty() || name.contains(char::is_whitespace) {
        return None;
    }
    let rest = rest.strip_suffix(')')?;
    let open = rest.rfind('(')?;
    let (description, mem) = (rest[..open].trim(), &rest[open + 1..]);
    let (total, free) = mem.split_once(',')?;
    let mib = |s: &str| {
        s.trim()
            .strip_suffix("free")
            .unwrap_or(s)
            .trim()
            .strip_suffix("MiB")?
            .trim()
            .parse::<u64>()
            .ok()
    };
    Some(Device {
        name: name.to_string(),
        description: description.to_string(),
        total_mib: mib(total)?,
        free_mib: mib(free)?,
    })
}

/// The GPU with the most free memory.
pub fn best_gpu(devices: &[Device]) -> Option<&Device> {
    devices.iter().filter(|d| d.is_gpu()).max_by_key(|d| d.free_mib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_entries() {
        for ok in [
            "llama-server",
            "llama-server.exe",
            "ggml-vulkan.dll",
            "libggml.so.1",
            "libllama.dylib",
            "LICENSE-curl",
            "libllama-server-impl.so",
        ] {
            assert!(is_expected(ok), "{ok}");
        }
        for no in [
            "llama-cli.exe",
            "../x.dll",
            ".hidden.dll",
            "readme.md",
            "libllama-bench-impl.so",
            "a b.dll",
            "",
        ] {
            assert!(!is_expected(no), "{no}");
        }
        assert_eq!(flat_name("./build/bin.dll"), Some("bin.dll"));
        assert_eq!(flat_name("x.dll"), Some("x.dll"));
        for bad in ["/etc/x", "\\x", "a\\b", "c:x", "a/b/c", "../x/y"] {
            assert_eq!(flat_name(bad), None, "{bad}");
        }
        assert!(!safe_name(&"a".repeat(201)));
    }

    #[test]
    fn devices() {
        let out = "ggml_vulkan: Found 2 Vulkan devices:\nAvailable devices:\n  Vulkan0: AMD Radeon (RADV NAVI32) (16368 MiB, 15912 MiB free)\n  Vulkan1: llvmpipe (LLVM 15.0.7, 256 bits) (32000 MiB, 32000 MiB free)\n  CUDA0: NVIDIA GeForce RTX 4070 (12281 MiB, 11034 MiB free)\n  MTL0: Apple M2 (21845 MiB, 21845 MiB free)\n  junk: line (x MiB, y MiB free)\n  bad name: x (1 MiB, 1 MiB free)\n  X0: no memory\n  X1: (1 MiB)\n";
        let d = parse_list_devices(out);
        assert_eq!(d.len(), 4);
        assert_eq!(d[0].description, "AMD Radeon (RADV NAVI32)");
        assert_eq!((d[0].total_mib, d[0].free_mib), (16368, 15912));
        assert_eq!(d[0].backend(), "Vulkan");
        assert!(!d[1].is_gpu());
        assert_eq!(d[2].backend(), "CUDA");
        assert_eq!(d[3].backend(), "Metal");
        assert_eq!(d[2].label(), "CUDA0: NVIDIA GeForce RTX 4070 (11034 of 12281 MiB free)");
        assert_eq!(best_gpu(&d).map(|g| g.name.as_str()), Some("MTL0"));
        assert!(parse_list_devices("Available devices:\n  (none)\n").is_empty());
        assert!(best_gpu(&[]).is_none());
    }
}
