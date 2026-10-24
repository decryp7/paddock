//! Where formatted lines go: one file per process per day (`<process>.YYYY-MM-DD.log`) with a size budget, old days
//! removed, and standard output for development.

use crate::clef::{Enrichment, format_event};
use crate::clock::{Clock, now_utc};
use crate::error::lock;
use chrono::{Duration, NaiveDate};
use serde_json::Map;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A destination for formatted lines (without the newline). Implementations must not panic and must not fail the caller:
/// a full disk or a locked file drops the line.
pub trait LineSink: Send + Sync + 'static {
    /// Writes one line.
    fn write_line(&self, level: log::Level, line: &str);
}

/// Standard output (a terminal while developing; nowhere in a GUI build).
#[derive(Debug, Clone, Copy, Default)]
pub struct StdoutSink;

impl LineSink for StdoutSink {
    fn write_line(&self, _: log::Level, line: &str) {
        let _ = writeln!(std::io::stdout().lock(), "{line}");
    }
}

/// A failed open is retried at most this often (a broken disk must not make every line slow).
const RETRY_AFTER: Duration = Duration::seconds(30);

struct State {
    day: Option<NaiveDate>,
    file: Option<File>,
    written: u64,
    warned: bool,
    failed_at: Option<chrono::DateTime<chrono::Utc>>,
    problem: Option<String>,
    dropped: u64,
}

/// The daily files of one process: today's file, at most `max_bytes` a day (errors always pass, a one-time notice says
/// when the limit was reached), and the newest `keep_files` older days.
pub struct DailyFiles {
    dir: PathBuf,
    prefix: String,
    keep_files: usize,
    max_bytes: u64,
    clock: Arc<dyn Clock>,
    enrich: Enrichment,
    state: Mutex<State>,
}

impl DailyFiles {
    /// Files `<prefix>.<day>.log` in `dir`. Nothing is created until the first line.
    pub fn new(
        dir: &Path,
        prefix: &str,
        keep_files: u32,
        max_bytes: u64,
        clock: Arc<dyn Clock>,
        enrich: Enrichment,
    ) -> Self {
        DailyFiles {
            dir: dir.to_path_buf(),
            prefix: prefix.to_string(),
            keep_files: keep_files as usize,
            max_bytes,
            clock,
            enrich,
            state: Mutex::new(State {
                day: None,
                file: None,
                written: 0,
                warned: false,
                failed_at: None,
                problem: None,
                dropped: 0,
            }),
        }
    }

    /// The file lines go to today.
    pub fn active_file(&self) -> PathBuf {
        self.dir
            .join(file_name(&self.prefix, now_utc(&*self.clock).date_naive()))
    }

    /// Why the file cannot be written, in words for the user; `None` while it works.
    pub fn problem(&self) -> Option<String> {
        lock(&self.state).problem.clone()
    }

    /// Lines dropped because the file could not be written.
    pub fn dropped(&self) -> u64 {
        lock(&self.state).dropped
    }

    /// Opens (or rotates to) today's file; on failure remembers why.
    fn open_today(&self, st: &mut State, today: NaiveDate) {
        st.day = None;
        st.file = None;
        let path = self.dir.join(file_name(&self.prefix, today));
        let opened = std::fs::create_dir_all(&self.dir)
            .and_then(|()| OpenOptions::new().create(true).append(true).open(&path));
        match opened {
            Ok(f) => {
                st.written = f.metadata().map(|m| m.len()).unwrap_or(0);
                st.file = Some(f);
                st.day = Some(today);
                st.warned = false;
                st.failed_at = None;
                st.problem = None;
                self.prune(today);
            }
            Err(e) => {
                st.failed_at = Some(now_utc(&*self.clock));
                st.problem = Some(format!(
                    "the log file {} cannot be written ({e}); check the folder's rights and free space",
                    path.display()
                ));
            }
        }
    }

