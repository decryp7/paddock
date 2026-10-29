//! Transparent reading of compressed input: gzip files and zip archives are
//! detected by their magic bytes (not their extension) and expanded into the
//! files they contain. Decompressed sizes are capped so a hostile archive
//! (a "zip bomb") fails with a message instead of exhausting memory.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use std::sync::atomic::{AtomicBool, Ordering};

/// Largest decompressed size of one entry (1 GiB).
pub const MAX_ENTRY_BYTES: u64 = 1 << 30;
/// Largest total decompressed size of one archive (4 GiB).
pub const MAX_ARCHIVE_BYTES: u64 = 4 << 30;
/// Most entries one archive may hold (a hostile one can name millions in a few megabytes).
pub const MAX_ENTRIES: usize = 65_536;
/// Separator between an archive's path and an entry inside it.
pub const ENTRY_SEPARATOR: &str = "!/";

/// One file to import: the bytes, and the entry name when it came from an archive.
pub struct Entry {
    /// `None` for a plain file; the entry path for a zip member; the inner file
    /// name for a gzip file.
    pub name: Option<String>,
    pub bytes: Vec<u8>,
    /// The entry's own path when the archive names it more than once and this copy was given `name` instead
    /// ([`expand`]); the import report lists it.
    pub renamed_from: Option<String>,
}

pub fn is_gzip(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x1f, 0x8b])
}

pub fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

/// Expands `bytes` (the content of `file_name`) into the files to import.
///
/// A zip that names one full path more than once (tools that append to an archive write such files) is not refused and
/// loses nothing: every entry is read, in the archive's order. The first entry of a path keeps its name; the next ones
/// become `<stem> (2)<.ext>`, `(3)` and so on in the same folder (`logs/app.log` -> `logs/app (2).log`), skipping any
/// name a real entry of the archive has. Such an entry carries its own path in [`Entry::renamed_from`]. Names are
/// compared after `\` has become `/`, so `d/a.log` and `d\a.log` count as one path. The user decided this on
/// 2026-10-29 (docs/plans/debrief-1-2.md, section 6). The per-entry and per-archive limits apply to every entry,
/// renamed or not, and an archive of more than [`MAX_ENTRIES`] entries is refused.
pub fn expand(file_name: &str, bytes: Vec<u8>) -> Result<Vec<Entry>> {
    expand_cancellable(file_name, bytes, &AtomicBool::new(false))
}

/// [`expand`] that stops between two entries once `cancel` is set, with an [`ExpandCancelled`] error.
pub fn expand_cancellable(file_name: &str, bytes: Vec<u8>, cancel: &AtomicBool) -> Result<Vec<Entry>> {
    expand_with(file_name, bytes, Limits::DEFAULT, cancel)
}

/// The limits [`expand_with`] enforces.
#[derive(Clone, Copy)]
struct Limits {
    entry: u64,
    archive: u64,
    entries: usize,
}

impl Limits {
    const DEFAULT: Limits = Limits {
        entry: MAX_ENTRY_BYTES,
        archive: MAX_ARCHIVE_BYTES,
        entries: MAX_ENTRIES,
    };
}

/// [`expand`] with explicit limits per entry and per archive.
#[cfg(test)]
fn expand_capped(file_name: &str, bytes: Vec<u8>, max_entry: u64, max_archive: u64) -> Result<Vec<Entry>> {
    let limits = Limits {
        entry: max_entry,
        archive: max_archive,
        entries: MAX_ENTRIES,
    };
    expand_with(file_name, bytes, limits, &AtomicBool::new(false))
}

