//! Bounded reading of log files: a report never reads a whole log. The newest bytes of the newest files are taken, from
//! the end of each file (a seek, so a 1 GB file costs the same as a small one), starting at a line boundary.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The default log bytes in a report, over all files.
pub const LOG_WINDOW: u64 = 2 * 1024 * 1024;
/// Most log files looked at.
pub const MAX_LOG_FILES: usize = 4;

/// The last `max` bytes of `path`, cut forward to the next line start when the read began inside a line; `None` when the
/// file cannot be read. Returns the bytes and whether the file was longer.
pub fn tail_of(path: &Path, max: u64) -> Option<(Vec<u8>, bool)> {
    let file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    tail_of_reader(file, len, max)
}

/// [`tail_of`] over any seekable source `len` bytes long: one seek, then at most `max` bytes read.
fn tail_of_reader(mut source: impl Read + Seek, len: u64, max: u64) -> Option<(Vec<u8>, bool)> {
    let start = len.saturating_sub(max);
    source.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity((len - start).min(max) as usize);
    source.take(max).read_to_end(&mut bytes).ok()?;
    if start > 0 {
        // Began inside a line (or a character): drop up to the first newline.
        match bytes.iter().position(|b| *b == b'\n') {
            Some(i) => {
                bytes.drain(..=i);
            }
            None => bytes.clear(),
        }
    }
    Some((bytes, start > 0))
}

/// The files of `dir` named `<prefix>...` with the extension `.log`, newest first (by modified time, then name).
pub fn log_files(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    name.starts_with(prefix) && name.ends_with(".log") && e.path().is_file()
                })
                .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                .collect()
        })
        .unwrap_or_default();
    found.sort_by(|(ta, pa), (tb, pb)| tb.cmp(ta).then_with(|| pb.cmp(pa)));
    found.into_iter().map(|(_, p)| p).collect()
}

/// A window over the newest log files: `(file name, bytes, cut)` newest first. At most [`MAX_LOG_FILES`] files and `budget`
/// bytes in all; a file that cannot be read is skipped.
pub fn log_window(dir: &Path, prefix: &str, budget: u64) -> Vec<(String, Vec<u8>, bool)> {
    let mut left = budget;
    let mut out = Vec::new();
    for path in log_files(dir, prefix).into_iter().take(MAX_LOG_FILES) {
        if left == 0 {
            break;
        }
        if let Some((bytes, cut)) = tail_of(&path, left).filter(|(b, _)| !b.is_empty()) {
            left = left.saturating_sub(bytes.len() as u64);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push((name, bytes, cut));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_small_file_is_whole_and_a_cut_starts_at_a_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.log");
        std::fs::write(&p, "one\ntwo\nthree\n").unwrap();
        assert_eq!(
            tail_of(&p, 100).unwrap(),
            (b"one\ntwo\nthree\n".to_vec(), false)
        );
        // 8 bytes: "o\nthree\n" begins inside "two"; the partial line is dropped.
        assert_eq!(tail_of(&p, 8).unwrap(), (b"three\n".to_vec(), true));
        // A single huge line with no newline inside the window leaves nothing (never a half line).
        std::fs::write(&p, "x".repeat(50)).unwrap();
        assert_eq!(tail_of(&p, 10).unwrap(), (Vec::new(), true));
        assert!(tail_of(&dir.path().join("missing.log"), 10).is_none());
    }

    /// A log of any length made up on the fly (zeros, then `tail`), counting every byte read: no disk, no allocation of its
    /// length, so the test costs the same on every file system (NTFS does not make `set_len` files sparse).
    struct Synthetic {
        len: u64,
        tail: &'static [u8],
        at: u64,
        read: u64,
    }

    impl Read for Synthetic {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(self.len.saturating_sub(self.at) as usize);
            let tail_from = self.len - self.tail.len() as u64;
            for (i, b) in buf[..n].iter_mut().enumerate() {
                let pos = self.at + i as u64;
                *b = if pos >= tail_from {
                    self.tail[(pos - tail_from) as usize]
                } else {
                    0
                };
            }
            self.at += n as u64;
            self.read += n as u64;
            Ok(n)
        }
    }

    impl Seek for Synthetic {
        fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
            // `tail_of_reader` seeks only from the start.
            if let SeekFrom::Start(at) = to {
                self.at = at;
            }
            Ok(self.at)
        }
    }

    #[test]
    fn a_one_gigabyte_log_is_read_from_the_end_only() {
        let mut log = Synthetic {
            len: 1 << 30,
            tail: b"\nlast-1\nlast-2\n",
            at: 0,
            read: 0,
        };
        let (bytes, cut) = tail_of_reader(&mut log, 1 << 30, 1024 * 1024).unwrap();
        assert!(cut);
        assert!(bytes.ends_with(b"last-1\nlast-2\n"));
        // A seek, not a scan: the bytes read are the window, whatever the length (counted, never timed).
        assert_eq!(log.read, 1024 * 1024);
    }

    #[test]
    fn a_file_is_read_through_the_same_tail() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.log");
        let mut f = File::create(&p).unwrap();
        f.write_all(&[b'x'; 4096]).unwrap();
        f.write_all(b"\nlast-1\nlast-2\n").unwrap();
        drop(f);
        let (bytes, cut) = tail_of(&p, 1024).unwrap();
        assert!(cut);
        assert_eq!(bytes, b"last-1\nlast-2\n");
    }

    #[test]
    fn the_window_takes_the_newest_files_within_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        for (i, name) in [
            "app.2026-10-22.log",
            "app.2026-10-23.log",
            "app.2026-10-24.log",
        ]
        .iter()
        .enumerate()
        {
            let p = dir.path().join(name);
            std::fs::write(&p, format!("first{i}\nsecond{i}\nthird{i}\n")).unwrap();
            let t = std::time::SystemTime::UNIX_EPOCH
                + std::time::Duration::from_secs(1_000 + i as u64 * 100);
            File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
        std::fs::write(dir.path().join("other.2026-10-24.log"), "not ours\n").unwrap();
        std::fs::write(dir.path().join("app.notes.txt"), "no\n").unwrap();
        let names: Vec<String> = log_window(dir.path(), "app", 10_000)
            .into_iter()
            .map(|(n, _, _)| n)
            .collect();
        assert_eq!(
            names,
            [
                "app.2026-10-24.log",
                "app.2026-10-23.log",
                "app.2026-10-22.log"
            ]
        );
        // A budget for one file and a bit: the newest whole, the next cut to its last whole line; a file that would add nothing is left out.
        let w = log_window(dir.path(), "app", 30);
        assert_eq!(
            w.iter()
                .map(|(n, _, c)| (n.as_str(), *c))
                .collect::<Vec<_>>(),
            [("app.2026-10-24.log", false), ("app.2026-10-23.log", true)]
        );
        assert_eq!(String::from_utf8_lossy(&w[1].1), "third1\n");
        assert!(w.iter().map(|(_, b, _)| b.len()).sum::<usize>() <= 30);
        assert!(log_window(dir.path(), "app", 0).is_empty());
        assert!(log_window(&dir.path().join("nope"), "app", 100).is_empty());
    }
}
