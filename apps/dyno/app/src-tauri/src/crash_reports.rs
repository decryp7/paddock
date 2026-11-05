//! The operating system's crash reports for Dyno's processes, by program and time only (ER-27): a native crash (SIGTRAP, an access
//! violation) leaves no panic marker, so the shared "previous run did not end cleanly" line says that something died and these
//! lines say what the OS recorded and when. Never the report's content (it holds paths and memory) and never the file name as it
//! is: a legacy name carries the computer's name (`Dyno_2026-11-05-101500_<host>.crash`), so the line keeps the program and the
//! timestamp and cuts everything after it. `.diag` files (resource reports) are not crashes and are not listed.
//!
//! Folders: macOS `~/Library/Logs/DiagnosticReports` and `/Library/Logs/DiagnosticReports` (reports of root daemons such as the
//! helper); Windows `%LOCALAPPDATA%\CrashDumps` and the Windows Error Reporting archives `...\WER\ReportArchive\AppCrash_<program>*`
//! (the default where no dump folder is configured). An empty result says which folder was looked at, so "none" is never a bare
//! claim.

use crate::logging;
use paddock_diag::{Content, Context, Section, SectionFile, SectionOutput};
use std::path::{Path, PathBuf};

/// The programs whose reports are listed (a report's file name starts with the program's name).
pub const PROGRAMS: [&str; 4] = ["dyno-app", "dyno-agent", "dyno-helper", "Dyno"];
/// File extensions of crash reports (`.diag` is a resource report, not a crash, and holds the host name).
const EXTENSIONS: [&str; 3] = ["ips", "crash", "dmp"];
/// At most this many reports are listed per folder, newest first, and at most this many folder entries are looked at.
pub const MAX_REPORTS: usize = 20;
const MAX_ENTRIES: usize = 5000;

/// One report: the program, the timestamp from its name (or from its modification time) and when it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The program (`dyno-app`, `AppCrash_dyno-app.exe`), never a host name.
    pub program: String,
    /// `YYYY-MM-DD-HHMMSS` from the name when it has one, else the modification time in the same shape.
    pub stamp: String,
    /// Modification time, seconds since the epoch.
    pub modified: i64,
}

/// A folder to look in, named without the user's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// How the log names it (`~/Library/Logs/DiagnosticReports`).
    pub label: String,
    /// Where it is.
    pub path: PathBuf,
}

/// Whether `name` is worth a `stat`: decided on the name alone, before any file system call (ER-17).
pub fn is_candidate(name: &str) -> bool {
    describe(name, 0).is_some()
}

/// The program and the stamp of a report file or WER folder name, or none when it is not one of Dyno's. `modified` fills the stamp
/// when the name has no timestamp. Only the program and the timestamp are kept: what follows the timestamp in a legacy name is the
/// computer's name.
pub fn describe(name: &str, modified: i64) -> Option<(String, String)> {
    // Windows Error Reporting archive folder: `AppCrash_<program>_<hashes>`; the program is the part up to the next `_`.
    if let Some(rest) = name.strip_prefix("AppCrash_") {
        let program = rest.split('_').next().unwrap_or("");
        let base = program.strip_suffix(".exe").unwrap_or(program);
        return PROGRAMS
            .iter()
            .any(|p| base.eq_ignore_ascii_case(p))
            .then(|| (format!("AppCrash_{program}"), stamp_of(modified)));
    }
    let (stem, ext) = name.rsplit_once('.')?;
    if !EXTENSIONS.contains(&ext) {
        return None;
    }
    let program = PROGRAMS.iter().find(|p| stem.starts_with(**p) && boundary_after(stem, p.len()))?;
    let after = &stem[program.len()..];
    // `<program>-<YYYY-MM-DD-HHMMSS>` (.ips) or `<program>_<YYYY-MM-DD-HHMMSS>_<host>` (.crash): keep the timestamp, drop the rest.
    let stamp = after
        .strip_prefix(['-', '_'])
        .and_then(|t| t.get(..17))
        .filter(|t| looks_like_stamp(t))
        .map_or_else(|| stamp_of(modified), str::to_string);
    // A minidump is `<program>.exe.<pid>.dmp`: the program is then the whole `<program>.exe`.
    let shown = if ext == "dmp" && after.starts_with(".exe") {
        format!("{program}.exe")
    } else {
        (*program).to_string()
    };
    Some((shown, stamp))
}