fn expand_with(file_name: &str, bytes: Vec<u8>, limits: Limits, cancel: &AtomicBool) -> Result<Vec<Entry>> {
    let Limits {
        entry: max_entry,
        archive: max_archive,
        entries: max_entries,
    } = limits;
    check_cancel(cancel)?;
    if is_gzip(&bytes) {
        let inner = file_name
            .strip_suffix(".gz")
            .or_else(|| file_name.strip_suffix(".GZ"))
            .unwrap_or(file_name);
        let data = read_capped(flate2::read::MultiGzDecoder::new(Cursor::new(bytes)), max_entry).context("invalid gzip data")?;
        return Ok(vec![Entry {
            name: Some(inner.to_string()),
            bytes: data,
            renamed_from: None,
        }]);
    }
    if is_zip(&bytes) {
        let zip = zip::ZipArchive::new(Cursor::new(&bytes[..])).context("invalid zip archive")?;
        too_many(zip.len(), max_entries)?;
        let records = central_records(&bytes, zip.central_directory_start(), zip.offset(), max_entries)?;
        check_ranges(&bytes, &records)?;
        let mut namer = Namer::new(
            records
                .iter()
                .map(|r| r.name.clone())
                .chain(zip.file_names().filter_map(Result::ok).map(normal)),
        );
        if records.len() != zip.len() {
            drop(zip);
            return expand_repeated(&bytes, &records, namer, limits, cancel);
        }
        drop(zip);
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).context("invalid zip entry")?;
        let mut out = Vec::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            check_cancel(cancel)?;
            let entry = zip.by_index(i).context("invalid zip entry")?;
            if entry.is_dir() {
                continue;
            }
            let own = normal(entry.name().context("invalid zip entry name")?);
            let name = namer.assign(&own);
            let data = read_capped(entry, max_entry).with_context(|| format!("zip entry {name}"))?;
            total += data.len() as u64;
            if total > max_archive {
                return Err(anyhow!("archive expands to more than {} MiB", max_archive >> 20));
            }
            out.push(Entry {
                renamed_from: (name != own).then_some(own),
                name: Some(name),
                bytes: data,
            });
        }
        return Ok(out);
    }
    Ok(vec![Entry {
        name: None,
        bytes,
        renamed_from: None,
    }])
}

/// The error of an expansion stopped by its cancel signal.
#[derive(Debug)]
pub struct ExpandCancelled;

impl std::fmt::Display for ExpandCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("expanding the archive was cancelled")
    }
}

impl std::error::Error for ExpandCancelled {}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(ExpandCancelled.into());
    }
    Ok(())
}

fn too_many(count: usize, max: usize) -> Result<()> {
    if count > max {
        bail!("the zip archive has more than {max} entries");
    }
    Ok(())
}

/// A zip path with `\` as `/`.
fn normal(name: impl AsRef<str>) -> String {
    name.as_ref().replace('\\', "/")
}

/// One file header of a zip's central directory: its name, where its local header is, how many compressed bytes follow
/// that header and what they must decode to.
struct Record {
    /// The name in the central directory, with `\` as `/`.
    name: String,
    is_dir: bool,
    local_header: u64,
    compressed: u64,
    uncompressed: u64,
    crc: u32,
}

/// IBM code page 437 for the bytes `0x80..=0xFF` (the zip name encoding when the UTF-8 flag is not set).
const CP437_HIGH: &str =
    "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

/// A central directory name: UTF-8 when general purpose flag bit 11 says so, code page 437 otherwise (as zip's reader).
fn decode_name(raw: &[u8], flags: u64) -> String {
    if flags & 0x800 != 0 {
        return String::from_utf8_lossy(raw).into_owned();
    }
    let high: Vec<char> = CP437_HIGH.chars().collect();
    raw.iter()
        .map(|&b| if b < 0x80 { char::from(b) } else { high[usize::from(b) - 0x80] })
        .collect()
}

