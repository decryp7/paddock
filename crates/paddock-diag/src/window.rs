//! Bounded reading of log files: a report never reads a whole log. The newest bytes of the newest files are taken, from
//! the end of each file (a seek, so a 1 GB file costs the same as a small one), starting at a line boundary.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The default log bytes in a report, over all files.
pub const LOG_WINDOW: u64 = 4 * 1024 * 1024;
/// Most log files looked at.
pub const MAX_LOG_FILES: usize = 4;

/// The last `max` bytes of `path`, cut forward to the next line start when the read began inside a line; `None` when the
/// file cannot be read. Returns the bytes and whether the file was longer.
pub fn tail_of(path: &Path, max: u64) -> Option<(Vec<u8>, bool)> {
    try_tail(path, max).ok()
}

/// [`tail_of`] with the reason a file could not be read.
pub fn try_tail(path: &Path, max: u64) -> std::io::Result<(Vec<u8>, bool)> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    tail_of_reader(file, len, max)
}

/// [`tail_of`] over any seekable source `len` bytes long: one seek, then at most `max` bytes read.
fn tail_of_reader(
    mut source: impl Read + Seek,
    len: u64,
    max: u64,
) -> std::io::Result<(Vec<u8>, bool)> {
    let start = len.saturating_sub(max);
    source.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((len - start).min(max) as usize);
    source.take(max).read_to_end(&mut bytes)?;
    if start > 0 {
        // Began inside a line (or a character): drop up to the first newline. With no newline in the window the window is the
        // tail of one long line (a log whose last line has no newline yet): it is kept, never dropped.
        if let Some(i) = bytes.iter().position(|b| *b == b'\n') {
            bytes.drain(..=i);
        }
    }
    Ok((bytes, start > 0))
}

/// The files of `dir` named `<prefix>...` with the extension `.log`, newest first (by modified time, then name).
pub fn log_files(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    try_log_files(dir, prefix).unwrap_or_default()
}

/// [`log_files`] with the reason the folder could not be listed (a missing folder is an error too: the caller tells the two
/// apart by [`std::io::ErrorKind::NotFound`]).
pub fn try_log_files(dir: &Path, prefix: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)?
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with(prefix) && name.ends_with(".log") && e.path().is_file()
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    found.sort_by(|(ta, pa), (tb, pb)| tb.cmp(ta).then_with(|| pb.cmp(pa)));
    Ok(found.into_iter().map(|(_, p)| p).collect())
}

/// What [`log_window_report`] found.
#[derive(Debug, Default)]
pub struct LogWindow {
    /// `(file name, bytes, cut)`, as [`log_window`] returns.
    pub files: Vec<(String, Vec<u8>, bool)>,
    /// Files that exist but could not be read: `(name, why)`.
    pub skipped: Vec<(String, String)>,
    /// Why the folder could not be listed, when it could not and exists (a missing folder is just "no logs yet").
    pub folder_error: Option<String>,
}

/// A window over the newest log files: `(file name, bytes, cut)`, each process's files newest first. At most [`MAX_LOG_FILES`]
/// files per process and `budget` bytes in all, shared between the processes (`<id>.<day>.log` for the app, `<id>-<process>.<day>.log`
/// for the others): each gets an equal share, and what a small one does not use goes to the larger ones, so a chatty process
/// cannot push another's log out of the report. A file that cannot be read is skipped.
pub fn log_window(dir: &Path, prefix: &str, budget: u64) -> Vec<(String, Vec<u8>, bool)> {
    log_window_report(dir, prefix, budget).files
}

