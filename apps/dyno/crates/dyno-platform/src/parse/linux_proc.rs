//! Parsers for what Linux exposes under `/proc` and `/sys`: pressure, memory, disk counters, process status, maps,
//! locks, CPU lists and mount filtering. Pure functions over text, so they are tested on every OS; none of them
//! panics on any input.

use dyno_core::metrics::MemoryPressure;

/// Most bytes of a `/proc` or `/sys` file the readers hand to these parsers.
pub const MAX_PROC_TEXT: usize = 2 * 1024 * 1024;

/// PSI "some" `avg10` of `/proc/pressure/memory`: the percentage of the last 10 s in which at least one task
/// stalled waiting for memory.
pub fn psi_some_avg10(text: &str) -> Option<f32> {
    let line = text.lines().find(|l| l.starts_with("some "))?;
    let v = line.split_whitespace().find_map(|f| f.strip_prefix("avg10="))?;
    v.parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0)
}

/// PSI `some avg10` at or above this (percent) is "Elevated": the system is regularly stalling on memory.
pub const PSI_ELEVATED: f32 = 5.0;
/// PSI `some avg10` at or above this (percent) is "Critical": tasks stall for a fifth of the time or more.
pub const PSI_CRITICAL: f32 = 20.0;
/// Available memory below this share of the total is at least "Elevated".
pub const AVAILABLE_ELEVATED_PCT: u64 = 10;
/// Available memory below this share of the total is "Critical".
pub const AVAILABLE_CRITICAL_PCT: u64 = 4;

/// The memory-pressure level: the worse of what PSI says (`some avg10`, see [`PSI_ELEVATED`] and [`PSI_CRITICAL`])
/// and what is left of the memory (see [`AVAILABLE_ELEVATED_PCT`] and [`AVAILABLE_CRITICAL_PCT`]).
pub fn pressure_level(psi_avg10: f32, available: u64, total: u64) -> MemoryPressure {
    let by_psi = if psi_avg10 >= PSI_CRITICAL {
        MemoryPressure::Critical
    } else if psi_avg10 >= PSI_ELEVATED {
        MemoryPressure::Elevated
    } else {
        MemoryPressure::Normal
    };
    let by_free = if total == 0 {
        MemoryPressure::Normal
    } else if available.saturating_mul(100) < total.saturating_mul(AVAILABLE_CRITICAL_PCT) {
        MemoryPressure::Critical
    } else if available.saturating_mul(100) < total.saturating_mul(AVAILABLE_ELEVATED_PCT) {
        MemoryPressure::Elevated
    } else {
        MemoryPressure::Normal
    };
    by_psi.max(by_free)
}

/// The memory figures of `/proc/meminfo`, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemInfo {
    /// `MemTotal`.
    pub total: u64,
    /// `MemAvailable` (falls back to free + cached on kernels before 3.14 that lack it).
    pub available: u64,
    /// `SwapTotal`.
    pub swap_total: u64,
    /// `SwapFree`.
    pub swap_free: u64,
}

fn kib_field(text: &str, key: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with(key) && l[key.len()..].starts_with(':'))?;
    let n: u64 = line[key.len() + 1..].split_whitespace().next()?.parse().ok()?;
    Some(n.saturating_mul(1024))
}

/// Parses `/proc/meminfo`; `None` without a `MemTotal`.
pub fn meminfo(text: &str) -> Option<MemInfo> {
    let total = kib_field(text, "MemTotal")?;
    let available = kib_field(text, "MemAvailable")
        .or_else(|| Some(kib_field(text, "MemFree")?.saturating_add(kib_field(text, "Cached").unwrap_or(0))))
        .unwrap_or(0)
        .min(total);
    Some(MemInfo {
        total,
        available,
        swap_total: kib_field(text, "SwapTotal").unwrap_or(0),
        swap_free: kib_field(text, "SwapFree").unwrap_or(0),
    })
}

