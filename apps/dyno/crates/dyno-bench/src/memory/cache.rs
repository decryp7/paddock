//! The size of the last-level caches together, per OS (ADR 0030 section 1): Linux reads `/sys/devices/system/cpu/cpu*/cache`,
//! macOS asks `sysctl` (the performance and efficiency clusters' L2 on Apple silicon, else L3 or L2), Windows asks
//! `GetLogicalProcessorInformationEx`. `None` when the OS will not say; the test then assumes 32 MiB.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use dyno_core::bench::memory::parse_linux_cache;
use std::collections::BTreeSet;
use std::path::Path;

/// The last-level caches of a Linux machine from a sysfs tree rooted at `sys_cpu` (`/sys/devices/system/cpu`): the highest level
/// found, each cache counted once however many CPUs share it (by their `shared_cpu_list`).
pub fn linux_total(sys_cpu: &Path) -> Option<u64> {
    let mut best: Option<(u32, BTreeSet<(String, u64)>)> = None;
    for cpu in std::fs::read_dir(sys_cpu).ok()?.flatten() {
        let name = cpu.file_name().to_string_lossy().into_owned();
        if !name.starts_with("cpu") || !name[3..].chars().all(|c| c.is_ascii_digit()) || name.len() == 3 {
            continue;
        }
        let Ok(indexes) = std::fs::read_dir(cpu.path().join("cache")) else {
            continue;
        };
        for idx in indexes.flatten() {
            let read = |f: &str| std::fs::read_to_string(idx.path().join(f)).ok();
            let (Some(level), Some(size)) = (read("level"), read("size")) else {
                continue;
            };
            let (Ok(level), Some(size)) = (level.trim().parse::<u32>(), parse_linux_cache(&size)) else {
                continue;
            };
            if read("type").is_some_and(|t| t.trim() == "Instruction") {
                continue;
            }
            let shared = read("shared_cpu_list").unwrap_or_default().trim().to_string();
            match &mut best {
                Some((l, set)) if *l == level => {
                    set.insert((shared, size));
                }
                Some((l, _)) if *l > level => {}
                _ => best = Some((level, BTreeSet::from([(shared, size)]))),
            }
        }
    }
    best.map(|(_, set)| set.iter().map(|(_, s)| *s).sum()).filter(|t| *t > 0)
}

/// Parses `sysctl -n` output: one unsigned number.
pub fn parse_sysctl(out: &str) -> Option<u64> {
    out.trim().parse().ok()
}

/// macOS: the caches together from a `sysctl` reader (a port, so the tests need no machine).
pub fn macos_total(sysctl: &dyn Fn(&str) -> Option<String>) -> Option<u64> {
    let get = |k: &str| sysctl(k).and_then(|s| parse_sysctl(&s));
    let clusters: u64 = (0..4).filter_map(|i| get(&format!("hw.perflevel{i}.l2cachesize"))).sum();
    if clusters > 0 {
        return Some(clusters);
    }
    get("hw.l3cachesize").or_else(|| get("hw.l2cachesize")).filter(|c| *c > 0)
}