/// [`log_window`] that also says what it could not read, so a report can tell "no logs yet" from "logs I could not open".
pub fn log_window_report(dir: &Path, prefix: &str, budget: u64) -> LogWindow {
    let mut report = LogWindow::default();
    let listed = match try_log_files(dir, prefix) {
        Ok(files) => files,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            report.folder_error = Some(format!("{:?}", e.kind()));
            Vec::new()
        }
    };
    // The files by process, in the order each process's newest file appears.
    let mut groups: Vec<(String, Vec<PathBuf>)> = Vec::new();
    for path in listed {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let process = name.split('.').next().unwrap_or("").to_string();
        match groups.iter_mut().find(|(p, _)| *p == process) {
            Some((_, files)) => {
                if files.len() < MAX_LOG_FILES {
                    files.push(path);
                }
            }
            None => groups.push((process, vec![path])),
        }
    }
    // The smallest processes first, each taking its equal share of what is left (or all it has).
    let size = |files: &[PathBuf]| {
        files
            .iter()
            .filter_map(|p| p.metadata().ok())
            .map(|m| m.len())
            .sum::<u64>()
    };
    let mut order: Vec<usize> = (0..groups.len()).collect();
    order.sort_by_key(|i| size(&groups[*i].1));
    let mut left = budget;
    let mut taken: Vec<Vec<(String, Vec<u8>, bool)>> = vec![Vec::new(); groups.len()];
    for (k, i) in order.iter().enumerate() {
        let share = left / (groups.len() - k) as u64;
        let mut room = share;
        for path in &groups[*i].1 {
            if room == 0 {
                break;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match try_tail(path, room) {
                Ok((bytes, cut)) if !bytes.is_empty() => {
                    room = room.saturating_sub(bytes.len() as u64);
                    taken[*i].push((name, bytes, cut));
                }
                Ok(_) => {}
                Err(e) => report.skipped.push((name, format!("{:?}", e.kind()))),
            }
        }
        left -= share - room;
    }
    report.files = taken.into_iter().flatten().collect();
    report
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
        // A tail with no newline in it is the end of one long line: kept, marked cut (a log's unfinished last line is evidence).
        std::fs::write(&p, "x".repeat(50)).unwrap();
        assert_eq!(
            tail_of(&p, 10).unwrap(),
            ("x".repeat(10).into_bytes(), true)
        );
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

    /// Logging for root cause (Dyno's 30 s login connect): the app writes far more than its background process, and the window
    /// used to go to the newest files first, so a chatty app log pushed the agent's out of the report. Each process (`<id>`,
    /// `<id>-agent`, ...) gets an equal share; what a small one does not use goes to the others.
    #[test]
    fn each_process_gets_a_share_of_the_window_and_a_chatty_one_cannot_push_another_out() {
        let dir = tempfile::tempdir().unwrap();
        let big = "a line of the chatty app log\n".repeat(400); // 11 600 bytes
        std::fs::write(dir.path().join("dyno.2026-10-31.log"), &big).unwrap();
        std::fs::write(
            dir.path().join("dyno-agent.2026-10-31.log"),
            "agent start\nagent bound\n",
        )
        .unwrap();
        // The agent's log is the OLDER file, so the chatty app's group comes first. Only the smallest-first rule then lets the small
        // log keep all its bytes and hand the rest of its share to the app (without it the app would stop at half the window).
        for (name, secs) in [
            ("dyno-agent.2026-10-31.log", 1_000),
            ("dyno.2026-10-31.log", 2_000),
        ] {
            File::options()
                .write(true)
                .open(dir.path().join(name))
                .unwrap()
                .set_modified(
                    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs),
                )
                .unwrap();
        }
        let got = log_window(dir.path(), "dyno", 4_000);
        let by_name = |n: &str| {
            got.iter()
                .find(|(name, _, _)| name == n)
                .unwrap_or_else(|| {
                    panic!(
                        "{n} missing from {:?}",
                        got.iter().map(|g| &g.0).collect::<Vec<_>>()
                    )
                })
        };
        assert_eq!(
            by_name("dyno-agent.2026-10-31.log").1,
            b"agent start\nagent bound\n",
            "the small log is whole"
        );
        let app = by_name("dyno.2026-10-31.log");
        assert!(app.2, "the chatty log is cut");
        // The share the agent did not use goes to the app: the report is as full as the budget allows.
        let total: usize = got.iter().map(|g| g.1.len()).sum();
        assert!(total > 3_900 && total <= 4_000, "{total}");
        // Two chatty processes split the window in two.
        std::fs::write(dir.path().join("dyno-agent.2026-10-31.log"), &big).unwrap();
        let got = log_window(dir.path(), "dyno", 4_000);
        // Each is cut at a line, so one may keep a few bytes less than its half and the other gets them.
        let sizes: Vec<usize> = got.iter().map(|g| g.1.len()).collect();
        assert!(
            sizes.iter().all(|n| (1_900..=2_100).contains(n))
                && sizes.iter().sum::<usize>() <= 4_000,
            "{sizes:?}"
        );
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn an_unreadable_folder_or_file_is_reported_not_called_empty() {
        let dir = tempfile::tempdir().unwrap();
        // A missing folder is "no logs yet": no error.
        let none = log_window_report(&dir.path().join("nope"), "app", 100);
        assert!(none.files.is_empty() && none.skipped.is_empty() && none.folder_error.is_none());
        // A path that is a file, not a folder, cannot be listed: that is an error, not an empty folder.
        let f = dir.path().join("plain");
        std::fs::write(&f, "x").unwrap();
        assert!(log_window_report(&f, "app", 100).folder_error.is_some());
        // A log that cannot be opened is named with its reason (a directory named like a log is not a log: use a locked file on Unix).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let p = dir.path().join("app.2026-10-24.log");
            std::fs::write(&p, "a\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
            let is_root = File::open(&p).is_ok();
            let w = log_window_report(dir.path(), "app", 100);
            if !is_root {
                assert_eq!(w.skipped.len(), 1, "{w:?}");
                assert_eq!(w.skipped[0].0, "app.2026-10-24.log");
                assert!(w.files.is_empty());
            }
        }
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

    #[test]
    fn log_files_lists_matching_logs_and_an_unreadable_folder_is_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.2026-10-24.log"), "a").unwrap();
        std::fs::write(dir.path().join("app.notes.txt"), "b").unwrap();
        std::fs::write(dir.path().join("other.log"), "c").unwrap();
        std::fs::create_dir(dir.path().join("app.dir.log")).unwrap();
        let names: Vec<String> = log_files(dir.path(), "app")
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["app.2026-10-24.log"], "only files named app*.log");
        // A missing folder is told apart by try_log_files and is simply empty here.
        let missing = dir.path().join("gone");
        assert!(log_files(&missing, "app").is_empty());
        assert_eq!(
            try_log_files(&missing, "app").unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }
}
