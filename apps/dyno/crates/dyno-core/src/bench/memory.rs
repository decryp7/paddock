//! The memory test's method (ADR 0030 section 1): how big the STREAM-style arrays are, which kernel counts how many bytes, and the
//! sizes of the latency sweep. Pure: the cache sizes and the available memory come in.

/// One mebibyte.
pub const MIB: u64 = 1024 * 1024;
/// Assumed size of the last-level caches together when the OS will not say.
pub const UNKNOWN_CACHE: u64 = 32 * MIB;
/// An array is at least this many bytes.
pub const MIN_ARRAY: u64 = 128 * MIB;
/// An array is at most this fraction of the memory available at the start: `1 / AVAILABLE_DIVISOR`.
pub const AVAILABLE_DIVISOR: u64 = 8;
/// An array is at least this many times the last-level caches together.
pub const CACHE_FACTOR: u64 = 4;
/// Without a reading of the available memory the arrays stay within this many bytes.
pub const NO_READING_CAP: u64 = 512 * MIB;
/// The latency sweep: from 16 KiB to 512 MiB in doublings.
pub const SWEEP_FROM: u64 = 16 * 1024;
/// The latency sweep's end.
pub const SWEEP_TO: u64 = 512 * MIB;
/// The point that is the headline "main memory" latency.
pub const HEADLINE_BYTES: u64 = 256 * MIB;
/// One node of the latency chain per this many bytes (covers 64-byte and 128-byte lines).
pub const LINE: u64 = 128;

/// The four STREAM kernels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    /// `c = a`.
    Copy,
    /// `b = s * c`.
    Scale,
    /// `c = a + b`.
    Add,
    /// `a = b + s * c`.
    Triad,
}

impl Stream {
    /// All four, in STREAM's order.
    pub const ALL: [Stream; 4] = [Stream::Copy, Stream::Scale, Stream::Add, Stream::Triad];

    /// Lower-case name for ids.
    pub fn id(self) -> &'static str {
        match self {
            Stream::Copy => "copy",
            Stream::Scale => "scale",
            Stream::Add => "add",
            Stream::Triad => "triad",
        }
    }

    /// Words for the metric.
    pub fn title(self) -> &'static str {
        match self {
            Stream::Copy => "Copy",
            Stream::Scale => "Scale",
            Stream::Add => "Add",
            Stream::Triad => "Triad",
        }
    }

    /// Arrays touched per element, counted as STREAM counts: reads plus writes (not the write-allocate read).
    pub fn arrays(self) -> u64 {
        match self {
            Stream::Copy | Stream::Scale => 2,
            Stream::Add | Stream::Triad => 3,
        }
    }
}

/// The bytes one pass of `kernel` over arrays of `elements` `f64` counts as moving.
pub fn stream_bytes(kernel: Stream, elements: u64) -> u64 {
    kernel.arrays() * elements * 8
}

/// The size of each of the three arrays, bytes: at least 4x the caches together and at least 128 MiB, at most 1/8 of the memory
/// available; refused with a reason when that leaves less than 4x the caches. Rounded down to a whole number of 4 KiB pages.
pub fn array_bytes(caches_total: Option<u64>, available: Option<u64>) -> Result<u64, String> {
    let caches = caches_total.filter(|c| *c > 0).unwrap_or(UNKNOWN_CACHE);
    let want = (caches.saturating_mul(CACHE_FACTOR)).max(MIN_ARRAY);
    let cap = available.map_or(NO_READING_CAP, |a| a / AVAILABLE_DIVISOR);
    let need = caches.saturating_mul(CACHE_FACTOR);
    if cap < need {
        return Err(format!(
            "Not enough free memory for a memory test that leaves the processor's caches behind: it needs arrays of {} MB and about {} MB are free to use. Close some programs and run again.",
            need / 1_000_000,
            cap / 1_000_000
        ));
    }
    Ok(want.min(cap) / 4096 * 4096)
}

/// The sizes of the latency sweep, bytes, ascending.
pub fn sweep() -> Vec<u64> {
    let mut v = Vec::new();
    let mut s = SWEEP_FROM;
    while s <= SWEEP_TO {
        v.push(s);
        s *= 2;
    }
    v
}

/// The id of a sweep point: `mem-latency-16k`, `mem-latency-1m`.
pub fn sweep_id(bytes: u64) -> String {
    if bytes >= MIB {
        format!("mem-latency-{}m", bytes / MIB)
    } else {
        format!("mem-latency-{}k", bytes / 1024)
    }
}