/// Every record of the central directory at `start`, repeated names included (zip's own index keeps one per name).
/// `offset` is the size of data before the archive (the local header offsets are relative to the archive's start).
/// More than `max` records, damaged or truncated records are an error, never a panic.
fn central_records(bytes: &[u8], start: u64, offset: u64, max: usize) -> Result<Vec<Record>> {
    const CENTRAL: u32 = 0x0201_4b50;
    const FIXED: usize = 46;
    let truncated = || anyhow!("the zip's central directory is truncated");
    let le = |at: usize, n: usize| -> Option<u64> {
        let b = bytes.get(at..at.checked_add(n)?)?;
        Some(b.iter().rev().fold(0u64, |v, &x| (v << 8) | u64::from(x)))
    };
    let u16_at = |at: usize| le(at, 2);
    let u32_at = |at: usize| le(at, 4);
    let u64_at = |at: usize| le(at, 8);
    let mut records = Vec::new();
    let mut at = usize::try_from(start).context("the zip's central directory is out of range")?;
    while u32_at(at) == Some(u64::from(CENTRAL)) {
        too_many(records.len() + 1, max)?;
        let field = |rel: usize, read: &dyn Fn(usize) -> Option<u64>| at.checked_add(rel).and_then(read).ok_or_else(truncated);
        let flags = field(8, &u16_at)?;
        let crc = field(16, &u32_at)? as u32;
        let (mut uncompressed, mut compressed) = (field(24, &u32_at)?, field(20, &u32_at)?);
        let mut local_header = field(42, &u32_at)?;
        let (name_len, extra_len, comment_len) = (
            field(28, &u16_at)? as usize,
            field(30, &u16_at)? as usize,
            field(32, &u16_at)? as usize,
        );
        let name_at = at.saturating_add(FIXED);
        let raw = name_at
            .checked_add(name_len)
            .and_then(|end| bytes.get(name_at..end))
            .ok_or_else(truncated)?;
        let name = normal(decode_name(raw, flags));
        // Zip64 (APPNOTE 4.5.3): the values that did not fit are in the extra field 0x0001, in this order.
        let mut extra = name_at.saturating_add(name_len);
        let extra_end = extra.saturating_add(extra_len);
        while extra + 4 <= extra_end {
            let (id, len) = (field(extra - at, &u16_at)?, field(extra + 2 - at, &u16_at)? as usize);
            if id == 1 {
                let field_end = extra.saturating_add(4).saturating_add(len);
                let mut v = extra + 4;
                for value in [&mut uncompressed, &mut compressed, &mut local_header] {
                    if *value == 0xFFFF_FFFF {
                        if v.saturating_add(8) > field_end {
                            bail!("the zip's central directory has a damaged zip64 extra field");
                        }
                        *value = u64_at(v).ok_or_else(truncated)?;
                        v += 8;
                    }
                }
            }
            extra = extra.saturating_add(4 + len);
        }
        records.push(Record {
            is_dir: name.ends_with('/'),
            name,
            local_header: local_header.saturating_add(offset),
            compressed,
            uncompressed,
            crc,
        });
        at = extra_end.saturating_add(comment_len);
    }
    Ok(records)
}

/// Refuses an archive in which two entries share a byte of header or data (the zip crate's `has_overlapping_entries`
/// for records it cannot index): many central records over one local entry would be read as many entries, and the
/// renaming and reading work grows with their number, not with the archive's size.
fn check_ranges(bytes: &[u8], records: &[Record]) -> Result<()> {
    const LOCAL: u32 = 0x0403_4b50;
    const FIXED: u64 = 30;
    let bad = || anyhow!("invalid zip archive: an entry's header is outside the archive");
    let mut ranges = Vec::with_capacity(records.len());
    for r in records {
        let at = usize::try_from(r.local_header).map_err(|_| bad())?;
        let head = bytes.get(at..at.checked_add(FIXED as usize).ok_or_else(bad)?).ok_or_else(bad)?;
        if u32::from_le_bytes([head[0], head[1], head[2], head[3]]) != LOCAL {
            bail!("invalid zip archive: no file header where the central directory points");
        }
        let lens = u64::from(u16::from_le_bytes([head[26], head[27]])) + u64::from(u16::from_le_bytes([head[28], head[29]]));
        let end = r
            .local_header
            .checked_add(FIXED + lens)
            .and_then(|e| e.checked_add(r.compressed))
            .filter(|&e| e <= bytes.len() as u64)
            .ok_or_else(bad)?;
        ranges.push((r.local_header, end));
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|w| w[0].1 > w[1].0) {
        bail!("invalid zip archive: entries overlap");
    }
    Ok(())
}

