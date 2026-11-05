//! The operating system's crash reports for Dyno's processes, by name and time only (ER-27): a native crash (SIGTRAP, an access
//! violation) leaves no panic marker, so the shared "previous run did not end cleanly" line says that something died and these
//! names say what the OS recorded and when. Never the report's content (it holds paths and memory). macOS lists
//! `~/Library/Logs/DiagnosticReports`, Windows `%LOCALAPPDATA%\CrashDumps`; elsewhere there is nothing to list.

use crate::logging;
use paddock_diag::{Content, Context, Section, SectionFile, SectionOutput};
use std::path::{Path, PathBuf};

/// The programs whose reports are listed (a report's file name starts with the program's name).
pub const PROGRAMS: [&str; 4] = ["dyno-app", "dyno-agent", "dyno-helper", "Dyno"];
/// File extensions of the OS's reports.
const EXTENSIONS: [&str; 4] = ["ips", "crash", "dmp", "diag"];
/// At most this many reports are listed, newest first, and at most this many folder entries are looked at.
pub const MAX_REPORTS: usize = 20;
const MAX_ENTRIES: usize = 5000;

/// One report: its file name and when it was written (seconds since the epoch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The file name.
    pub name: String,
    /// Modification time, seconds since the epoch.
    pub modified: i64,
}

/// The reports of Dyno's programs among `entries` (file name, modification time), newest first, at most [`MAX_REPORTS`].
pub fn select(entries: &[(String, i64)]) -> Vec<Report> {
    let mut out: Vec<Report> = entries
        .iter()
        .filter(|(name, _)| is_report(name))
        .map(|(name, modified)| Report {
            name: name.clone(),
            modified: *modified,
        })
        .collect();
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
    out.truncate(MAX_REPORTS);
    out
}

fn is_report(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    EXTENSIONS.contains(&ext) && PROGRAMS.iter().any(|p| stem.starts_with(p))
}

/// UTC `YYYY-MM-DDTHH:MM:SSZ` of seconds since the epoch (civil-from-days, no time-zone data needed).
fn utc(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The report list as text: one line per report, `name<TAB>written (UTC)<TAB>age`.
pub fn render(reports: &[Report], now: i64) -> String {
    if reports.is_empty() {
        return "none\n".to_string();
    }
    reports
        .iter()
        .map(|r| format!("{}\t{}\t{} s ago\n", r.name, utc(r.modified), (now - r.modified).max(0)))
        .collect()
}

/// The folder the OS keeps reports in, when this platform has one.
pub fn default_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Logs/DiagnosticReports"))
    }
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(|h| PathBuf::from(h).join("CrashDumps"))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        None
    }
}

/// Reads the folder's entry names and times, bounded.
pub fn read_entries(dir: &Path) -> std::io::Result<Vec<(String, i64)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)?.take(MAX_ENTRIES).flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        out.push((name, modified));
    }
    Ok(out)
}

/// Report a problem's section: the OS crash reports of Dyno's programs, by name and time.
pub struct CrashReports {
    dir: Option<PathBuf>,
}

impl CrashReports {
    /// The section over `dir` (none: the platform has no such folder).
    pub fn new(dir: Option<PathBuf>) -> Self {
        CrashReports { dir }
    }
}

impl Section for CrashReports {
    fn id(&self) -> &str {
        "os-crash-reports"
    }
    fn collect(&self, cx: &Context<'_>) -> SectionOutput {
        let Some(dir) = &self.dir else {
            return SectionOutput::note("This system keeps no crash-report folder Dyno knows of.");
        };
        match read_entries(dir) {
            Ok(entries) => SectionOutput::one(SectionFile::app(
                "os-crash-reports.txt",
                "Names and times of the system's crash reports for Dyno (not their content)",
                Content::Text(render(&select(&entries), cx.now.timestamp())),
            )),
            Err(e) => SectionOutput::note(format!("The system's crash-report folder could not be read ({})", e.kind())),
        }
    }
}

/// Logs at start what the OS recorded for earlier runs (names and times, never content): a native crash leaves no panic marker,
/// so this is the evidence that the "previous run did not end cleanly" line has a crash behind it.
pub fn log_at_start(dir: Option<&Path>, now: i64) {
    let Some(dir) = dir else { return };
    match read_entries(dir) {
        Ok(entries) => {
            let reports = select(&entries);
            if reports.is_empty() {
                logging::info("crash reports: the system has none for Dyno");
            }
            for r in reports.iter().take(5) {
                logging::info(&format!(
                    "crash report on record: {} written {} ({} s before this start)",
                    r.name,
                    utc(r.modified),
                    (now - r.modified).max(0)
                ));
            }
        }
        Err(e) => logging::info(&format!("crash reports: the folder could not be read ({})", e.kind())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(n: &str, t: i64) -> (String, i64) {
        (n.to_string(), t)
    }

    #[test]
    fn only_dynos_reports_are_listed_newest_first() {
        let list = select(&[
            e("dyno-app-2026-11-05-101500.ips", 100),
            e("Other-2026-11-05-101500.ips", 300),
            e("dyno-app-2026-11-05-111500.ips", 200),
            e("dyno-app-notes.txt", 400),
            e("Dyno_2026-11-04.crash", 50),
            e("dyno-agent-1.dmp", 10),
        ]);
        let names: Vec<&str> = list.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "dyno-app-2026-11-05-111500.ips",
                "dyno-app-2026-11-05-101500.ips",
                "Dyno_2026-11-04.crash",
                "dyno-agent-1.dmp"
            ]
        );
    }

    #[test]
    fn the_list_is_capped() {
        let many: Vec<_> = (0..100).map(|i| e(&format!("dyno-app-{i}.ips"), i)).collect();
        assert_eq!(select(&many).len(), MAX_REPORTS);
        assert_eq!(select(&many)[0].modified, 99);
    }

    #[test]
    fn the_text_has_name_time_and_age_and_nothing_else() {
        let text = render(
            &[Report {
                name: "dyno-app-1.ips".into(),
                modified: 1_793_836_000,
            }],
            1_793_836_090,
        );
        assert_eq!(text, "dyno-app-1.ips\t2026-11-04T23:46:40Z\t90 s ago\n");
        assert_eq!(render(&[], 5), "none\n");
    }

    #[test]
    fn the_folder_is_read_by_name_and_time_only_and_a_missing_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("dyno-app-1.ips"), "secret path /Users/x").unwrap();
        std::fs::write(dir.path().join("other.txt"), "x").unwrap();
        let entries = read_entries(dir.path()).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|(_, t)| *t > 0));
        assert!(read_entries(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn the_start_logs_the_reports_without_their_content() {
        let _ = crate::logging::testlog::lines(); // installs the capture before anything is logged
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("dyno-app-start-probe-77.ips"), "secret path /Users/x").unwrap();
        log_at_start(Some(dir.path()), i64::MAX / 4);
        let lines = crate::logging::testlog::lines();
        let line = lines
            .iter()
            .find(|l| l.contains("dyno-app-start-probe-77.ips"))
            .expect("the report is named in the log");
        assert!(!line.contains("secret"), "{line}");
        assert!(line.contains("crash report"), "{line}");
    }
}