/// The character after a program name is a separator (`-`, `_`, `.`) or the end: `dyno-appx` is not `dyno-app`.
fn boundary_after(stem: &str, at: usize) -> bool {
    stem[at..].chars().next().is_none_or(|c| matches!(c, '-' | '_' | '.'))
}

/// `YYYY-MM-DD-HHMMSS`.
fn looks_like_stamp(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() == 17
        && b.iter()
            .enumerate()
            .all(|(i, c)| if matches!(i, 4 | 7 | 10) { *c == b'-' } else { c.is_ascii_digit() })
}

/// The reports of Dyno's programs among `entries` (name, modification time), newest first, at most [`MAX_REPORTS`].
pub fn select(entries: &[(String, i64)]) -> Vec<Report> {
    let mut out: Vec<Report> = entries
        .iter()
        .filter_map(|(name, modified)| {
            describe(name, *modified).map(|(program, stamp)| Report {
                program,
                stamp,
                modified: *modified,
            })
        })
        .collect();
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.program.cmp(&b.program)));
    out.truncate(MAX_REPORTS);
    out
}

/// UTC `YYYY-MM-DD-HHMMSS` of seconds since the epoch (civil-from-days, no time-zone data needed).
pub fn stamp_of(secs: i64) -> String {
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
        "{year:04}-{month:02}-{day:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The report list of one folder as text: one line per report, `program<TAB>stamp<TAB>age`; an empty list says which folder was
/// looked at.
pub fn render(folder: &str, reports: &[Report], now: i64) -> String {
    if reports.is_empty() {
        return format!("none in {folder}\n");
    }
    reports
        .iter()
        .map(|r| format!("{}\t{}\t{} s ago\n", r.program, r.stamp, (now - r.modified).max(0)))
        .collect()
}

/// The folders the OS keeps reports in on this platform.
pub fn default_folders() -> Vec<Folder> {
    // Linux has no report folder of its own here: `out` stays empty and needs no `mut` there.
    #[cfg_attr(not(any(target_os = "macos", windows)), allow(unused_mut))]
    let mut out = Vec::new();
    #[cfg(target_os = "macos")]
    {
        if let Some(h) = std::env::var_os("HOME") {
            out.push(Folder {
                label: "~/Library/Logs/DiagnosticReports".into(),
                path: PathBuf::from(h).join("Library/Logs/DiagnosticReports"),
            });
        }
        out.push(Folder {
            label: "/Library/Logs/DiagnosticReports".into(),
            path: PathBuf::from("/Library/Logs/DiagnosticReports"),
        });
    }
    #[cfg(windows)]
    {
        if let Some(h) = std::env::var_os("LOCALAPPDATA") {
            let h = PathBuf::from(h);
            out.push(Folder {
                label: "%LOCALAPPDATA%\\CrashDumps".into(),
                path: h.join("CrashDumps"),
            });
            out.push(Folder {
                label: "%LOCALAPPDATA%\\Microsoft\\Windows\\WER\\ReportArchive".into(),
                path: h.join("Microsoft/Windows/WER/ReportArchive"),
            });
        }
        if let Some(p) = std::env::var_os("ProgramData") {
            out.push(Folder {
                label: "%ProgramData%\\Microsoft\\Windows\\WER\\ReportArchive".into(),
                path: PathBuf::from(p).join("Microsoft/Windows/WER/ReportArchive"),
            });
        }
    }
    out
}

/// Reads the folder's entries that pass [`is_candidate`] by name (only those are `stat`ed), bounded.
pub fn read_entries(dir: &Path) -> std::io::Result<Vec<(String, i64)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)?.take(MAX_ENTRIES).flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !is_candidate(&name) {
            continue;
        }
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

/// Report a problem's section: the OS crash reports of Dyno's programs, by program and time.
pub struct CrashReports {
    folders: Vec<Folder>,
}

