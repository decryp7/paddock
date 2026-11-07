//! The format readers: each walks its archive and hands every entry to the [`Writer`].

use crate::control::Control;
use crate::error::{ArchiveError, Result, io};
use crate::limits;
use crate::options::Options;
use crate::writer::{Cap, Kind, Report, Writer};
use std::cell::Cell;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// An archive format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `.zip`
    Zip,
    /// `.tar`
    Tar,
    /// `.tar.gz`, `.tgz`
    TarGz,
    /// `.tar.xz`
    TarXz,
}

impl Format {
    /// Recognises the format from the file's first bytes (the name is not trusted: a download may be misnamed).
    pub fn sniff(path: &Path) -> Result<Format> {
        let mut f = std::fs::File::open(path).map_err(io("open", path))?;
        let mut head = [0u8; 6];
        let n = read_up_to(&mut f, &mut head).map_err(io("read", path))?;
        let head = &head[..n];
        if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
            return Ok(Format::Zip);
        }
        if head.starts_with(&[0x1f, 0x8b]) {
            return Ok(Format::TarGz);
        }
        if head.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0]) {
            return Ok(Format::TarXz);
        }
        let mut magic = [0u8; 5];
        f.seek(SeekFrom::Start(257)).map_err(io("read", path))?;
        if read_up_to(&mut f, &mut magic).map_err(io("read", path))? == 5 && &magic == b"ustar" {
            return Ok(Format::Tar);
        }
        Err(ArchiveError::Unsupported(format!(
            "{} is not a .zip, .tar, .tar.gz or .tar.xz archive. Download it again, or pick one of those.",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("the file")
        )))
    }
}

fn read_up_to(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// Counts the bytes pulled through it: the compressed position, which is what progress measures for streams.
struct Counting<R> {
    inner: R,
    pos: Rc<Cell<u64>>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.pos.set(self.pos.get() + n as u64);
        Ok(n)
    }
}

/// Reports progress that never goes backwards.
struct Monotonic<'a> {
    ctl: &'a dyn Control,
    last: Cell<u64>,
}

impl Monotonic<'_> {
    fn set(&self, done: u64) {
        if done > self.last.get() {
            self.last.set(done);
            self.ctl.progress(done);
        }
    }
}

/// Unpacks `archive` (of `format`) into the existing, empty folder `root`; `tmp` is where a `.tar.xz` is expanded first.
pub(crate) fn read_into(
    archive: &Path,
    format: Format,
    root: &Path,
    tmp: &Path,
    opts: &Options,
    ctl: &dyn Control,
) -> Result<Report> {
    let len = std::fs::metadata(archive)
        .map_err(io("read", archive))?
        .len();
    let cap = Cap::new(opts, len);
    match format {
        Format::Zip => zip_into(archive, root, opts, ctl, cap),
        Format::Tar => tar_into(
            std::fs::File::open(archive).map_err(io("open", archive))?,
            len,
            root,
            opts,
            ctl,
            cap,
            |f| Box::new(f),
        ),
        Format::TarGz => tar_into(
            std::fs::File::open(archive).map_err(io("open", archive))?,
            len,
            root,
            opts,
            ctl,
            cap,
            |f| Box::new(flate2::read::GzDecoder::new(f)),
        ),
        Format::TarXz => {
            let expanded = unxz(archive, tmp, opts, ctl, cap)?;
            let _guard = RemoveOnDrop(expanded.clone());
            let elen = std::fs::metadata(&expanded)
                .map_err(io("read", &expanded))?
                .len();
            tar_into(
                std::fs::File::open(&expanded).map_err(io("open", &expanded))?,
                elen,
                root,
                opts,
                ctl,
                cap,
                |f| Box::new(f),
            )
        }
    }
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A writer that stops at a size and checks for cancel, so a decompression bomb ends early.
struct Limited<'a> {
    inner: std::fs::File,
    written: u64,
    cap: u64,
    err: ArchiveError,
    ctl: &'a dyn Control,
}