/// The size a sweep id names (the inverse of [`sweep_id`]); `None` for any other id.
pub fn sweep_bytes(id: &str) -> Option<u64> {
    let rest = id.strip_prefix("mem-latency-")?;
    let (n, unit) = rest.split_at(rest.len().checked_sub(1)?);
    let n: u64 = n.parse().ok()?;
    match unit {
        "k" => Some(n * 1024),
        "m" => Some(n * MIB),
        _ => None,
    }
}

/// Parses a Linux cache size such as `32K`, `1024K` or `32M`.
pub fn parse_linux_cache(text: &str) -> Option<u64> {
    let t = text.trim();
    let (n, unit) = t.split_at(t.len().checked_sub(1)?);
    let n: u64 = n.parse().ok()?;
    match unit {
        "K" | "k" => Some(n * 1024),
        "M" | "m" => Some(n * MIB),
        "G" | "g" => Some(n * 1024 * MIB),
        _ => t.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrays_are_four_times_the_caches_and_at_least_128_mib() {
        assert_eq!(
            array_bytes(Some(16 * MIB), Some(64 * 1024 * MIB)).unwrap(),
            128 * MIB,
            "small caches: the floor"
        );
        assert_eq!(array_bytes(Some(64 * MIB), Some(64 * 1024 * MIB)).unwrap(), 256 * MIB);
        assert_eq!(
            array_bytes(None, Some(64 * 1024 * MIB)).unwrap(),
            128 * MIB,
            "unknown caches count as 32 MiB"
        );
    }

    #[test]
    fn arrays_are_at_most_an_eighth_of_the_available_memory() {
        // 1 GiB free: an eighth is 128 MiB, exactly the floor.
        assert_eq!(array_bytes(Some(16 * MIB), Some(1024 * MIB)).unwrap(), 128 * MIB);
        // 2 GiB free with 64 MiB of caches wants 256 MiB and gets the eighth, 256 MiB.
        assert_eq!(array_bytes(Some(64 * MIB), Some(2048 * MIB)).unwrap(), 256 * MIB);
        // 1.5 GiB free, 40 MiB of caches: wants 160 MiB, the eighth is 192 MiB, so it gets 160.
        assert_eq!(array_bytes(Some(40 * MIB), Some(1536 * MIB)).unwrap(), 160 * MIB);
    }

    #[test]
    fn too_little_memory_is_refused_with_the_numbers() {
        let e = array_bytes(Some(64 * MIB), Some(1024 * MIB)).unwrap_err();
        assert!(
            e.contains("Not enough free memory") && e.contains("268 MB") && e.contains("134 MB"),
            "{e}"
        );
    }

    #[test]
    fn without_a_reading_of_free_memory_the_arrays_stay_modest() {
        assert_eq!(array_bytes(Some(32 * MIB), None).unwrap(), 128 * MIB);
        assert!(array_bytes(Some(32 * MIB), None).unwrap() <= NO_READING_CAP);
    }

    #[test]
    fn stream_counts_bytes_the_way_stream_does() {
        assert_eq!(stream_bytes(Stream::Copy, 1000), 16_000);
        assert_eq!(stream_bytes(Stream::Scale, 1000), 16_000);
        assert_eq!(stream_bytes(Stream::Add, 1000), 24_000);
        assert_eq!(stream_bytes(Stream::Triad, 1000), 24_000);
    }

    #[test]
    fn the_sweep_doubles_from_16_kib_to_512_mib_and_its_ids_round_trip() {
        let s = sweep();
        assert_eq!((s[0], *s.last().unwrap(), s.len()), (16 * 1024, 512 * MIB, 16));
        assert!(s.contains(&HEADLINE_BYTES));
        for b in s {
            assert_eq!(sweep_bytes(&sweep_id(b)), Some(b));
        }
        assert_eq!(sweep_id(16 * 1024), "mem-latency-16k");
        assert_eq!(sweep_id(256 * MIB), "mem-latency-256m");
        assert_eq!(sweep_bytes("mem-latency"), None);
        assert_eq!(sweep_bytes("mem-latency-xk"), None);
    }

    #[test]
    fn linux_cache_sizes_parse() {
        assert_eq!(parse_linux_cache("32K\n"), Some(32 * 1024));
        assert_eq!(parse_linux_cache("1M"), Some(MIB));
        assert_eq!(parse_linux_cache("4096"), Some(4096));
        assert_eq!(parse_linux_cache(""), None);
        assert_eq!(parse_linux_cache("abc"), None);
    }
}
