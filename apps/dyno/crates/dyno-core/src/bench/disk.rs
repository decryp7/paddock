//! The disk test's method (ADR 0030 section 1): the four patterns, the byte and time limits of each phase, the write budget the
//! picker shows, the free-space policy and which volumes are refused. Pure: the runner does the I/O and the agent the asking.

use crate::disk::volume::{Volume, VolumeLocation};
use std::time::Duration;

/// One mebibyte.
pub const MIB: u64 = 1024 * 1024;
/// One gibibyte.
pub const GIB: u64 = 1024 * MIB;
/// The most a ramp (the part that is not counted) may take, ms, besides its byte limit.
pub const RAMP_MAX_MILLIS: u64 = 1_000;
/// A ramp moves at most this fraction of the file: `1 / RAMP_DIVISOR`.
pub const RAMP_DIVISOR: u64 = 8;
/// The most a measured part may take, ms (it ends earlier when it has moved the file's size).
pub const MEASURE_MAX_MILLIS: u64 = 5_000;
/// The measured part of a random pattern on a spinning disk, ms (each operation takes milliseconds there).
pub const ROTATIONAL_RANDOM_MILLIS: u64 = 2_000;
/// The least free space left besides 10 % of the volume, bytes.
pub const MIN_FREE_MARGIN: u64 = 2 * GIB;
/// Repetitions of every disk pattern.
pub const REPETITIONS: u32 = 3;
/// The default file size, MiB (quick: [`QUICK_MIB`]).
pub const DEFAULT_MIB: u32 = 1024;
/// The quick preset's file size, MiB.
pub const QUICK_MIB: u32 = 256;

/// One access pattern, measured for reading and for writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pattern {
    /// Stable id, part of the metric ids.
    pub id: &'static str,
    /// The words for the phase and the metric.
    pub label: &'static str,
    /// Bytes per operation.
    pub block: usize,
    /// Threads, each with one outstanding `pread`/`pwrite` on its own offsets (never called "queue depth").
    pub threads: usize,
    /// Random offsets (otherwise sequential, each thread on its own stretch).
    pub random: bool,
}

/// CrystalDiskMark's defaults as the reference; fio's names in the logs.
pub const PATTERNS: [Pattern; 4] = [
    Pattern {
        id: "seq1m-t1",
        label: "Sequential 1 MiB, 1 thread",
        block: 1024 * 1024,
        threads: 1,
        random: false,
    },
    Pattern {
        id: "seq1m-t8",
        label: "Sequential 1 MiB, 8 threads",
        block: 1024 * 1024,
        threads: 8,
        random: false,
    },
    Pattern {
        id: "rand4k-t1",
        label: "Random 4 KiB, 1 thread",
        block: 4096,
        threads: 1,
        random: true,
    },
    Pattern {
        id: "rand4k-t32",
        label: "Random 4 KiB, 32 threads",
        block: 4096,
        threads: 32,
        random: true,
    },
];

/// What bounds one phase of a pattern: whichever ends first. Every byte limit is enforced in the runner's loop, never derived from a speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The ramp moves at most this many bytes.
    pub ramp_bytes: u64,
    /// The ramp takes at most this long.
    pub ramp_time: Duration,
    /// The measured part moves at most this many bytes (the file's size).
    pub measure_bytes: u64,
    /// The measured part takes at most this long.
    pub measure_time: Duration,
}

/// The limits of `pattern` on a file of `file_bytes`.
pub fn limits(file_bytes: u64, pattern: &Pattern, rotational: bool) -> Limits {
    let measure_ms = if rotational && pattern.random {
        ROTATIONAL_RANDOM_MILLIS
    } else {
        MEASURE_MAX_MILLIS
    };
    Limits {
        ramp_bytes: file_bytes / RAMP_DIVISOR,
        ramp_time: Duration::from_millis(RAMP_MAX_MILLIS),
        measure_bytes: file_bytes,
        measure_time: Duration::from_millis(measure_ms),
    }
}