impl CrashReports {
    /// The section over `folders` (none: the platform has no such folder).
    pub fn new(folders: Vec<Folder>) -> Self {
        CrashReports { folders }
    }
}

impl Section for CrashReports {
    fn id(&self) -> &str {
        "os-crash-reports"
    }
    fn collect(&self, cx: &Context<'_>) -> SectionOutput {
        if self.folders.is_empty() {
            return SectionOutput::note("This system keeps no crash-report folder Dyno knows of.");
        }
        let now = cx.now.timestamp();
        let mut text = String::new();
        let mut notes = Vec::new();
        for f in &self.folders {
            text.push_str(&format!("# {}\n", f.label));
            match read_entries(&f.path) {
                Ok(entries) => text.push_str(&render(&f.label, &select(&entries), now)),
                Err(e) => {
                    text.push_str(&format!("not readable ({})\n", e.kind()));
                    notes.push(format!("The crash-report folder {} could not be read ({})", f.label, e.kind()));
                }
            }
        }
        SectionOutput {
            files: vec![SectionFile::app(
                "os-crash-reports.txt",
                "Programs and times of the system's crash reports for Dyno (not their content or file names)",
                Content::Text(text),
            )],
            notes,
        }
    }
}

/// Logs what the OS recorded for earlier runs (program and time, never content or file names), on a thread of its own: a folder can
/// hold thousands of entries and the start must not wait for it.
pub fn log_at_start_in_background(folders: Vec<Folder>, now: i64) {
    let spawned = std::thread::Builder::new()
        .name("crash-reports".into())
        .spawn(move || log_at_start(&folders, now));
    if let Err(e) = spawned {
        logging::info(&format!("crash reports: the thread could not start: {e}"));
    }
}