/// A memory limit of a cgroup (`memory.max`, or v1 `memory.limit_in_bytes`): `None` for "max" or an absurd v1
/// "unlimited" value (a page-rounded `i64::MAX`).
pub fn cgroup_limit(text: &str) -> Option<u64> {
    let v: u64 = text.trim().parse().ok()?;
    (v > 0 && v < (1 << 60)).then_some(v)
}

/// A value from a `key value` file such as `memory.stat`.
pub fn stat_value(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once(' ')?;
        (k == key).then(|| v.trim().parse().ok()).flatten()
    })
}

/// One disk's counters from `/proc/diskstats`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskStat {
    /// Device name (`sda`, `nvme0n1`).
    pub name: String,
    /// Milliseconds the device had I/O in flight (`io_ticks`).
    pub io_ticks_ms: u64,
    /// Weighted milliseconds spent doing I/O: integrating the queue length over time.
    pub weighted_ms: u64,
}

/// Parses `/proc/diskstats`, keeping whole physical disks only (see [`is_whole_disk`]).
pub fn diskstats(text: &str) -> Vec<DiskStat> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            // major minor name + at least 11 counters; io_ticks is the 13th value after the name, weighted the 14th.
            let name = f.get(2)?;
            if !is_whole_disk(name) {
                return None;
            }
            Some(DiskStat {
                name: (*name).to_string(),
                io_ticks_ms: f.get(12)?.parse().ok()?,
                weighted_ms: f.get(13)?.parse().ok()?,
            })
        })
        .collect()
}

/// Whether a block device name is a whole physical disk: `sda`, `vda`, `xvda`, `hda`, `nvme0n1`, `mmcblk0`; not
/// partitions (`sda1`, `nvme0n1p2`), nor `loop`, `ram`, `zram`, device-mapper or md devices (which would count the
/// same I/O twice).
pub fn is_whole_disk(name: &str) -> bool {
    for prefix in ["sd", "vd", "xvd", "hd"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_lowercase());
        }
    }
    if let Some(rest) = name.strip_prefix("nvme") {
        let Some((ctl, ns)) = rest.split_once('n') else { return false };
        return !ctl.is_empty() && ctl.bytes().all(|b| b.is_ascii_digit()) && !ns.is_empty() && ns.bytes().all(|b| b.is_ascii_digit());
    }
    if let Some(rest) = name.strip_prefix("mmcblk") {
        return !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit());
    }
    false
}

/// Disk load over an interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiskLoad {
    /// Average number of requests waiting or in service, summed over the disks.
    pub queue: f32,
    /// The busiest disk's share of the interval with I/O in flight, 0–100.
    pub busy: f32,
}

/// The queue length and busy percentage between two readings `elapsed_ms` apart. `None` when no time passed, no
/// disk is in both readings, or the counters went backwards (reset).
pub fn disk_load(prev: &[DiskStat], now: &[DiskStat], elapsed_ms: u64) -> Option<DiskLoad> {
    if elapsed_ms == 0 {
        return None;
    }
    let (mut queue, mut busy, mut seen) = (0.0f64, 0.0f64, false);
    for n in now {
        let Some(p) = prev.iter().find(|p| p.name == n.name) else {
            continue;
        };
        let (Some(ticks), Some(weighted)) = (n.io_ticks_ms.checked_sub(p.io_ticks_ms), n.weighted_ms.checked_sub(p.weighted_ms)) else {
            continue;
        };
        seen = true;
        queue += weighted as f64 / elapsed_ms as f64;
        busy = busy.max((ticks as f64 * 100.0 / elapsed_ms as f64).min(100.0));
    }
    seen.then_some(DiskLoad {
        queue: queue as f32,
        busy: busy as f32,
    })
}

/// The `key: value` field of `/proc/<pid>/status` (value trimmed).
pub fn status_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k == key).then(|| v.trim())
    })
}