/// The most bytes a run writes to one drive: the fill (one pass of the file, so the read patterns read real data), plus for each
/// write pattern and repetition the ramp (at most 1/8 of the file) and the measured part (at most the file). The picker shows it
/// rounded up and the runner enforces the same number, so what is shown is an upper bound by construction.
pub fn disk_write_budget(file_mib: u32, repetitions: u32) -> u64 {
    disk_write_budget_with(file_mib, repetitions, 0)
}

/// [`disk_write_budget`] plus the sustained write of `sustained_gib` GiB, when the person confirmed one.
pub fn disk_write_budget_with(file_mib: u32, repetitions: u32, sustained_gib: u32) -> u64 {
    let sustained = u64::from(sustained_gib) * GIB;
    sustained.saturating_add(fill_and_passes(file_mib, repetitions))
}

fn fill_and_passes(file_mib: u32, repetitions: u32) -> u64 {
    let file = u64::from(file_mib) * MIB;
    let per_pass = file / RAMP_DIVISOR + file;
    file.saturating_add(
        u64::from(repetitions)
            .saturating_mul(PATTERNS.len() as u64)
            .saturating_mul(per_pass),
    )
}

/// The most the sustained write may write to one drive, GiB.
pub const SUSTAINED_WRITE_MAX_GIB: u32 = 64;
/// The sustained write is also at most this share of the free space.
pub const SUSTAINED_FREE_DIVISOR: u64 = 4;
/// The sustained write ends after this long whatever its speed, seconds (a slow drive is for the person to cancel, not for us to
/// wait out, but the run is bounded).
pub const SUSTAINED_WRITE_MAX_SECS: u64 = 30 * 60;

/// The sustained write the person may choose on a drive with `free` bytes free: the smaller of 25 % of the free space and 64 GiB,
/// in whole GiB; zero when not even 1 GiB fits.
pub fn sustained_write_cap_gib(free: u64) -> u32 {
    u32::try_from((free / SUSTAINED_FREE_DIVISOR / GIB).min(u64::from(SUSTAINED_WRITE_MAX_GIB))).unwrap_or(0)
}

/// The words the confirmation shows for a sustained write.
pub fn sustained_write_warning(gib: u32, drive: &str) -> String {
    format!("This writes {gib} GB more to {drive} to measure its speed after its fast cache is full. It adds wear to the drive and can take several minutes.")
}

/// The budget rounded up to whole GB for the words "This writes up to 15 GB to Macintosh HD".
pub fn budget_gb_up(bytes: u64) -> u64 {
    bytes.div_ceil(1_000_000_000)
}

/// The free space a drive needs before a run: the file plus the larger of 10 % of the volume and [`MIN_FREE_MARGIN`].
pub fn free_space_required(file_bytes: u64, capacity: Option<u64>) -> u64 {
    file_bytes.saturating_add((capacity.unwrap_or(0) / 10).max(MIN_FREE_MARGIN))
}

/// Refuses a run when the volume has too little free space, with the numbers and what to do. `free` unknown is a refusal too:
/// the test never writes blind.
pub fn check_free(name: &str, file_bytes: u64, capacity: Option<u64>, free: Option<u64>) -> Result<(), String> {
    let Some(free) = free else {
        return Err(format!("Dyno cannot tell how much space {name} has free, so it does not test it."));
    };
    let need = free_space_required(file_bytes, capacity);
    if free >= need {
        Ok(())
    } else {
        Err(format!(
            "{name} has {} GB free; the disk test needs {} GB (the test file plus a safety margin). Free some space or choose a smaller file.",
            free / 1_000_000_000,
            need.div_ceil(1_000_000_000)
        ))
    }
}