/// Logs what the OS recorded for earlier runs, folder by folder.
pub fn log_at_start(folders: &[Folder], now: i64) {
    for f in folders {
        match read_entries(&f.path) {
            Ok(entries) => {
                let reports = select(&entries);
                if reports.is_empty() {
                    logging::info(&format!("crash reports: none in {}", f.label));
                }
                for r in reports.iter().take(5) {
                    logging::info(&format!(
                        "crash report on record in {}: {} at {} ({} s before this start)",
                        f.label,
                        r.program,
                        r.stamp,
                        (now - r.modified).max(0)
                    ));
                }
            }
            Err(e) => logging::info(&format!("crash reports: {} could not be read ({})", f.label, e.kind())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(n: &str, t: i64) -> (String, i64) {
        (n.to_string(), t)
    }

    #[test]
    fn only_dynos_crash_reports_are_listed_newest_first_with_the_host_name_cut_off() {
        let list = select(&[
            e("dyno-app-2026-11-05-101500.ips", 100),
            e("Other-2026-11-05-101500.ips", 300),
            e("dyno-app-2026-11-05-111500.ips", 200),
            e("dyno-app-notes.txt", 400),
            e("Dyno_2026-11-04-090000_Janes-MacBook-Pro.crash", 50),
            e("Dyno_2026-11-04-090000_Janes-MacBook-Pro.cpu_resource.diag", 60),
            e("dyno-helper-2026-11-03-010203.ips", 10),
        ]);
        let shown: Vec<(&str, &str)> = list.iter().map(|r| (r.program.as_str(), r.stamp.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("dyno-app", "2026-11-05-111500"),
                ("dyno-app", "2026-11-05-101500"),
                ("Dyno", "2026-11-04-090000"),
                ("dyno-helper", "2026-11-03-010203"),
            ]
        );
        let text = render("f", &list, 1_000_000);
        assert!(
            !text.contains("Janes") && !text.contains("diag") && !text.contains(".ips"),
            "{text}"
        );
    }

    #[test]
    fn real_macos_report_names_are_matched_without_real_data() {
        // The shapes macOS writes: `<process>-<date>-<time>.ips` (macOS 12+), `<process>_<date>-<time>_<host>.crash` (older),
        // `<process>_<date>-<time>_<host>.<kind>.diag` (resource reports, not crashes). Invented host.
        assert_eq!(
            describe("dyno-app-2026-11-05-101500.ips", 0),
            Some(("dyno-app".to_string(), "2026-11-05-101500".to_string()))
        );
        assert_eq!(
            describe("Dyno_2026-11-05-101500_example-host.crash", 0),
            Some(("Dyno".to_string(), "2026-11-05-101500".to_string()))
        );
        assert_eq!(describe("Dyno_2026-11-05-101500_example-host.cpu_resource.diag", 0), None);
        assert_eq!(
            describe("dyno-appx-2026-11-05-101500.ips", 0),
            None,
            "a program that merely starts alike"
        );
    }

    #[test]
    fn windows_error_reporting_folders_and_minidumps_are_matched_by_program_and_time() {
        let (program, stamp) = describe("AppCrash_dyno-app.exe_8f3a2b1c_cab_0a1b2c3d", 1_793_836_000).unwrap();
        assert_eq!((program.as_str(), stamp.as_str()), ("AppCrash_dyno-app.exe", "2026-11-04-234640"));
        let (program, stamp) = describe("dyno-app.exe.4242.dmp", 1_793_836_000).unwrap();
        assert_eq!((program.as_str(), stamp.as_str()), ("dyno-app.exe", "2026-11-04-234640"));
        assert_eq!(describe("AppCrash_other.exe_1_2", 1), None);
    }

    #[test]
    fn the_list_is_capped_and_nothing_else_is_statted_by_name_alone() {
        let many: Vec<_> = (0..100).map(|i| e(&format!("dyno-app-2026-11-05-{i:06}.ips"), i)).collect();
        assert_eq!(select(&many).len(), MAX_REPORTS);
        assert_eq!(select(&many)[0].modified, 99);
        assert!(!is_candidate("unrelated.log") && !is_candidate("dyno-app.txt") && is_candidate("dyno-app-2026-11-05-101500.ips"));
    }

    #[test]
    fn an_empty_result_names_the_folder_it_looked_in() {
        assert_eq!(
            render("~/Library/Logs/DiagnosticReports", &[], 5),
            "none in ~/Library/Logs/DiagnosticReports\n"
        );
        let text = render(
            "f",
            &[Report {
                program: "dyno-app".into(),
                stamp: "2026-11-04-234640".into(),
                modified: 1_793_836_000,
            }],
            1_793_836_090,
        );
        assert_eq!(text, "dyno-app\t2026-11-04-234640\t90 s ago\n");
    }

    #[test]
    fn the_folder_is_read_without_statting_unrelated_files_and_a_missing_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("dyno-app-2026-11-05-101500.ips"), "secret path /Users/x").unwrap();
        std::fs::write(dir.path().join("other.txt"), "x").unwrap();
        let entries = read_entries(dir.path()).unwrap();
        assert_eq!(entries.len(), 1, "other.txt is dropped by name before any metadata call");
        assert!(entries[0].1 > 0);
        assert!(read_entries(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn the_start_logs_program_and_time_without_content_or_file_names_and_names_empty_folders() {
        let _ = crate::logging::testlog::lines(); // installs the capture before anything is logged
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("dyno-app-2026-11-05-101500.ips"), "secret path /Users/x").unwrap();
        let empty = tempfile::tempdir().unwrap();
        let folders = vec![
            Folder {
                label: "start-probe-with-report".into(),
                path: dir.path().to_path_buf(),
            },
            Folder {
                label: "start-probe-empty".into(),
                path: empty.path().to_path_buf(),
            },
        ];
        log_at_start(&folders, i64::MAX / 4);
        let lines = crate::logging::testlog::lines();
        let line = lines
            .iter()
            .find(|l| l.contains("start-probe-with-report"))
            .expect("the report is on record");
        assert!(
            line.contains("dyno-app at 2026-11-05-101500") && !line.contains("secret") && !line.contains(".ips"),
            "{line}"
        );
        assert!(
            lines.iter().any(|l| l.contains("crash reports: none in start-probe-empty")),
            "{lines:?}"
        );
    }
}