/// The nice value of `/proc/<pid>/stat` (field 19). The command name may contain spaces and parentheses, so fields
/// are counted after the last `)`.
pub fn stat_nice(text: &str) -> Option<i32> {
    let after = &text[text.rfind(')')? + 1..];
    // After the name: state is field 3, nice is field 19.
    after.split_whitespace().nth(16)?.parse().ok()
}

/// The CPUs a process may run on, from `Cpus_allowed` of `/proc/<pid>/status`: comma-separated 32-bit hex groups,
/// most significant first. Cores beyond 63 do not fit and are dropped.
pub fn cpus_allowed(text: &str) -> Option<u64> {
    let v = status_field(text, "Cpus_allowed")?;
    let mut mask = 0u64;
    for g in v.split(',') {
        let word = u32::from_str_radix(g.trim(), 16).ok()?;
        mask = (mask << 32) | u64::from(word);
    }
    Some(mask)
}

/// A CPU list such as `0-3,6,8-9` (the format of `/sys/devices/system/cpu/online`) as a mask; cores beyond 63 are
/// ignored and malformed pieces skipped.
pub fn cpu_list_mask(text: &str) -> u64 {
    let mut mask = 0u64;
    for piece in text.trim().split(',') {
        let (a, b) = match piece.trim().split_once('-') {
            Some((a, b)) => (a.parse::<u32>(), b.parse::<u32>()),
            None => (piece.trim().parse::<u32>(), piece.trim().parse::<u32>()),
        };
        if let (Ok(a), Ok(b)) = (a, b) {
            for c in a..=b.min(63) {
                mask |= 1u64 << c;
            }
        }
    }
    mask
}

/// The distinct file-backed mappings of `/proc/<pid>/maps` (shared libraries and the program), in first-seen order.
pub fn maps_files(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        // "address perms offset dev inode   path": the path may hold spaces, so skip five fields and keep the rest.
        let mut rest = line;
        for _ in 0..5 {
            rest = rest.trim_start();
            rest = rest.split_once(char::is_whitespace).map_or("", |(_, r)| r);
        }
        let path = rest.trim();
        if path.starts_with('/') && seen.insert(path.to_string()) {
            out.push(path.to_string());
        }
    }
    out
}

/// One lock of `/proc/locks`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockRecord {
    /// `POSIX`, `FLOCK`, `OFDLCK`, `LEASE`.
    pub kind: String,
    /// The process holding it (0 for some leases and OFD locks held through shared descriptions).
    pub pid: u32,
    /// Device of the locked file.
    pub major: u32,
    /// Device of the locked file.
    pub minor: u32,
    /// Inode of the locked file.
    pub inode: u64,
}

/// Parses `/proc/locks`.
pub fn locks(text: &str) -> Vec<LockRecord> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.contains(&"->") {
                // A blocked waiter, not a holder.
                return None;
            }
            // "1: POSIX  ADVISORY  WRITE 4242 08:01:131075 0 EOF"; blocked waiters start with "->".
            let at = f.iter().position(|w| w.contains(':') && w.matches(':').count() == 2)?;
            let pid: u32 = f.get(at.checked_sub(1)?)?.parse().ok()?;
            let mut dev = f[at].split(':');
            let major = u32::from_str_radix(dev.next()?, 16).ok()?;
            let minor = u32::from_str_radix(dev.next()?, 16).ok()?;
            let inode = dev.next()?.parse().ok()?;
            let kind = f.iter().find(|w| matches!(**w, "POSIX" | "FLOCK" | "OFDLCK" | "LEASE"))?;
            Some(LockRecord {
                kind: (*kind).to_string(),
                pid,
                major,
                minor,
                inode,
            })
        })
        .collect()
}

/// The (major, minor) of a `st_dev` as Linux encodes it.
pub fn dev_major_minor(dev: u64) -> (u32, u32) {
    let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff);
    let minor = (dev & 0xff) | ((dev >> 12) & !0xff);
    (major as u32, minor as u32)
}

/// The inode of a `socket:[1234]` descriptor link.
pub fn socket_inode(link: &str) -> Option<u64> {
    link.strip_prefix("socket:[")?.strip_suffix(']')?.parse().ok()
}

