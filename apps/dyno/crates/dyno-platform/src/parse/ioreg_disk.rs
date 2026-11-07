//! `ioreg -r -d 1 -w0 -c IOBlockStorageDriver` output: each disk driver's cumulative Statistics, and the busy percentage
//! derived from two readings (macOS has no busy-time counter; the time the disk spent serving reads and writes is the closest
//! thing, and the same quantity iostat-style tools show as utilisation).

/// One disk driver's cumulative counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskCounters {
    /// The driver's registry id (stable while the disk is attached); pairs a reading with the previous one.
    pub id: u64,
    /// "Total Time (Read)", nanoseconds.
    pub read_ns: u64,
    /// "Total Time (Write)", nanoseconds.
    pub write_ns: u64,
    /// "Bytes (Read)".
    pub read_bytes: u64,
    /// "Bytes (Write)".
    pub write_bytes: u64,
}

/// The number after `"key"=` (integers only).
fn number_after(text: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{key}\"=");
    let start = text.find(&pat)? + pat.len();
    let digits: String = text[start..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The registry id in a `+-o Name  <class X, id 0x100000b32, ...>` header.
fn header_id(block: &str) -> Option<u64> {
    let head = block.lines().next()?;
    let at = head.find("id 0x")? + "id 0x".len();
    let hex: String = head[at..].chars().take_while(char::is_ascii_hexdigit).collect();
    u64::from_str_radix(&hex, 16).ok()
}

/// Parses the driver list; a driver without the four counters is skipped.
pub fn parse_disk_counters(text: &str) -> Vec<DiskCounters> {
    let mut out = Vec::new();
    for block in text.split("+-o ").skip(1) {
        let (Some(id), Some(at)) = (header_id(block), block.find("\"Statistics\"")) else {
            continue;
        };
        let stats = block[at..].split('}').next().unwrap_or("").replace(" = ", "=");
        let (Some(read_ns), Some(write_ns), Some(read_bytes), Some(write_bytes)) = (
            number_after(&stats, "Total Time (Read)"),
            number_after(&stats, "Total Time (Write)"),
            number_after(&stats, "Bytes (Read)"),
            number_after(&stats, "Bytes (Write)"),
        ) else {
            continue;
        };
        out.push(DiskCounters {
            id,
            read_ns,
            write_ns,
            read_bytes,
            write_bytes,
        });
    }
    out
}

/// The busiest disk's busy percentage (0 to 100) over `wall_ns`: the growth of its read plus write service time against the
/// wall clock (a disk serving several requests at once can exceed the wall time; that reads as 100). Disks missing from
/// either reading, or whose counters went backwards (detached and re-attached), are skipped. `None` when no disk can be
/// compared or no time passed.
pub fn busy_percent(prev: &[DiskCounters], now: &[DiskCounters], wall_ns: u64) -> Option<f32> {
    if wall_ns == 0 {
        return None;
    }
    now.iter()
        .filter_map(|n| {
            let p = prev.iter().find(|p| p.id == n.id)?;
            let (dr, dw) = (n.read_ns.checked_sub(p.read_ns)?, n.write_ns.checked_sub(p.write_ns)?);
            let pct = (dr as f64 + dw as f64) / wall_ns as f64 * 100.0;
            Some(pct.clamp(0.0, 100.0) as f32)
        })
        .reduce(f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shape of a real reading (values invented).
    const TWO: &str = r#"+-o IOBlockStorageDriver  <class IOBlockStorageDriver, id 0x100000b32, registered, matched, active, busy 0 (168 ms), retain 8>
    {
      "IOClass" = "IOBlockStorageDriver"
      "Statistics" = {"Operations (Write)"=1677,"Latency Time (Write)"=0,"Bytes (Read)"=2000,"Errors (Write)"=0,"Total Time (Read)"=5000000,"Latency Time (Read)"=0,"Retries (Read)"=0,"Errors (Read)"=0,"Total Time (Write)"=3000000,"Bytes (Write)"=3000,"Operations (Read)"=16,"Retries (Write)"=0}
      "IOPersonalityPublisher" = "com.apple.iokit.IOStorageFamily"
    }

+-o IOBlockStorageDriver  <class IOBlockStorageDriver, id 0x100000eef, registered, matched, active, busy 0 (65 ms), retain 8>
    {
      "Statistics" = {"Bytes (Read)"=10,"Total Time (Read)"=100,"Total Time (Write)"=200,"Bytes (Write)"=20}
    }
"#;

    fn c(id: u64, read_ns: u64, write_ns: u64) -> DiskCounters {
        DiskCounters {
            id,
            read_ns,
            write_ns,
            read_bytes: 0,
            write_bytes: 0,
        }
    }

    #[test]
    fn parses_every_driver_with_its_id_and_counters() {
        let d = parse_disk_counters(TWO);
        assert_eq!(d.len(), 2);
        assert_eq!(
            d[0],
            DiskCounters {
                id: 0x100000b32,
                read_ns: 5_000_000,
                write_ns: 3_000_000,
                read_bytes: 2000,
                write_bytes: 3000
            }
        );
        assert_eq!((d[1].id, d[1].read_ns, d[1].write_ns), (0x100000eef, 100, 200));
    }

    #[test]
    fn garbage_and_incomplete_drivers_are_skipped() {
        assert!(parse_disk_counters("").is_empty());
        assert!(parse_disk_counters("\u{0}\u{1}\"Statistics\"").is_empty());
        assert!(parse_disk_counters("+-o X  <class X, id 0x1>\n \"Statistics\" = {\"Bytes (Read)\"=1}").is_empty());
        assert!(parse_disk_counters("+-o X  <class X>\n \"Statistics\" = {\"Bytes (Read)\"=1}").is_empty());
    }

    #[test]
    fn busy_percent_case_table() {
        let s = 1_000_000_000u64;
        type Case = (&'static str, Vec<DiskCounters>, Vec<DiskCounters>, u64, Option<f32>);
        let cases: &[Case] = &[
            ("idle", vec![c(1, 10, 10)], vec![c(1, 10, 10)], s, Some(0.0)),
            (
                "a quarter: 100 ms read + 150 ms write",
                vec![c(1, 0, 0)],
                vec![c(1, 100_000_000, 150_000_000)],
                s,
                Some(25.0),
            ),
            (
                "overlapping requests clamp to 100",
                vec![c(1, 0, 0)],
                vec![c(1, 900_000_000, 900_000_000)],
                s,
                Some(100.0),
            ),
            (
                "the busiest of two disks",
                vec![c(1, 0, 0), c(2, 0, 0)],
                vec![c(1, 100_000_000, 0), c(2, 500_000_000, 0)],
                s,
                Some(50.0),
            ),
            (
                "a new disk is skipped",
                vec![c(1, 0, 0)],
                vec![c(1, 100_000_000, 0), c(2, 9, 9)],
                s,
                Some(10.0),
            ),
            (
                "counters that went backwards are skipped",
                vec![c(1, 500, 500)],
                vec![c(1, 100, 100)],
                s,
                None,
            ),
            ("no disk in common", vec![c(1, 0, 0)], vec![c(2, 0, 0)], s, None),
            ("no earlier reading", vec![], vec![c(1, 0, 0)], s, None),
            ("no time passed", vec![c(1, 0, 0)], vec![c(1, 5, 5)], 0, None),
        ];
        for (name, prev, now, wall, want) in cases {
            assert_eq!(busy_percent(prev, now, *wall), *want, "{name}");
        }
    }
}