impl Write for Limited<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.ctl.check().is_err() {
            return Err(std::io::Error::other("cancelled"));
        }
        self.written += buf.len() as u64;
        if self.written > self.cap {
            return Err(std::io::Error::other("too large"));
        }
        self.inner.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn unxz(
    archive: &Path,
    tmp: &Path,
    opts: &Options,
    ctl: &dyn Control,
    cap: Cap,
) -> Result<PathBuf> {
    ctl.phase("expand", "Expanding", None);
    let file = std::fs::File::open(archive).map_err(io("open", archive))?;
    let out = std::fs::File::create(tmp).map_err(io("write", tmp))?;
    let guard = RemoveOnDrop(tmp.to_path_buf());
    // Tar adds a 512-byte header per entry and padding on top of the content.
    let tar_overhead = (opts.limits.max_entries as u64)
        .saturating_mul(1024)
        .saturating_add(1024);
    let mut w = Limited {
        inner: out,
        written: 0,
        cap: cap.bytes.saturating_add(tar_overhead),
        err: cap.error(),
        ctl,
    };
    let result = lzma_rs::xz_decompress(&mut BufReader::new(file), &mut w);
    match result {
        Ok(()) => {
            w.flush().map_err(io("write", tmp))?;
            std::mem::forget(guard);
            Ok(tmp.to_path_buf())
        }
        Err(e) => {
            drop(guard);
            ctl.check()?;
            if w.written > w.cap {
                Err(w.err)
            } else {
                Err(ArchiveError::Corrupt(format!("{e}")))
            }
        }
    }
}

fn tar_into(
    file: std::fs::File,
    len: u64,
    root: &Path,
    opts: &Options,
    ctl: &dyn Control,
    cap: Cap,
    wrap: impl FnOnce(Counting<std::fs::File>) -> Box<dyn Read>,
) -> Result<Report> {
    ctl.phase("unpack", "Unpacking", Some(len));
    let pos = Rc::new(Cell::new(0));
    let progress = Monotonic {
        ctl,
        last: Cell::new(0),
    };
    let counted = Counting {
        inner: file,
        pos: pos.clone(),
    };
    let mut tar = tar::Archive::new(BufReader::new(wrap(counted)));
    let mut w = Writer::new(root, opts, ctl, cap);
    let corrupt = |e: std::io::Error| ArchiveError::Corrupt(e.to_string());
    for entry in tar.entries().map_err(corrupt)? {
        let mut entry = entry.map_err(corrupt)?;
        let raw = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let t = entry.header().entry_type();
        let exec = entry
            .header()
            .mode()
            .map(|m| m & 0o111 != 0)
            .unwrap_or(false);
        let link = |e: &mut tar::Entry<'_, _>| {
            e.link_name_bytes()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default()
        };
        let kind = if t.is_file() {
            Kind::File
        } else if t.is_dir() {
            Kind::Dir
        } else if t.is_symlink() {
            Kind::Symlink(link(&mut entry))
        } else if t.is_hard_link() {
            Kind::Hardlink(link(&mut entry))
        } else {
            Kind::Other
        };
        let size = entry.size();
        w.entry(&raw, kind, exec, size, &mut entry)?;
        progress.set(pos.get().min(len));
    }
    let report = w.finish()?;
    progress.set(len);
    Ok(report)
}

fn zip_into(
    archive: &Path,
    root: &Path,
    opts: &Options,
    ctl: &dyn Control,
    cap: Cap,
) -> Result<Report> {
    let corrupt = |e: zip::result::ZipError| ArchiveError::Corrupt(e.to_string());
    let file = std::fs::File::open(archive).map_err(io("open", archive))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file)).map_err(corrupt)?;
    if zip.len() > opts.limits.max_entries {
        return Err(limits::entries(opts.limits.max_entries as u64));
    }
    let mut total = 0u64;
    for i in 0..zip.len() {
        total = total.saturating_add(zip.by_index_raw(i).map_err(corrupt)?.size());
    }
    ctl.phase("unpack", "Unpacking", Some(total));
    let progress = Monotonic {
        ctl,
        last: Cell::new(0),
    };
    let mut w = Writer::new(root, opts, ctl, cap);
    let mut done = 0u64;
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).map_err(corrupt)?;
        let raw = e.name().map_err(corrupt)?.into_owned();
        let size = e.size();
        let exec = e.unix_mode().is_some_and(|m| m & 0o111 != 0);
        let kind = if e.is_dir() {
            Kind::Dir
        } else if e.is_symlink() {
            let mut target = String::new();
            (&mut e)
                .take(4096)
                .read_to_string(&mut target)
                .map_err(|x| ArchiveError::Corrupt(x.to_string()))?;
            Kind::Symlink(target)
        } else {
            Kind::File
        };
        w.entry(&raw, kind, exec, size, &mut e)?;
        done = done.saturating_add(size);
        progress.set(done.min(total));
    }
    let report = w.finish()?;
    progress.set(total);
    Ok(report)
}