/// `PRETTY_NAME` of an os-release file (quotes removed).
pub fn os_pretty_name(text: &str) -> Option<String> {
    let v = text.lines().find_map(|l| l.trim().strip_prefix("PRETTY_NAME="))?;
    let v = v.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// The systemd service a process belongs to, from `/proc/<pid>/cgroup` (`0::/user.slice/user-1000.slice/user@1000.service/app.slice/foo.service`).
/// Only user-level services count (`user@N.service/…`); scopes and the system's own services return `None`.
pub fn user_service(text: &str) -> Option<String> {
    let path = text.lines().find_map(|l| l.strip_prefix("0::"))?;
    let after = path.split_once(".service/")?.1;
    // The leaf is a service when it ends in ".service"; nested slices/scopes are not startup items.
    let leaf = after.rsplit('/').next()?;
    (path.contains("/user@") && leaf.ends_with(".service") && !leaf.starts_with('-')).then(|| leaf.to_string())
}

/// Whether a mounted volume is plumbing the user should not see as a disk: virtual and in-memory filesystems,
/// read-only package images (squashfs, snap loop mounts), and the files a container mounts into itself.
pub fn is_hidden_mount(device: &str, mount: &str) -> bool {
    const FS_NAMES: [&str; 12] = [
        "tmpfs",
        "overlay",
        "squashfs",
        "devtmpfs",
        "shm",
        "proc",
        "sysfs",
        "cgroup",
        "cgroup2",
        "efivarfs",
        "ramfs",
        "fuse.portal",
    ];
    const PREFIXES: [&str; 9] = [
        "/proc",
        "/sys",
        "/dev",
        "/run",
        "/snap",
        "/var/snap",
        "/var/lib/docker",
        "/var/lib/containers",
        "/var/lib/kubelet",
    ];
    const CONTAINER_FILES: [&str; 4] = ["/etc/hosts", "/etc/hostname", "/etc/resolv.conf", "/etc/machine-id"];
    let under = |p: &str| mount == p || mount.strip_prefix(p).is_some_and(|r| r.starts_with('/'));
    FS_NAMES.contains(&device) || device.starts_with("/dev/loop") || PREFIXES.iter().any(|p| under(p)) || CONTAINER_FILES.contains(&mount)
}

/// Whether a network interface is virtual plumbing whose traffic would count twice or is not the user's network:
/// loopback and container/VM bridges and their veth ends.
pub fn is_virtual_interface(name: &str) -> bool {
    name == "lo"
        || [
            "docker", "veth", "virbr", "br-", "cni", "flannel", "cali", "vnet", "podman", "lxcbr", "lxdbr",
        ]
        .iter()
        .any(|p| name.starts_with(p))
}

/// The average of the `scaling_cur_freq` values (kHz) as MHz; `None` when none parses.
pub fn mean_khz_as_mhz(values: &[String]) -> Option<u32> {
    let khz: Vec<u64> = values.iter().filter_map(|v| v.trim().parse().ok()).collect();
    if khz.is_empty() {
        return None;
    }
    u32::try_from(khz.iter().sum::<u64>() / khz.len() as u64 / 1000).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PSI: &str = "some avg10=12.50 avg60=3.00 avg300=1.00 total=123456\nfull avg10=1.00 avg60=0.00 avg300=0.00 total=789\n";

    #[test]
    fn psi_and_pressure() {
        assert_eq!(psi_some_avg10(PSI), Some(12.5));
        assert_eq!(psi_some_avg10("full avg10=1.00"), None);
        assert_eq!(psi_some_avg10("some avg10=x"), None);
        assert_eq!(psi_some_avg10("some avg10=-3"), None);
        let gib = 1u64 << 30;
        assert_eq!(pressure_level(0.0, 8 * gib, 16 * gib), MemoryPressure::Normal);
        assert_eq!(pressure_level(6.0, 8 * gib, 16 * gib), MemoryPressure::Elevated);
        assert_eq!(pressure_level(30.0, 8 * gib, 16 * gib), MemoryPressure::Critical);
        assert_eq!(pressure_level(0.0, gib, 16 * gib), MemoryPressure::Elevated);
        assert_eq!(pressure_level(0.0, gib / 8, 16 * gib), MemoryPressure::Critical);
        assert_eq!(pressure_level(0.0, 0, 0), MemoryPressure::Normal);
        assert_eq!(pressure_level(f32::NAN, 3, 100), MemoryPressure::Critical);
    }

    #[test]
    fn meminfo_in_bytes() {
        let t = "MemTotal:       16384000 kB\nMemFree:         1000000 kB\nMemAvailable:    8192000 kB\nSwapTotal:       2048000 kB\nSwapFree:        1024000 kB\n";
        let m = meminfo(t).unwrap();
        assert_eq!(m.total, 16_384_000 * 1024);
        assert_eq!(m.available, 8_192_000 * 1024);
        assert_eq!((m.swap_total, m.swap_free), (2_048_000 * 1024, 1_024_000 * 1024));
        let old = "MemTotal: 1000 kB\nMemFree: 100 kB\nCached: 200 kB\n";
        assert_eq!(meminfo(old).unwrap().available, 300 * 1024);
        assert_eq!(meminfo("MemAvailable: 5 kB").map(|m| m.total), None);
        assert_eq!(meminfo("MemTotal: 10 kB\nMemAvailable: 99 kB").unwrap().available, 10 * 1024);
    }

    #[test]
    fn cgroup_values() {
        assert_eq!(cgroup_limit("max\n"), None);
        assert_eq!(cgroup_limit("536870912\n"), Some(536_870_912));
        assert_eq!(cgroup_limit("9223372036854771712"), None);
        assert_eq!(cgroup_limit("0"), None);
        assert_eq!(stat_value("anon 10\ninactive_file 4096\n", "inactive_file"), Some(4096));
        assert_eq!(stat_value("anon 10\n", "file"), None);
    }

    const DISKSTATS: &str = "\
   7       0 loop0 100 0 800 10 0 0 0 0 0 20 10 0 0 0 0 0 0
   8       0 sda 5000 10 400000 3000 2000 5 160000 1000 0 4000 4500 0 0 0 0 0 0
   8       1 sda1 4000 10 300000 2500 1500 5 150000 900 0 3500 3400 0 0 0 0 0 0
 259       0 nvme0n1 9000 0 700000 2000 8000 0 900000 3000 0 6000 7000 0 0 0 0 0 0
 259       1 nvme0n1p1 100 0 800 20 0 0 0 0 0 20 20 0 0 0 0 0 0
 253       0 dm-0 100 0 800 20 0 0 0 0 0 20 20 0 0 0 0 0 0
";

    #[test]
    fn whole_disks_only() {
        let d = diskstats(DISKSTATS);
        let names: Vec<&str> = d.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["sda", "nvme0n1"]);
        assert_eq!((d[0].io_ticks_ms, d[0].weighted_ms), (4000, 4500));
        for good in ["sda", "vdb", "xvda", "hda", "nvme0n1", "nvme12n3", "mmcblk0"] {
            assert!(is_whole_disk(good), "{good}");
        }
        for bad in [
            "sda1",
            "vdb2",
            "nvme0n1p1",
            "nvme0",
            "nvmen1",
            "mmcblk0p1",
            "loop0",
            "ram0",
            "zram0",
            "dm-0",
            "md0",
            "sd",
            "",
        ] {
            assert!(!is_whole_disk(bad), "{bad}");
        }
        assert!(diskstats("garbage\n8 0 sda 1 2\n").is_empty());
    }

    #[test]
    fn disk_load_from_deltas() {
        let prev = vec![
            DiskStat {
                name: "sda".into(),
                io_ticks_ms: 1000,
                weighted_ms: 1000,
            },
            DiskStat {
                name: "sdb".into(),
                io_ticks_ms: 0,
                weighted_ms: 0,
            },
        ];
        let now = vec![
            DiskStat {
                name: "sda".into(),
                io_ticks_ms: 1500,
                weighted_ms: 2500,
            },
            DiskStat {
                name: "sdb".into(),
                io_ticks_ms: 100,
                weighted_ms: 100,
            },
            DiskStat {
                name: "sdc".into(),
                io_ticks_ms: 5,
                weighted_ms: 5,
            },
        ];
        let l = disk_load(&prev, &now, 1000).unwrap();
        assert!((l.busy - 50.0).abs() < 0.01 && (l.queue - 1.6).abs() < 0.01, "{l:?}");
        assert_eq!(disk_load(&prev, &now, 0), None);
        assert_eq!(disk_load(&[], &now, 1000), None);
        // A counter reset is skipped rather than underflowing.
        assert_eq!(disk_load(&now, &prev, 1000), None);
        let over = vec![DiskStat {
            name: "sda".into(),
            io_ticks_ms: 9000,
            weighted_ms: 9000,
        }];
        assert_eq!(disk_load(&prev[..1], &over, 1000).map(|l| l.busy), Some(100.0));
    }

    #[test]
    fn status_and_stat() {
        let status = "Name:\tfirefox\nThreads:\t42\nCpus_allowed:\tffffffff,0000ff0f\nUid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(status_field(status, "Threads"), Some("42"));
        assert_eq!(status_field(status, "Nope"), None);
        assert_eq!(cpus_allowed(status), Some(0xffff_ffff_0000_ff0f));
        assert_eq!(cpus_allowed("Cpus_allowed:\t0000000f\n"), Some(0xf));
        assert_eq!(cpus_allowed("Cpus_allowed:\tzz\n"), None);
        assert_eq!(cpus_allowed(""), None);
        let stat = "4242 (my (odd) app) S 1 4242 4242 0 -1 4194304 100 0 0 0 5 3 0 0 20 10 4 0 12345 1000000 500 18446744073709551615";
        assert_eq!(stat_nice(stat), Some(10));
        assert_eq!(stat_nice("no parens"), None);
        assert_eq!(stat_nice("1 (x) S 1"), None);
    }

    #[test]
    fn cpu_lists() {
        assert_eq!(cpu_list_mask("0-3\n"), 0xf);
        assert_eq!(cpu_list_mask("0,2,4-5"), 0b110101);
        assert_eq!(cpu_list_mask("0-200"), u64::MAX);
        assert_eq!(cpu_list_mask("x,1,3-"), 0b10);
        assert_eq!(cpu_list_mask(""), 0);
    }

    #[test]
    fn maps_files_are_distinct_and_file_backed() {
        let t = "\
55d0c8a00000-55d0c8a02000 r--p 00000000 08:01 131075 /usr/bin/app
55d0c8a02000-55d0c8a05000 r-xp 00002000 08:01 131075 /usr/bin/app
7f1c00000000-7f1c00021000 rw-p 00000000 00:00 0
7f1c10000000-7f1c10100000 r-xp 00000000 08:01 262200 /usr/lib/aarch64-linux-gnu/libc.so.6
7f1c20000000-7f1c20001000 r--p 00000000 08:01 262300 /home/me/My Libs/libx.so (deleted)
7ffd5e1f0000-7ffd5e211000 rw-p 00000000 00:00 0 [stack]
";
        assert_eq!(
            maps_files(t),
            [
                "/usr/bin/app",
                "/usr/lib/aarch64-linux-gnu/libc.so.6",
                "/home/me/My Libs/libx.so (deleted)"
            ]
        );
        assert!(maps_files("\u{0}\u{1}").is_empty());
    }

    #[test]
    fn locks_and_devices() {
        let t = "1: POSIX  ADVISORY  WRITE 4242 08:01:131075 0 EOF\n2: FLOCK  ADVISORY  WRITE 77 00:2a:12 0 EOF\n2: -> FLOCK  ADVISORY  WRITE 88 00:2a:12 0 EOF\nbad\n";
        let l = locks(t);
        assert_eq!(l.len(), 2);
        assert_eq!(
            (l[0].kind.as_str(), l[0].pid, l[0].major, l[0].minor, l[0].inode),
            ("POSIX", 4242, 8, 1, 131_075)
        );
        assert_eq!((l[1].pid, l[1].major, l[1].minor, l[1].inode), (77, 0, 0x2a, 12));
        assert_eq!(dev_major_minor(0x801), (8, 1));
        assert_eq!(dev_major_minor((259 << 8) | 3), (259, 3));
        assert_eq!(socket_inode("socket:[1234]"), Some(1234));
        assert_eq!(socket_inode("/tmp/x"), None);
        assert_eq!(socket_inode("socket:[x]"), None);
    }

    #[test]
    fn os_release_and_cgroups() {
        assert_eq!(
            os_pretty_name("NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\n"),
            Some("Ubuntu 24.04.1 LTS".into())
        );
        assert_eq!(os_pretty_name("PRETTY_NAME=Arch\n"), Some("Arch".into()));
        assert_eq!(os_pretty_name("PRETTY_NAME=\"\"\n"), None);
        assert_eq!(os_pretty_name(""), None);
        let user = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/syncthing.service\n";
        assert_eq!(user_service(user), Some("syncthing.service".into()));
        assert_eq!(user_service("0::/system.slice/cron.service\n"), None);
        assert_eq!(user_service("0::/user.slice/user-1000.slice/session-3.scope\n"), None);
        assert_eq!(
            user_service("0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-firefox-1234.scope\n"),
            None
        );
        assert_eq!(user_service("1:name=x:/a\n"), None);
    }

    #[test]
    fn hidden_mounts_and_interfaces() {
        assert!(is_hidden_mount("tmpfs", "/tmp"));
        assert!(is_hidden_mount("overlay", "/"));
        assert!(is_hidden_mount("/dev/loop3", "/snap/core/123"));
        assert!(is_hidden_mount("/dev/sda1", "/var/lib/docker/overlay2/x"));
        assert!(is_hidden_mount("/dev/vda1", "/etc/hosts"));
        assert!(is_hidden_mount("x", "/run/user/1000"));
        assert!(!is_hidden_mount("/dev/nvme0n1p2", "/"));
        assert!(!is_hidden_mount("/dev/sdb1", "/mnt/data"));
        assert!(!is_hidden_mount("/dev/sdb1", "/running"));
        assert!(!is_hidden_mount("/dev/nvme0n1p1", "/boot/efi"));
        for v in ["lo", "docker0", "veth12ab", "virbr0", "br-1f2e", "cni0", "podman1"] {
            assert!(is_virtual_interface(v), "{v}");
        }
        for r in ["eth0", "enp3s0", "wlan0", "wlp2s0", "tun0", "tailscale0"] {
            assert!(!is_virtual_interface(r), "{r}");
        }
    }

    #[test]
    fn frequencies() {
        assert_eq!(mean_khz_as_mhz(&["2400000\n".into(), "3600000".into(), "bad".into()]), Some(3000));
        assert_eq!(mean_khz_as_mhz(&[]), None);
    }

    #[test]
    fn random_text_never_panics() {
        let samples = [
            "",
            "\u{0}\u{1}\u{2}",
            "(((",
            ") ) )",
            ":::",
            "a:b:c:d",
            "some ",
            "avg10=",
            "- - -",
            "999999999999999999999999 x",
        ];
        for s in samples {
            let _ = (
                psi_some_avg10(s),
                meminfo(s),
                cgroup_limit(s),
                stat_value(s, "x"),
                diskstats(s),
                stat_nice(s),
                cpus_allowed(s),
            );
            let _ = (
                cpu_list_mask(s),
                maps_files(s),
                locks(s),
                os_pretty_name(s),
                user_service(s),
                socket_inode(s),
                status_field(s, "a"),
            );
        }
    }
}
