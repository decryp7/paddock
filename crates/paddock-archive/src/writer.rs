//! The one place files are written: every entry from every reader goes through [`Writer::entry`].

use crate::control::Control;
use crate::limits;
use crate::error::{ArchiveError, Result, io};
use crate::name;
use crate::options::{Links, Modes, Options};
use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// What an archive entry is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    File,
    Dir,
    /// A symbolic link to this target text.
    Symlink(String),
    /// A hard link to this archive path.
    Hardlink(String),
    /// Devices, fifos, sockets and anything else: never created.
    Other,
}

/// What was unpacked.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// The files written, relative paths with `/`, sorted.
    pub files: Vec<String>,
    /// Folders created for entries (not counting the parents of files).
    pub dirs: usize,
    /// Bytes written (copies of links included).
    pub bytes: u64,
    /// Entries left out by the layout, the filter, `Links::Skip` or because they are special files.
    pub skipped: usize,
    /// Links whose target was not unpacked, so no copy could be made.
    pub dropped_links: Vec<String>,
}

pub(crate) struct Writer<'a> {
    root: &'a Path,
    opts: &'a Options,
    ctl: &'a dyn Control,
    entries: usize,
    seen: HashSet<String>,
    files: BTreeMap<String, PathBuf>,
    links: Vec<(String, Vec<String>)>,
    keep_links: Vec<(PathBuf, String)>,
    pub(crate) bytes: u64,
    cap: Cap,
    report: Report,
}

/// The most bytes that may be written, and which limit that is.
#[derive(Clone, Copy)]
pub(crate) struct Cap {
    pub bytes: u64,
    pub by_ratio: bool,
    pub limit: u64,
}

impl Cap {
    pub(crate) fn new(opts: &Options, archive_len: u64) -> Cap {
        let l = &opts.limits;
        let ratio = archive_len
            .saturating_mul(l.max_ratio)
            .max(l.ratio_floor_bytes);
        if ratio < l.max_total_bytes {
            Cap {
                bytes: ratio,
                by_ratio: true,
                limit: l.max_ratio,
            }
        } else {
            Cap {
                bytes: l.max_total_bytes,
                by_ratio: false,
                limit: l.max_total_bytes,
            }
        }
    }

    pub(crate) fn error(&self) -> ArchiveError {
        if self.by_ratio {
            limits::ratio(self.limit)
        } else {
            limits::total_bytes(self.limit)
        }
    }
}

const CHUNK: usize = 64 * 1024;

fn join(root: &Path, rel: &[String]) -> PathBuf {
    rel.iter().fold(root.to_path_buf(), |p, c| p.join(c))
}

