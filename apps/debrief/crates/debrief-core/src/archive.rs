//! Transparent reading of compressed input: gzip files and zip archives are
//! detected by their magic bytes (not their extension) and expanded into the
//! files they contain. Decompressed sizes are capped so a hostile archive
//! (a "zip bomb") fails with a message instead of exhausting memory.

use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashSet;
use std::io::{Cursor, Read};

/// Largest decompressed size of one entry (1 GiB).
pub const MAX_ENTRY_BYTES: u64 = 1 << 30;
/// Largest total decompressed size of one archive (4 GiB).
pub const MAX_ARCHIVE_BYTES: u64 = 4 << 30;
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
/// name a real entry of the archive has. Such an entry carries its own path in [`Entry::renamed_from`]. The user decided
/// this on 2026-10-29 (docs/plans/debrief-1-2.md, section 6). The per-entry and per-archive limits apply to every entry,
/// renamed or not.
pub fn expand(file_name: &str, bytes: Vec<u8>) -> Result<Vec<Entry>> {
    expand_capped(file_name, bytes, MAX_ENTRY_BYTES, MAX_ARCHIVE_BYTES)
}

/// [`expand`] with explicit limits per entry and per archive.
fn expand_capped(file_name: &str, bytes: Vec<u8>, max_entry: u64, max_archive: u64) -> Result<Vec<Entry>> {
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
        let records = central_records(&bytes, zip.central_directory_start(), zip.offset())?;
        if records.len() != zip.len() {
            return expand_repeated(&bytes, &zip, &records, max_entry, max_archive);
        }
        drop(zip);
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).context("invalid zip archive")?;
        let mut out = Vec::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let entry = zip.by_index(i).context("invalid zip entry")?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().context("invalid zip entry name")?.replace('\\', "/");
            let data = read_capped(entry, max_entry).with_context(|| format!("zip entry {name}"))?;
            total += data.len() as u64;
            if total > max_archive {
                return Err(anyhow!("archive expands to more than {} MiB", max_archive >> 20));
            }
            out.push(Entry {
                name: Some(name),
                bytes: data,
                renamed_from: None,
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

/// One file header of a zip's central directory: where its local header is and how many compressed bytes follow it.
struct Record {
    local_header: u64,
    compressed: u64,
}

/// Every record of the central directory at `start`, repeated names included (zip's own index keeps one per name).
/// `offset` is the size of data before the archive (the local header offsets are relative to the archive's start).
/// Damaged or truncated records are an error, never a panic.
fn central_records(bytes: &[u8], start: u64, offset: u64) -> Result<Vec<Record>> {
    const CENTRAL: u32 = 0x0201_4b50;
    const FIXED: usize = 46;
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
        let field = |rel: usize, read: &dyn Fn(usize) -> Option<u64>| {
            at.checked_add(rel)
                .and_then(read)
                .ok_or_else(|| anyhow!("the zip's central directory is truncated"))
        };
        let (mut uncompressed, mut compressed) = (field(24, &u32_at)?, field(20, &u32_at)?);
        let mut local_header = field(42, &u32_at)?;
        let (name_len, extra_len, comment_len) = (
            field(28, &u16_at)? as usize,
            field(30, &u16_at)? as usize,
            field(32, &u16_at)? as usize,
        );
        // Zip64 (APPNOTE 4.5.3): the values that did not fit are in the extra field 0x0001, in this order.
        let mut extra = at.saturating_add(FIXED + name_len);
        let extra_end = extra.saturating_add(extra_len);
        while extra + 4 <= extra_end {
            let (id, len) = (field(extra - at, &u16_at)?, field(extra + 2 - at, &u16_at)? as usize);
            if id == 1 {
                let mut v = extra + 4;
                for value in [&mut uncompressed, &mut compressed, &mut local_header] {
                    if *value == 0xFFFF_FFFF {
                        *value = u64_at(v).ok_or_else(|| anyhow!("the zip's central directory is truncated"))?;
                        v = v.saturating_add(8);
                    }
                }
            }
            extra = extra.saturating_add(4 + len);
        }
        let _ = uncompressed;
        records.push(Record {
            local_header: local_header.saturating_add(offset),
            compressed,
        });
        at = extra_end.saturating_add(comment_len);
    }
    Ok(records)
}

/// [`expand_capped`] for a zip whose central directory names a path more than once: each entry is read from its own
/// local header, and repeated paths are renamed (see [`expand`]).
fn expand_repeated(
    bytes: &[u8],
    zip: &zip::ZipArchive<Cursor<&[u8]>>,
    records: &[Record],
    max_entry: u64,
    max_archive: u64,
) -> Result<Vec<Entry>> {
    let mut taken: HashSet<String> = zip.file_names().filter_map(Result::ok).map(|n| n.replace('\\', "/")).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut total = 0u64;
    for r in records {
        let from = usize::try_from(r.local_header)
            .ok()
            .and_then(|at| bytes.get(at..))
            .ok_or_else(|| anyhow!("invalid zip entry: its header is outside the archive"))?;
        let mut reader = Cursor::new(from);
        let Some(entry) =
            zip::read::read_zipfile_from_stream_with_compressed_size(&mut reader, r.compressed).context("invalid zip entry")?
        else {
            bail!("invalid zip entry: no file header where the central directory points");
        };
        if entry.is_dir() {
            continue;
        }
        let own = entry.name().context("invalid zip entry name")?.replace('\\', "/");
        let name = if seen.insert(own.clone()) {
            own.clone()
        } else {
            numbered(&own, &mut taken)
        };
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
    Ok(out)
}

/// The first `<stem> (n)<.ext>` (n = 2, 3, ...) in the folder of `path` that no entry has taken; it is taken now.
fn numbered(path: &str, taken: &mut HashSet<String>) -> String {
    let (folder, file) = path.rsplit_once('/').map_or(("", path), |(d, f)| (d, f));
    let (stem, ext) = match file.rfind('.') {
        Some(dot) if dot > 0 => file.split_at(dot),
        _ => (file, ""),
    };
    let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
    let mut n = 2u64;
    loop {
        let candidate = format!("{prefix}{stem} ({n}){ext}");
        if taken.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
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
        let mut taken: HashSet<String> = ["x (2).log".to_string()].into();
        assert_eq!(numbered("x.log", &mut taken), "x (3).log");
        assert_eq!(numbered("x.log", &mut taken), "x (4).log");
        assert_eq!(numbered("d/e/app.log.1", &mut taken), "d/e/app.log (2).1");
        assert_eq!(numbered(".hidden", &mut taken), ".hidden (2)");
        assert_eq!(numbered("noext", &mut taken), "noext (2)");
    }

    #[test]
    fn a_damaged_central_directory_is_an_error_not_a_panic() {
        let z = zip_twice(b"one", b"two");
        // Cut inside the central directory: zip itself or the record walk refuses it.
        for cut in [z.len() - 30, z.len() - 60] {
            let _ = expand("cut.zip", z[..cut].to_vec());
        }
        assert!(central_records(&z, u64::MAX, 0).unwrap_or_default().is_empty());
    }
}