/// [`expand_with`] for a zip whose central directory names a path more than once: each entry is read from its own
/// local header, with the CRC-32 and sizes of its central record (a local header written with a data descriptor holds
/// none), and repeated paths are renamed (see [`expand`]).
fn expand_repeated(bytes: &[u8], records: &[Record], mut namer: Namer, limits: Limits, cancel: &AtomicBool) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    let mut total = 0u64;
    for r in records {
        check_cancel(cancel)?;
        if r.is_dir {
            continue;
        }
        let from = usize::try_from(r.local_header)
            .ok()
            .and_then(|at| bytes.get(at..))
            .ok_or_else(|| anyhow!("invalid zip entry: its header is outside the archive"))?;
        let mut reader = Cursor::new(from);
        let options = zip::read::ZipReadOptions::new()
            .override_compressed_size(r.compressed)
            .override_uncompressed_size(r.uncompressed)
            .override_crc(r.crc);
        let name = namer.assign(&r.name);
        let Some(entry) =
            zip::read::read_zipfile_from_stream_with_options(&mut reader, options).with_context(|| format!("zip entry {name}"))?
        else {
            bail!("invalid zip entry: no file header where the central directory points");
        };
        let data = read_capped(entry, limits.entry).with_context(|| format!("zip entry {name}"))?;
        total += data.len() as u64;
        if total > limits.archive {
            return Err(anyhow!("archive expands to more than {} MiB", limits.archive >> 20));
        }
        out.push(Entry {
            renamed_from: (name != r.name).then(|| r.name.clone()),
            name: Some(name),
            bytes: data,
        });
    }
    Ok(out)
}

/// Gives every entry of an archive a distinct name (see [`expand`]). Each rename is amortised O(1): the next number to
/// try for a path is remembered, so n copies of one path cost n probes, not n squared.
struct Namer {
    /// Every name an entry of the archive has, and the ones given out.
    taken: HashSet<String>,
    /// The paths that have been assigned at least once.
    seen: HashSet<String>,
    /// For a path, the next number `<stem> (n)<.ext>` to try.
    next: HashMap<String, u64>,
}

impl Namer {
    fn new(names: impl IntoIterator<Item = String>) -> Self {
        Namer {
            taken: names.into_iter().collect(),
            seen: HashSet::new(),
            next: HashMap::new(),
        }
    }

    /// The name for the next entry whose own path is `own`: `own` itself for the first, a numbered one after.
    fn assign(&mut self, own: &str) -> String {
        if self.seen.insert(own.to_string()) {
            self.taken.insert(own.to_string());
            return own.to_string();
        }
        let (folder, file) = own.rsplit_once('/').map_or(("", own), |(d, f)| (d, f));
        let (stem, ext) = match file.rfind('.') {
            Some(dot) if dot > 0 => file.split_at(dot),
            _ => (file, ""),
        };
        let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
        let n = self.next.entry(own.to_string()).or_insert(2);
        loop {
            let candidate = format!("{prefix}{stem} ({n}){ext}");
            *n += 1;
            if self.taken.insert(candidate.clone()) {
                return candidate;
            }
        }
    }
}

/// Reads one entry of an archive again (for the context view).
pub fn read_entry(bytes: Vec<u8>, file_name: &str, entry: &str) -> Result<Vec<u8>> {
    expand(file_name, bytes)?
        .into_iter()
        .find(|e| e.name.as_deref() == Some(entry))
        .map(|e| e.bytes)
        .ok_or_else(|| anyhow!("{entry} is no longer in {file_name}"))
}