impl<'a> Writer<'a> {
    pub(crate) fn new(
        root: &'a Path,
        opts: &'a Options,
        ctl: &'a dyn Control,
        cap: Cap,
    ) -> Writer<'a> {
        Writer {
            root,
            opts,
            ctl,
            entries: 0,
            seen: HashSet::new(),
            files: BTreeMap::new(),
            links: vec![],
            keep_links: vec![],
            bytes: 0,
            cap,
            report: Report::default(),
        }
    }

    /// Handles one entry. `size` is what the archive declares (only for progress and an early limit check); `r` yields the
    /// content, read at most `max_file_bytes + 1` bytes of it.
    pub(crate) fn entry(
        &mut self,
        raw: &str,
        kind: Kind,
        exec: bool,
        size: u64,
        r: &mut dyn Read,
    ) -> Result<()> {
        self.entries += 1;
        if self.entries > self.opts.limits.max_entries {
            return Err(limits::entries(self.opts.limits.max_entries as u64));
        }
        self.ctl.check()?;
        if kind == Kind::Other {
            // A device or fifo is refused only when it would be unpacked; its name is still validated.
            name::components(raw, &self.opts.limits)?;
            self.report.skipped += 1;
            return Ok(());
        }
        let comps = name::components(raw, &self.opts.limits)?;
        let Some(rel) = name::apply_layout(comps, self.opts.layout) else {
            self.report.skipped += 1;
            return Ok(());
        };
        let rel_text = rel.join("/");
        if let Some(keep) = &self.opts.filter
            && !keep(&rel_text)
        {
            self.report.skipped += 1;
            return Ok(());
        }
        match kind {
            Kind::Dir => self.dir(&rel),
            Kind::File => self.file(&rel, rel_text, exec, size, r),
            Kind::Symlink(target) => self.link(raw, &rel, rel_text, &target, false),
            Kind::Hardlink(target) => self.link(raw, &rel, rel_text, &target, true),
            Kind::Other => Ok(()),
        }
    }

    fn claim(&mut self, rel_text: &str) -> Result<()> {
        if !self.seen.insert(rel_text.to_lowercase()) {
            return Err(ArchiveError::Duplicate(rel_text.to_string()));
        }
        Ok(())
    }

    fn dir(&mut self, rel: &[String]) -> Result<()> {
        // A folder named twice is normal (explicit entry, then files in it); only a file/folder clash is an error, which
        // `create_dir_all` reports.
        let path = join(self.root, rel);
        std::fs::create_dir_all(&path).map_err(io("create folder", &path))?;
        self.report.dirs += 1;
        Ok(())
    }

    fn file(
        &mut self,
        rel: &[String],
        rel_text: String,
        exec: bool,
        size: u64,
        r: &mut dyn Read,
    ) -> Result<()> {
        let limits = &self.opts.limits;
        if size > limits.max_file_bytes {
            return Err(limits::file_bytes(limits.max_file_bytes));
        }
        self.claim(&rel_text)?;
        let path = join(self.root, rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io("create folder", parent))?;
        }
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(io("write", &path))?;
        let mut r = r.take(limits.max_file_bytes.saturating_add(1));
        let mut buf = vec![0u8; CHUNK];
        let mut n = 0u64;
        loop {
            self.ctl.check()?;
            let got = r
                .read(&mut buf)
                .map_err(|e| ArchiveError::Corrupt(e.to_string()))?;
            if got == 0 {
                break;
            }
            n += got as u64;
            if n > limits.max_file_bytes {
                return Err(limits::file_bytes(limits.max_file_bytes));
            }
            if self.bytes + n > self.cap.bytes {
                return Err(self.cap.error());
            }
            f.write_all(&buf[..got]).map_err(io("write", &path))?;
        }
        f.flush().map_err(io("write", &path))?;
        drop(f);
        self.bytes += n;
        set_mode(&path, exec || self.opts.modes == Modes::AllExecutable);
        self.files.insert(rel_text, path);
        Ok(())
    }

    fn link(
        &mut self,
        raw: &str,
        rel: &[String],
        rel_text: String,
        target: &str,
        hard: bool,
    ) -> Result<()> {
        match self.opts.links {
            Links::Refuse => {
                return Err(ArchiveError::Unsafe {
                    entry: raw.chars().take(200).collect(),
                    reason: "is a link, and links are not allowed here".into(),
                });
            }
            Links::Skip => {
                self.report.skipped += 1;
                return Ok(());
            }
            Links::Copy | Links::Keep => {}
        }
        let keep_on_disk = self.opts.links == Links::Keep && !hard && cfg!(unix);
        let dir = &rel[..rel.len() - 1];
        let resolved = if hard {
            // A hard link names a path of the archive, to which the same layout applies.
            let comps = name::components(target, &self.opts.limits)?;
            name::apply_layout(comps, self.opts.layout)
        } else {
            Some(name::link_target(
                raw,
                target,
                dir,
                keep_on_disk,
                &self.opts.limits,
            )?)
        };
        self.claim(&rel_text)?;
        let Some(resolved) = resolved else {
            self.report.dropped_links.push(rel_text);
            return Ok(());
        };
        if keep_on_disk {
            let path = join(self.root, rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(io("create folder", parent))?;
            }
            self.keep_links.push((path, target.to_string()));
            self.files.insert(rel_text, PathBuf::new());
        } else {
            self.links.push((rel_text, resolved));
        }
        Ok(())
    }

    /// Makes the copies for links (a link to a link resolves in rounds), creates the kept symbolic links and returns the report.
    pub(crate) fn finish(mut self) -> Result<Report> {
        for _ in 0..8 {
            let pending = std::mem::take(&mut self.links);
            let before = pending.len();
            for (name, target) in pending {
                let target_text = target.join("/");
                match self
                    .files
                    .get(&target_text)
                    .filter(|p| !p.as_os_str().is_empty())
                    .cloned()
                {
                    Some(from) => {
                        let to = join(
                            self.root,
                            &name.split('/').map(str::to_string).collect::<Vec<_>>(),
                        );
                        if let Some(parent) = to.parent() {
                            std::fs::create_dir_all(parent).map_err(io("create folder", parent))?;
                        }
                        let n = std::fs::copy(&from, &to).map_err(io("copy", &to))?;
                        self.bytes += n;
                        if self.bytes > self.cap.bytes {
                            return Err(self.cap.error());
                        }
                        self.files.insert(name, to);
                    }
                    None => self.links.push((name, target)),
                }
            }
            if self.links.is_empty() || self.links.len() == before {
                break;
            }
        }
        for (name, _) in std::mem::take(&mut self.links) {
            self.report.dropped_links.push(name);
        }
        #[cfg(unix)]
        for (path, target) in std::mem::take(&mut self.keep_links) {
            std::os::unix::fs::symlink(&target, &path).map_err(io("create link", &path))?;
        }
        self.report.files = self.files.into_keys().collect();
        self.report.bytes = self.bytes;
        Ok(self.report)
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, exec: bool) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(if exec { 0o755 } else { 0o644 }),
    );
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: bool) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::NoControl;
    use crate::options::{Layout, Limits, Links};

    /// A reader that yields more than the entry declared (a lying zip or tar).
    struct Endless;

    impl Read for Endless {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            buf.fill(7);
            Ok(buf.len())
        }
    }

    #[test]
    fn an_entry_that_outgrows_its_declared_size_hits_the_file_limit() {
        let d = tempfile::tempdir().unwrap();
        let opts = Options {
            limits: Limits {
                max_file_bytes: 100_000,
                ..Limits::default()
            },
            ..Options::default()
        };
        let mut w = Writer::new(d.path(), &opts, &NoControl, Cap::new(&opts, 1 << 30));
        let e = w
            .entry("liar", Kind::File, false, 0, &mut Endless)
            .unwrap_err();
        assert!(
            matches!(
                &e,
                ArchiveError::Limit(l) if l.id.as_str() == "archive.file_bytes"
                    && l.limit == paddock_limits::Quantity::Bytes(100_000)
            ),
            "{e:?}"
        );
    }

    #[test]
    fn a_hard_link_to_an_entry_the_layout_leaves_out_is_dropped() {
        let d = tempfile::tempdir().unwrap();
        let opts = Options {
            links: Links::Copy,
            layout: Layout::Flat { max_depth: 1 },
            ..Options::default()
        };
        let mut w = Writer::new(d.path(), &opts, &NoControl, Cap::new(&opts, 1 << 30));
        w.entry(
            "l",
            Kind::Hardlink("a/b/deep".into()),
            false,
            0,
            &mut std::io::empty(),
        )
        .unwrap();
        let r = w.finish().unwrap();
        assert_eq!((r.files, r.dropped_links), (vec![], vec!["l".to_string()]));
    }
}
