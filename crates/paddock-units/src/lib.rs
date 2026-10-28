//! The one definition of how byte sizes are written and read (family decision 2026-10-28, ER-21).
//!
//! Two kinds of size exist, and every call site picks one:
//!
//! - [`Kind::Storage`]: files, disks, downloads, caches, imports, logs and network transfer. Decimal steps (1 KB = 1000 bytes),
//!   as the macOS Finder shows them.
//! - [`Kind::Memory`]: RAM, process memory, working set, heap and GPU memory. Binary steps (1 KB = 1024 bytes) under the same
//!   labels, as Activity Monitor and About This Mac show them (16 GiB is "16 GB"). A rate follows its quantity.
//!
//! Labels are always B-less short forms: `bytes`, `KB`, `MB`, `GB`, `TB`, `PB`. The TypeScript twin is `formatBytes`,
//! `formatWholeBytes` and `parseSize` in `@decryp7/paddock-ui/format`; `cases.json` is the table both are tested against.

/// Which convention a size follows; see the crate docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Files, disks, downloads, caches, imports, logs, network transfer: 1 KB = 1000 bytes.
    Storage,
    /// RAM, process memory, working set, heap, GPU memory: 1 KB = 1024 bytes.
    Memory,
}

impl Kind {
    /// Bytes in one KB of this kind.
    pub const fn step(self) -> u64 {
        match self {
            Kind::Storage => 1000,
            Kind::Memory => 1024,
        }
    }

    /// Bytes in `n` KB, MB, GB ... : `Kind::Storage.unit(2)` is one million.
    pub const fn unit(self, power: u32) -> u64 {
        self.step().pow(power)
    }
}

const LABELS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];

fn round_to(v: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (v * f).round() / f
}

fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn plain(n: u64) -> String {
    if n == 1 { "1 byte".to_string() } else { format!("{} bytes", group(n)) }
}

/// A size for people: `0` → `"0 bytes"`, `1` → `"1 byte"`, `1_500` → `"1.5 KB"` (storage), `16 << 30` → `"16 GB"` (memory).
/// One decimal below 10, none above, no trailing `.0`, thousands grouped. 999.9 KB rolls over to `"1 MB"`.
pub fn format_bytes(bytes: u64, kind: Kind) -> String {
    let step = kind.step();
    if bytes < step {
        return plain(bytes);
    }
    let rounded = |v: f64| if v < 10.0 { round_to(v, 1) } else { v.round() };
    let stepf = step as f64;
    let mut value = bytes as f64 / stepf;
    let mut i = 0;
    while i < LABELS.len() - 1 && rounded(value) >= stepf {
        value /= stepf;
        i += 1;
    }
    let shown = if value < 10.0 { round_to(value, 1) } else { value.round() };
    let text = if shown.fract() == 0.0 { group(shown as u64) } else { format!("{shown:.1}") };
    format!("{text} {}", LABELS[i])
}

/// The exact size for a tooltip or a log: `1234567` → `"1,234,567 bytes"`.
pub fn exact_bytes(bytes: u64) -> String {
    plain(bytes)
}

/// The largest unit `bytes` is a whole multiple of, for limits in messages and settings: `10_000_000` → `"10 MB"` (storage),
/// `10 << 20` → `"10 MB"` (memory), `1500` → `"1500 bytes"`.
pub fn format_whole(bytes: u64, kind: Kind) -> String {
    for power in (1..=3u32).rev() {
        let size = kind.unit(power);
        if bytes >= size && bytes.is_multiple_of(size) {
            return format!("{} {}", bytes / size, LABELS[power as usize - 1]);
        }
    }
    format!("{bytes} bytes")
}

/// A size typed by a person: `512`, `64k`, `10MB`, `1.5 g`, `2 GiB` read into bytes. A bare number is bytes. The suffix
/// (k, M, G, T, optional `B`) takes the steps of `kind`; an explicit `KiB` / `MiB` / `GiB` / `TiB` is always binary.
/// The error says what to write.
pub fn parse_size(text: &str, kind: Kind) -> Result<u64, String> {
    let bad = || format!("\"{text}\" is not a size: write a number of bytes or a number with k, M, G or T (for example 512M)");
    let lower = text.trim().to_ascii_lowercase();
    let t = lower.strip_suffix('b').unwrap_or(&lower);
    let (t, binary) = match t.strip_suffix('i') {
        Some(rest) => (rest, true),
        None => (t, false),
    };
    let kind = if binary { Kind::Memory } else { kind };
    let (num, power) = match t.chars().last() {
        Some('k') => (&t[..t.len() - 1], 1),
        Some('m') => (&t[..t.len() - 1], 2),
        Some('g') => (&t[..t.len() - 1], 3),
        Some('t') => (&t[..t.len() - 1], 4),
        _ if binary => return Err(bad()),
        _ => (t, 0),
    };
    let n: f64 = num.trim().parse().map_err(|_| bad())?;
    if !n.is_finite() || n < 0.0 {
        return Err(bad());
    }
    let bytes = n * kind.unit(power) as f64;
    if bytes >= u64::MAX as f64 {
        return Err(bad());
    }
    Ok(bytes as u64)
}