/// The caches together on this machine.
pub fn total() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        linux_total(Path::new("/sys/devices/system/cpu"))
    }
    #[cfg(target_os = "macos")]
    {
        macos_total(&|key| {
            // A fixed system program that only reads one kernel value and returns at once; this runner has no process helper of its own.
            #[allow(clippy::disallowed_methods)]
            let out = std::process::Command::new("/usr/sbin/sysctl").args(["-n", key]).output().ok()?;
            out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        })
    }
    #[cfg(windows)]
    {
        windows_total()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        None
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn windows_total() -> Option<u64> {
    use windows::Win32::System::SystemInformation::{
        GetLogicalProcessorInformationEx, RelationCache, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    };
    let mut len = 0_u32;
    // SAFETY: the first call only asks how many bytes are needed (a null buffer with a zero length).
    unsafe {
        // silent: the sizing call fails with ERROR_INSUFFICIENT_BUFFER by design; `len` is checked and the second call is judged
        #[allow(clippy::let_underscore_must_use, clippy::let_underscore_untyped)]
        let _ = GetLogicalProcessorInformationEx(RelationCache, None, &mut len);
    }
    if len == 0 || len > 1 << 20 {
        return None;
    }
    let mut buf = vec![0_u8; len as usize];
    // SAFETY: `buf` is `len` bytes and writable; the API fills it with variable-size records.
    unsafe {
        GetLogicalProcessorInformationEx(
            RelationCache,
            Some(buf.as_mut_ptr().cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()),
            &mut len,
        )
    }
    .ok()?;
    let (mut off, mut caches) = (0_usize, Vec::<(u8, u64)>::new());
    while off + 8 <= len as usize {
        // SAFETY: each record starts with its own size (a u32 after the u32 relationship) inside the filled buffer.
        let rec = unsafe { &*(buf.as_ptr().add(off).cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()) };
        if rec.Size == 0 || off + rec.Size as usize > len as usize {
            break;
        }
        // SAFETY: the relationship is RelationCache, so the union holds a CACHE_RELATIONSHIP.
        let c = unsafe { rec.Anonymous.Cache };
        caches.push((c.Level, u64::from(c.CacheSize)));
        off += rec.Size as usize;
    }
    let top = caches.iter().map(|(l, _)| *l).max()?;
    Some(caches.iter().filter(|(l, _)| *l == top).map(|(_, s)| *s).sum::<u64>()).filter(|t| *t > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache(root: &Path, cpu: u32, idx: u32, level: u32, kind: &str, size: &str, shared: &str) {
        let d = root.join(format!("cpu{cpu}/cache/index{idx}"));
        std::fs::create_dir_all(&d).unwrap();
        for (f, v) in [
            ("level", level.to_string()),
            ("type", kind.into()),
            ("size", size.into()),
            ("shared_cpu_list", shared.into()),
        ] {
            std::fs::write(d.join(f), v).unwrap();
        }
    }

    #[test]
    fn linux_counts_each_shared_last_level_cache_once() {
        let d = tempfile::tempdir().unwrap();
        // Two sockets with an L3 of 32M shared by CPUs 0-1 and 2-3; private L1/L2 below.
        for cpu in 0..4_u32 {
            let shared = if cpu < 2 { "0-1" } else { "2-3" };
            cache(d.path(), cpu, 0, 1, "Data", "32K", &cpu.to_string());
            cache(d.path(), cpu, 1, 1, "Instruction", "32K", &cpu.to_string());
            cache(d.path(), cpu, 2, 2, "Unified", "1024K", &cpu.to_string());
            cache(d.path(), cpu, 3, 3, "Unified", "32M", shared);
        }
        assert_eq!(linux_total(d.path()), Some(64 * 1024 * 1024));
    }

    #[test]
    fn linux_without_a_cache_tree_says_nothing() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(linux_total(d.path()), None);
        assert_eq!(linux_total(&d.path().join("missing")), None);
    }

    #[test]
    fn macos_adds_the_clusters_or_falls_back_to_l3_then_l2() {
        let apple = |k: &str| match k {
            "hw.perflevel0.l2cachesize" => Some("16777216\n".to_string()),
            "hw.perflevel1.l2cachesize" => Some("4194304\n".to_string()),
            _ => None,
        };
        assert_eq!(macos_total(&apple), Some(20 * 1024 * 1024));
        let intel = |k: &str| (k == "hw.l3cachesize").then(|| "8388608".to_string());
        assert_eq!(macos_total(&intel), Some(8 * 1024 * 1024));
        let old = |k: &str| (k == "hw.l2cachesize").then(|| "262144".to_string());
        assert_eq!(macos_total(&old), Some(262_144));
        assert_eq!(macos_total(&|_| None), None);
        assert_eq!(macos_total(&|_| Some("garbage".into())), None);
    }
}