/// Why a volume cannot be tested, in words for the person; `None` when it can. Network drives are tested only when the person
/// agreed for this run.
pub fn refusal(v: &Volume, allow_network: bool) -> Option<String> {
    let fs = v.fs.to_ascii_lowercase();
    if v.read_only {
        return Some(format!("{} is read-only, so Dyno cannot write a test file to it.", v.name));
    }
    if !v.reachable {
        return Some(format!("{} did not answer.", v.name));
    }
    if matches!(fs.as_str(), "iso9660" | "cd9660" | "udf" | "hsfs") {
        return Some(format!("{} is an optical disc.", v.name));
    }
    if matches!(fs.as_str(), "tmpfs" | "ramfs" | "devtmpfs") {
        return Some(format!("{} is a RAM disk; its speed is the memory's, not a drive's.", v.name));
    }
    if matches!(v.location, VolumeLocation::Network { .. }) && !allow_network {
        return Some(format!(
            "{} is a network drive. Tick \"include network drives\" to test it; the result is marked network.",
            v.name
        ));
    }
    None
}

/// A short id for a drive in metric ids: the lowercase letters and digits of its mount point (at most 16), then a hash of the whole
/// mount point, so two long names that start alike never collide. The system root is `root`.
pub fn drive_slug(mount: &str) -> String {
    let mut readable = String::new();
    for c in mount.chars() {
        if c.is_ascii_alphanumeric() {
            readable.push(c.to_ascii_lowercase());
        } else if !readable.ends_with('-') && !readable.is_empty() {
            readable.push('-');
        }
    }
    let readable: String = readable.trim_end_matches('-').chars().take(16).collect();
    let readable = readable.trim_end_matches('-');
    if readable.is_empty() {
        return "root".into();
    }
    // FNV-1a, 32 bits: stable across runs and platforms (unlike `DefaultHasher`).
    let hash = mount
        .bytes()
        .fold(0x811C_9DC5_u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
    format!("{readable}-{hash:08x}")
}

/// The metric id the runner gives a pattern's direction, and the suffix kinds: sequential metrics are MB/s only, random ones are IOPS and MB/s.
pub fn metric_id(pattern: &Pattern, write: bool, suffix: &str) -> String {
    let dir = if write { "write" } else { "read" };
    if suffix.is_empty() {
        format!("disk-{}-{dir}", pattern.id)
    } else {
        format!("disk-{}-{dir}-{suffix}", pattern.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk::volume::NetworkProtocol;

    fn vol(fs: &str, location: VolumeLocation) -> Volume {
        Volume {
            id: "/Volumes/X".into(),
            name: "X".into(),
            mount: "/Volumes/X".into(),
            fs: fs.into(),
            location,
            capacity: Some(100 * GIB),
            free: Some(50 * GIB),
            rotational: Some(false),
            read_only: false,
            reachable: true,
            whole_volume: true,
            measured_at: None,
        }
    }

    #[test]
    fn the_default_budget_is_fourteen_and_a_half_gib() {
        // 1 GiB fill + 12 x (1/8 + 1) GiB, ADR 0030 D3.
        assert_eq!(disk_write_budget(DEFAULT_MIB, REPETITIONS), 14_848 * MIB);
        assert_eq!(budget_gb_up(disk_write_budget(DEFAULT_MIB, REPETITIONS)), 16);
    }

    #[test]
    fn the_sustained_write_is_counted_in_the_same_budget_and_capped() {
        assert_eq!(
            disk_write_budget_with(DEFAULT_MIB, REPETITIONS, 0),
            disk_write_budget(DEFAULT_MIB, REPETITIONS)
        );
        assert_eq!(
            disk_write_budget_with(DEFAULT_MIB, REPETITIONS, 8),
            disk_write_budget(DEFAULT_MIB, REPETITIONS) + 8 * GIB
        );
        assert_eq!(sustained_write_cap_gib(100 * GIB), 25);
        assert_eq!(sustained_write_cap_gib(1000 * GIB), 64);
        assert_eq!(sustained_write_cap_gib(3 * GIB), 0);
        assert!(sustained_write_warning(8, "Macintosh HD").contains("8 GB more to Macintosh HD"));
    }

    #[test]
    fn the_budget_is_pinned_for_each_preset() {
        assert_eq!(disk_write_budget(QUICK_MIB, REPETITIONS), 3_712 * MIB);
        assert_eq!(disk_write_budget(4096, REPETITIONS), 59_392 * MIB);
        assert_eq!(disk_write_budget(16, 1), 16 * MIB + 4 * 18 * MIB);
        assert!(
            disk_write_budget(u32::MAX, u32::MAX) > 0,
            "huge numbers saturate instead of wrapping"
        );
    }

    #[test]
    fn limits_are_bytes_and_time_whichever_ends_first() {
        let l = limits(GIB, &PATTERNS[0], false);
        assert_eq!(l.ramp_bytes, 128 * MIB);
        assert_eq!(l.ramp_time, Duration::from_secs(1));
        assert_eq!(l.measure_bytes, GIB);
        assert_eq!(l.measure_time, Duration::from_secs(5));
        assert_eq!(
            limits(GIB, &PATTERNS[2], true).measure_time,
            Duration::from_secs(2),
            "spinning disk, random"
        );
        assert_eq!(
            limits(GIB, &PATTERNS[0], true).measure_time,
            Duration::from_secs(5),
            "spinning disk, sequential"
        );
    }

    #[test]
    fn free_space_needs_the_file_and_the_larger_margin() {
        // 100 GB volume: the margin is 10 GB; 10 GB volume: the margin is the 2 GiB floor.
        assert_eq!(free_space_required(GIB, Some(100 * GIB)), GIB + 10 * GIB);
        assert_eq!(free_space_required(GIB, Some(10 * GIB)), GIB + 2 * GIB);
        assert_eq!(free_space_required(GIB, None), 3 * GIB);
        assert!(check_free("X", GIB, Some(100 * GIB), Some(11 * GIB)).is_ok());
        let e = check_free("X", GIB, Some(100 * GIB), Some(5 * GIB)).unwrap_err();
        assert!(e.contains("5 GB free") && e.contains("needs"), "{e}");
        assert!(check_free("X", GIB, Some(100 * GIB), None).unwrap_err().contains("cannot tell"));
    }

    #[test]
    fn read_only_network_optical_ram_and_silent_volumes_are_refused_with_the_reason() {
        assert!(refusal(&vol("apfs", VolumeLocation::Internal), false).is_none());
        let mut ro = vol("apfs", VolumeLocation::Internal);
        ro.read_only = true;
        assert!(refusal(&ro, false).unwrap().contains("read-only"));
        let mut gone = vol("apfs", VolumeLocation::External { bus: None });
        gone.reachable = false;
        assert!(refusal(&gone, false).unwrap().contains("did not answer"));
        assert!(refusal(&vol("cd9660", VolumeLocation::Internal), false)
            .unwrap()
            .contains("optical"));
        assert!(refusal(&vol("tmpfs", VolumeLocation::Internal), false)
            .unwrap()
            .contains("RAM disk"));
        let net = VolumeLocation::Network {
            protocol: NetworkProtocol::Smb,
            host: "nas".into(),
        };
        assert!(refusal(&vol("smbfs", net.clone()), false).unwrap().contains("network drive"));
        assert!(
            refusal(&vol("smbfs", net), true).is_none(),
            "a network drive is tested when the person agreed"
        );
    }

    #[test]
    fn drive_slugs_are_short_and_safe() {
        let s = drive_slug("/Volumes/Macintosh HD");
        assert!(
            s.starts_with("volumes-macintos-") && s.len() == "volumes-macintos-".len() + 8,
            "{s}"
        );
        assert_eq!(s, drive_slug("/Volumes/Macintosh HD"), "stable");
        assert!(drive_slug("C:\\").starts_with("c-"));
        assert_eq!(drive_slug("/"), "root");
        assert!(drive_slug(&"a/".repeat(50)).len() <= 16 + 9);
        assert_ne!(
            drive_slug("/Volumes/A very long disk name 1"),
            drive_slug("/Volumes/A very long disk name 2")
        );
    }

    #[test]
    fn metric_ids_name_pattern_and_direction() {
        assert_eq!(metric_id(&PATTERNS[1], false, ""), "disk-seq1m-t8-read");
        assert_eq!(metric_id(&PATTERNS[3], true, "iops"), "disk-rand4k-t32-write-iops");
    }
}