fn read_capped(reader: impl Read, cap: u64) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    reader.take(cap + 1).read_to_end(&mut data)?;
    if data.len() as u64 > cap {
        return Err(anyhow!("expands to more than {} MiB", cap >> 20));
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    pub(crate) fn gzip(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    pub(crate) fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.add_directory("logs/", opts).unwrap();
        for (name, data) in files {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn expands_gzip_zip_and_plain_files() {
        let plain = expand("a.log", b"hello".to_vec()).unwrap();
        assert_eq!((plain[0].name.as_deref(), plain[0].bytes.as_slice()), (None, &b"hello"[..]));

        let gz = expand("app.log.gz", gzip(b"line 1\nline 2\n")).unwrap();
        assert_eq!(
            (gz[0].name.as_deref(), gz[0].bytes.as_slice()),
            (Some("app.log"), &b"line 1\nline 2\n"[..])
        );
        // Detected by content: no .gz suffix needed.
        assert_eq!(expand("rotated.1", gzip(b"x")).unwrap()[0].name.as_deref(), Some("rotated.1"));

        let z = zip_of(&[("logs/a.log", b"A"), ("logs\\b.log", b"B")]);
        let entries = expand("bundle.zip", z.clone()).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.clone().unwrap()).collect();
        assert_eq!(names, ["logs/a.log", "logs/b.log"]);
        assert_eq!(read_entry(z.clone(), "bundle.zip", "logs/b.log").unwrap(), b"B");
        assert!(read_entry(z, "bundle.zip", "missing").is_err());
    }

    #[test]
    fn rejects_corrupt_and_oversized_data() {
        assert!(expand("bad.gz", vec![0x1f, 0x8b, 0, 1, 2]).is_err());
        assert!(expand("bad.zip", b"PK\x03\x04garbage".to_vec()).is_err());
        assert!(read_capped(Cursor::new(vec![0u8; 11]), 10)
            .unwrap_err()
            .to_string()
            .contains("more than"));
        assert_eq!(read_capped(Cursor::new(vec![0u8; 10]), 10).unwrap().len(), 10);
        assert!(is_gzip(&gzip(b"")) && is_zip(&zip_of(&[])) && !is_zip(b"PK"));
        // Entries each within their limit can still add up past the archive's.
        let mib = vec![b'x'; 1 << 20];
        let z = zip_of(&[("a.log", &mib), ("b.log", &mib), ("c.log", &mib)]);
        assert_eq!(expand_capped("ok.zip", z.clone(), 1 << 20, 3 << 20).unwrap().len(), 3);
        let e = expand_capped("bomb.zip", z, 1 << 20, 2 << 20).err().unwrap().to_string();
        assert_eq!(e, "archive expands to more than 2 MiB");
    }

    /// A zip naming `logs/a.log` twice: written with two names of one length, the second then renamed in its local and
    /// central headers (zip's writer refuses a repeated name).
    fn zip_twice(first: &[u8], second: &[u8]) -> Vec<u8> {
        let z = zip_of(&[("logs/a.log", first), ("logs/q.log", second)]);
        let mut out = z.clone();
        let (from, to) = (b"logs/q.log", b"logs/a.log");
        let at: Vec<usize> = z
            .windows(from.len())
            .enumerate()
            .filter(|(_, w)| w == from)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(at.len(), 2, "one local and one central header");
        for i in at {
            out[i..i + to.len()].copy_from_slice(to);
        }
        out
    }

    #[test]
    fn a_repeated_path_keeps_both_entries_and_the_copy_says_where_it_came_from() {
        let entries = expand("dup.zip", zip_twice(b"one", b"two")).unwrap();
        let got: Vec<_> = entries
            .iter()
            .map(|e| (e.name.as_deref().unwrap(), e.bytes.as_slice(), e.renamed_from.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                ("logs/a.log", &b"one"[..], None),
                ("logs/a (2).log", &b"two"[..], Some("logs/a.log"))
            ]
        );
    }

    #[test]
    fn the_limits_apply_to_every_entry_of_a_repeated_path() {
        let mib = vec![b'x'; 1 << 20];
        let big = vec![b'y'; (1 << 20) + 1];
        // The renamed copy is over the per-entry limit: the error names it.
        let e = format!(
            "{:#}",
            expand_capped("dup.zip", zip_twice(&mib, &big), 1 << 20, 4 << 20).err().unwrap()
        );
        assert!(e.contains("zip entry logs/a (2).log") && e.contains("more than 1 MiB"), "{e}");
        // Both copies count toward the archive's total.
        let e = expand_capped("dup.zip", zip_twice(&mib, &mib), 1 << 20, (1 << 20) + 10)
            .err()
            .unwrap()
            .to_string();
        assert_eq!(e, "archive expands to more than 1 MiB");
        assert_eq!(expand_capped("dup.zip", zip_twice(&mib, &mib), 1 << 20, 2 << 20).unwrap().len(), 2);
    }

    #[test]
    fn numbered_names_skip_taken_ones_and_keep_the_extension_and_folder() {
        let mut namer = Namer::new(["x (2).log".to_string()]);
        assert_eq!(namer.assign("x.log"), "x.log");
        assert_eq!(namer.assign("x.log"), "x (3).log");
        assert_eq!(namer.assign("x.log"), "x (4).log");
        let mut namer = Namer::new([]);
        for (path, copy) in [
            ("d/e/app.log.1", "d/e/app.log (2).1"),
            (".hidden", ".hidden (2)"),
            ("noext", "noext (2)"),
        ] {
            assert_eq!(namer.assign(path), path);
            assert_eq!(namer.assign(path), copy);
        }
    }

    /// One central directory file header (APPNOTE 4.3.12) with the given flags, 32-bit sizes and extra field.
    fn central_record(name: &[u8], flags: u16, sizes: [u32; 2], extra: &[u8]) -> Vec<u8> {
        let mut r = 0x0201_4b50u32.to_le_bytes().to_vec();
        for v in [20u16, 20, flags, 0, 0, 0x21] {
            r.extend(v.to_le_bytes());
        }
        for v in [0xdead_beef, sizes[0], sizes[1]] {
            r.extend(u32::to_le_bytes(v));
        }
        for v in [name.len() as u16, extra.len() as u16, 0, 0, 0] {
            r.extend(v.to_le_bytes());
        }
        r.extend([0u8; 4]);
        r.extend(7u32.to_le_bytes());
        r.extend(name);
        r.extend(extra);
        r
    }

    #[test]
    fn central_records_read_names_crc_and_sizes_and_bound_the_zip64_field() {
        // Names: UTF-8 with the flag, code page 437 without; `\` becomes `/`.
        let mut dir = central_record("d\\é.log".as_bytes(), 0x800, [3, 3], &[]);
        dir.extend(central_record(b"d\\\x82.log", 0, [4, 4], &[]));
        let got = central_records(&dir, 0, 0, 10).unwrap();
        let names: Vec<_> = got.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["d/é.log", "d/é.log"]);
        assert_eq!((got[0].crc, got[0].uncompressed, got[1].compressed), (0xdead_beef, 3, 4));
        // A zip64 field (id 1) holding all three 8-byte values: read; one byte short: an error, not a read past it.
        let mut full = 1u16.to_le_bytes().to_vec();
        full.extend(24u16.to_le_bytes());
        full.extend((0..24u8).map(|i| if i % 8 == 0 { 9 } else { 0 }));
        let ok = central_record(b"a", 0, [u32::MAX, u32::MAX], &full);
        let r = &central_records(&ok, 0, 0, 10).unwrap()[0];
        assert_eq!((r.uncompressed, r.compressed), (9, 9));
        let mut short = 1u16.to_le_bytes().to_vec();
        short.extend(15u16.to_le_bytes());
        short.extend([0u8; 15]);
        let mut trailing = central_record(b"a", 0, [u32::MAX, u32::MAX], &short);
        trailing.extend([0u8; 8]); // bytes after the record that a read past the field would reach
        let e = central_records(&trailing, 0, 0, 10).err().unwrap().to_string();
        assert!(e.contains("zip64"), "{e}");
        // More records than allowed.
        let two = [central_record(b"a", 0, [1, 1], &[]), central_record(b"b", 0, [1, 1], &[])].concat();
        assert!(central_records(&two, 0, 0, 1)
            .err()
            .unwrap()
            .to_string()
            .contains("more than 1 entries"));
    }

    #[test]
    fn too_many_entries_and_a_cancelled_expansion_stop_early() {
        let z = zip_twice(b"one", b"two");
        let limits = Limits {
            entries: 1,
            ..Limits::DEFAULT
        };
        let e = expand_with("dup.zip", z.clone(), limits, &AtomicBool::new(false))
            .err()
            .unwrap()
            .to_string();
        assert!(e.contains("more than 1 entries"), "{e}");
        let cancelled = AtomicBool::new(true);
        let e = expand_cancellable("dup.zip", z, &cancelled).err().unwrap();
        assert!(e.is::<ExpandCancelled>(), "{e:#}");
    }

    #[test]
    fn central_records_over_one_local_entry_are_refused() {
        let z = zip_twice(b"one", b"two");
        let central: Vec<usize> = z
            .windows(4)
            .enumerate()
            .filter(|(_, w)| *w == b"PK\x01\x02")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(central.len(), 3, "the folder, then the two files");
        // The last record points at the first file's local header: its bytes belong to two entries.
        let mut bad = z.clone();
        let first = z[central[1] + 42..central[1] + 46].to_vec();
        bad[central[2] + 42..central[2] + 46].copy_from_slice(&first);
        let e = expand("overlap.zip", bad).err().unwrap();
        assert!(format!("{e:#}").contains("overlap"), "{e:#}");
    }

    /// Regression (review of c9f283344): a streamed, deflated zip (flag bit 3, sizes and CRC-32 only after the data) with a
    /// repeated path failed as a whole with "Invalid checksum".
    #[test]
    fn a_repeated_path_in_a_streamed_deflated_zip_is_kept() {
        let mut w = zip::ZipWriter::new_stream(Vec::new());
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.start_file("logs/a.log", o).unwrap();
        w.write_all(b"first line\n").unwrap();
        w.start_file("logs/q.log", o).unwrap();
        w.write_all(b"second line\n").unwrap();
        let z = w.finish().unwrap().into_inner();
        assert_ne!(z[6] & 8, 0, "the writer sets the data descriptor flag");
        let mut dup = z.clone();
        for i in z
            .windows(10)
            .enumerate()
            .filter(|(_, w)| *w == b"logs/q.log")
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
        {
            dup[i..i + 10].copy_from_slice(b"logs/a.log");
        }
        let got: Vec<_> = expand("dup.zip", dup)
            .unwrap()
            .into_iter()
            .map(|e| (e.name.unwrap(), e.bytes))
            .collect();
        assert_eq!(
            got,
            [
                ("logs/a.log".to_string(), b"first line\n".to_vec()),
                ("logs/a (2).log".to_string(), b"second line\n".to_vec())
            ]
        );
    }

    #[test]
    fn a_damaged_central_directory_is_an_error_not_a_panic() {
        let z = zip_twice(b"one", b"two");
        // Cut inside the central directory: zip itself or the record walk refuses it.
        for cut in [z.len() - 30, z.len() - 60] {
            assert!(expand("cut.zip", z[..cut].to_vec()).is_err(), "cut at {cut}");
        }
        assert!(central_records(&z, u64::MAX, 0, MAX_ENTRIES).unwrap_or_default().is_empty());
    }
}