    /// Removes the days beyond the newest `keep_files` older ones. Only this process's files are touched.
    fn prune(&self, today: NaiveDate) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut days: Vec<(NaiveDate, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                parse_day(&self.prefix, &e.file_name().to_string_lossy()).map(|d| (d, e.path()))
            })
            .filter(|(d, _)| *d != today)
            .collect();
        days.sort_by_key(|d| std::cmp::Reverse(d.0));
        for (_, path) in days.into_iter().skip(self.keep_files) {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl LineSink for DailyFiles {
    fn write_line(&self, level: log::Level, line: &str) {
        let now = now_utc(&*self.clock);
        let today = now.date_naive();
        let mut st = lock(&self.state);
        if st.day != Some(today) {
            let retry_ok = st
                .failed_at
                .is_none_or(|t| now - t >= RETRY_AFTER || now < t);
            if retry_ok {
                self.open_today(&mut st, today);
            }
        }
        if st.file.is_none() {
            st.dropped += 1;
            return;
        }
        let len = line.len() as u64 + 1;
        let admitted = st.written + len <= self.max_bytes || level == log::Level::Error;
        let mut out = String::new();
        if !admitted && !st.warned {
            st.warned = true;
            let notice = format!(
                "The log reached its daily limit of {} MB; only errors are written until tomorrow",
                self.max_bytes / (1024 * 1024)
            );
            out.push_str(&format_event(
                now,
                log::Level::Warn,
                "paddock_log",
                &notice,
                Map::new(),
                &self.enrich,
            ));
            out.push('\n');
        }
        if admitted {
            out.push_str(line);
            out.push('\n');
        }
        if out.is_empty() {
            return;
        }
        let written = st
            .file
            .as_mut()
            .map(|f| f.write_all(out.as_bytes()).is_ok())
            .unwrap_or(false);
        if written {
            st.written += out.len() as u64;
        } else {
            // A full disk or a vanished folder: drop the line, and reopen on the next one.
            st.dropped += 1;
            st.day = None;
            st.failed_at = Some(now);
            st.problem = Some(format!(
                "writing to the log file in {} failed; check free space",
                self.dir.display()
            ));
        }
    }
}

fn file_name(prefix: &str, day: NaiveDate) -> String {
    format!("{prefix}.{}.log", day.format("%Y-%m-%d"))
}

/// The day of a file named `<prefix>.<day>.log`; other files, and other prefixes (`app` vs `app-server`), give `None`.
fn parse_day(prefix: &str, name: &str) -> Option<NaiveDate> {
    let rest = name
        .strip_prefix(prefix)?
        .strip_prefix('.')?
        .strip_suffix(".log")?;
    NaiveDate::parse_from_str(rest, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clef::test_enrichment;
    use crate::clock::FakeClock;
    use chrono::{TimeZone, Utc};

    fn clock() -> Arc<FakeClock> {
        Arc::new(FakeClock::at_wall(
            Utc.with_ymd_and_hms(2026, 10, 20, 9, 0, 0).unwrap().into(),
        ))
    }

    fn files(dir: &Path, keep: u32, max: u64, c: &Arc<FakeClock>) -> DailyFiles {
        DailyFiles::new(dir, "demo", keep, max, c.clone(), test_enrichment())
    }

    #[test]
    fn writes_to_the_days_file_and_names_it() {
        let t = tempfile::tempdir().unwrap();
        let c = clock();
        let f = files(t.path(), 3, 1 << 20, &c);
        f.write_line(log::Level::Info, "one");
        f.write_line(log::Level::Info, "two");
        assert_eq!(f.active_file(), t.path().join("demo.2026-10-20.log"));
        assert_eq!(
            std::fs::read_to_string(f.active_file()).unwrap(),
            "one\ntwo\n"
        );
        assert_eq!(f.problem(), None);
    }

    #[test]
    fn a_new_day_starts_a_new_file_and_old_days_are_removed() {
        let t = tempfile::tempdir().unwrap();
        let c = clock();
        let f = files(t.path(), 2, 1 << 20, &c);
        // Files of other processes and unrelated files are never touched.
        std::fs::write(t.path().join("demo-server.2020-01-01.log"), "other process").unwrap();
        std::fs::write(t.path().join("notes.txt"), "mine").unwrap();
        for _ in 0..5 {
            f.write_line(log::Level::Info, "day");
            c.advance(Duration::days(1).to_std().unwrap());
        }
        f.write_line(log::Level::Info, "today");
        let mut names: Vec<String> = std::fs::read_dir(t.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "demo-server.2020-01-01.log",
                "demo.2026-10-23.log",
                "demo.2026-10-24.log",
                "demo.2026-10-25.log",
                "notes.txt"
            ],
            "today plus the two newest older days"
        );
    }

    #[test]
    fn after_the_daily_budget_only_errors_pass_and_the_notice_comes_once() {
        let t = tempfile::tempdir().unwrap();
        let c = clock();
        let f = files(t.path(), 3, 100, &c);
        let big = "x".repeat(60);
        f.write_line(log::Level::Info, &big);
        f.write_line(log::Level::Info, &big);
        f.write_line(log::Level::Info, &big);
        f.write_line(log::Level::Error, "an error");
        let text = std::fs::read_to_string(f.active_file()).unwrap();
        assert_eq!(text.matches(&big).count(), 1);
        assert_eq!(text.matches("daily limit").count(), 1);
        assert!(text.ends_with("an error\n"));
        // Tomorrow has a fresh budget and a fresh notice.
        c.advance(Duration::days(1).to_std().unwrap());
        f.write_line(log::Level::Info, &big);
        assert_eq!(
            std::fs::read_to_string(f.active_file()).unwrap(),
            format!("{big}\n")
        );
    }

    #[test]
    fn the_budget_counts_what_an_earlier_run_wrote_today() {
        let t = tempfile::tempdir().unwrap();
        let c = clock();
        std::fs::write(t.path().join("demo.2026-10-20.log"), "y".repeat(95)).unwrap();
        let f = files(t.path(), 3, 100, &c);
        f.write_line(log::Level::Info, "does not fit");
        let text = std::fs::read_to_string(f.active_file()).unwrap();
        assert!(!text.contains("does not fit") && text.contains("daily limit"));
    }

    #[test]
    fn an_unusable_folder_drops_lines_and_says_why_and_retries_later() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("blocker"), "a file, not a folder").unwrap();
        let c = clock();
        let f = files(&t.path().join("blocker").join("logs"), 3, 1 << 20, &c);
        f.write_line(log::Level::Info, "lost");
        f.write_line(log::Level::Error, "lost too");
        assert_eq!(f.dropped(), 2);
        assert!(f.problem().unwrap().contains("cannot be written"));
        // Repair the folder: the next line after the retry delay lands.
        std::fs::remove_file(t.path().join("blocker")).unwrap();
        f.write_line(log::Level::Info, "still waiting");
        assert_eq!(f.dropped(), 3, "no retry before the delay");
        c.advance(Duration::seconds(31).to_std().unwrap());
        f.write_line(log::Level::Info, "back");
        assert_eq!(std::fs::read_to_string(f.active_file()).unwrap(), "back\n");
        assert_eq!(f.problem(), None);
    }

    #[test]
    fn a_deleted_folder_while_running_recovers() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("logs");
        let c = clock();
        let f = files(&dir, 3, 1 << 20, &c);
        f.write_line(log::Level::Info, "first");
        // On Unix an open file survives its folder being removed, so only the reopen path is exercised there.
        std::fs::remove_dir_all(&dir).unwrap();
        f.write_line(log::Level::Info, "second");
        c.advance(Duration::days(1).to_std().unwrap());
        f.write_line(log::Level::Info, "third");
        assert_eq!(std::fs::read_to_string(f.active_file()).unwrap(), "third\n");
    }

    #[test]
    fn parses_only_its_own_files() {
        assert!(parse_day("app", "app.2026-10-20.log").is_some());
        assert!(parse_day("app", "app-server.2026-10-20.log").is_none());
        assert!(parse_day("app", "app.2026-13-40.log").is_none());
        assert!(parse_day("app", "app.2026-10-20.log.gz").is_none());
        StdoutSink.write_line(log::Level::Info, "{}");
    }

    #[test]
    fn a_write_that_fails_drops_the_line_says_why_and_reopens_on_the_next_one() {
        let t = tempfile::tempdir().unwrap();
        let c = clock();
        let f = files(t.path(), 3, 1 << 20, &c);
        f.write_line(log::Level::Info, "first");
        // The open file stops accepting writes (a full disk or a vanished share): a read-only handle fails the same way.
        lock(&f.state).file = Some(File::open(f.active_file()).unwrap());
        f.write_line(log::Level::Info, "lost");
        assert_eq!(f.dropped(), 1);
        assert!(f.problem().unwrap().contains("check free space"));
        // The failure is retried after the back-off, and the file is reopened.
        c.advance(Duration::seconds(31).to_std().unwrap());
        f.write_line(log::Level::Info, "again");
        assert_eq!(f.problem(), None);
        assert_eq!(
            std::fs::read_to_string(f.active_file()).unwrap(),
            "first\nagain\n"
        );
    }
}
