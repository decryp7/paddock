//! The report without a window: `<app> --export-diagnostics [<path.zip>] [--force]` writes the same report as Report a problem and
//! exits, for the case where the window itself failed to start (Dyno's tray app with a blank window, a webview that never
//! answers). It is the shared form of Debrief's `report/cli.rs` (which migrates to it later). Nothing here starts a window, a
//! plugin or a worker, and the report is redacted exactly as the dialog's is.
//!
//! Exit codes are the caller's: `0` and the printed path on success, `1` and the printed reason (what to fix) on failure.

use crate::{AppInfo, Collected, Redactor, Saved, Section, suggested_name};
use chrono::{DateTime, Utc};
use paddock_env::AppDirs;
use paddock_jobs::CancelToken;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The flag.
pub const FLAG: &str = "--export-diagnostics";
/// Replace an existing file.
pub const FORCE: &str = "--force";

/// Where the report goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// The file named on the command line.
    Path(PathBuf),
    /// A timestamped file in the user's Documents (else Downloads, else the home folder).
    Default,
}

/// The parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportArgs {
    /// Where.
    pub destination: Destination,
    /// Replace an existing file.
    pub force: bool,
}

/// `None` when the command line does not ask for an export (the app starts normally). Only the arguments before `--` count: a path
/// after it (a relaunch reopening a session) never triggers an export.
pub fn parse<S: AsRef<str>>(args: &[S]) -> Option<Result<ExportArgs, String>> {
    let args: Vec<&str> = args
        .iter()
        .map(AsRef::as_ref)
        .take_while(|a| *a != "--")
        .collect();
    let at = args
        .iter()
        .position(|a| *a == FLAG || a.starts_with(&format!("{FLAG}=")))?;
    let force = args.contains(&FORCE);
    let inline = args[at]
        .strip_prefix(&format!("{FLAG}="))
        .map(str::to_string);
    let next = args
        .get(at + 1)
        .filter(|a| !a.starts_with("--"))
        .map(|a| (*a).to_string());
    Some(match inline.or(next) {
        Some(p) if p.trim().is_empty() => Err(format!("{FLAG} needs a file name ending in .zip")),
        Some(p) => Ok(ExportArgs {
            destination: Destination::Path(PathBuf::from(p)),
            force,
        }),
        None => Ok(ExportArgs {
            destination: Destination::Default,
            force,
        }),
    })
}

/// The absolute file to write: a relative path is taken from `cwd`; the name must end in `.zip`; an existing file is refused unless
/// `force`. `default_dirs` are tried in order for [`Destination::Default`].
///
/// # Errors
/// What to change, in words.
pub fn resolve(
    args: &ExportArgs,
    app: &AppInfo,
    cwd: &Path,
    default_dirs: &[PathBuf],
    now: DateTime<Utc>,
) -> Result<PathBuf, String> {
    let path = match &args.destination {
        Destination::Path(p) if p.is_absolute() => p.clone(),
        Destination::Path(p) => cwd.join(p),
        Destination::Default => default_dirs
            .iter()
            .find(|d| d.is_dir())
            .ok_or_else(|| format!("found no Documents, Downloads or home folder; give the file to write: {FLAG} <path.zip>"))?
            .join(suggested_name(app, now)),
    };
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        return Err(format!("{} must end in .zip", path.display()));
    }
    if path.exists() && !args.force {
        return Err(format!(
            "{} exists; add {FORCE} to replace it",
            path.display()
        ));
    }
    Ok(path)
}

/// Collects the report from `sections` and writes it to `dest` (already resolved). Whatever a section cannot collect is a note in the
/// report, never a failure. The report has no description.
///
/// # Errors
/// Why the zip could not be written.
pub fn write(
    app: &AppInfo,
    dirs: &AppDirs,
    sections: &[Arc<dyn Section>],
    redactor: &Redactor,
    dest: &Path,
    now: DateTime<Utc>,
) -> Result<Saved, String> {
    Collected::collect(app, dirs, sections, redactor, now)
        .assemble("")
        .save(dest, &CancelToken::new(), &())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn app() -> AppInfo {
        AppInfo::new("demo", "Demo", "1.0.0", "abc")
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 5, 9, 0, 0).unwrap()
    }

    #[test]
    fn only_the_flag_before_the_separator_asks_for_an_export() {
        assert!(parse(&["demo"]).is_none());
        assert!(parse(&["demo", "--", "--export-diagnostics"]).is_none());
        assert_eq!(
            parse(&["demo", "--export-diagnostics", "r.zip", "--force"])
                .unwrap()
                .unwrap(),
            ExportArgs {
                destination: Destination::Path("r.zip".into()),
                force: true
            }
        );
        assert_eq!(
            parse(&["--export-diagnostics=/x/y.zip"])
                .unwrap()
                .unwrap()
                .destination,
            Destination::Path("/x/y.zip".into())
        );
        assert_eq!(
            parse(&["--export-diagnostics"])
                .unwrap()
                .unwrap()
                .destination,
            Destination::Default
        );
        assert!(parse(&["--export-diagnostics="]).unwrap().is_err());
    }

    #[test]
    fn the_destination_is_resolved_and_refused_with_what_to_change() {
        let dir = tempfile::tempdir().unwrap();
        let args = |d| ExportArgs {
            destination: d,
            force: false,
        };
        let p = resolve(
            &args(Destination::Path("r.zip".into())),
            &app(),
            dir.path(),
            &[],
            now(),
        )
        .unwrap();
        assert_eq!(p, dir.path().join("r.zip"));
        let d = resolve(
            &args(Destination::Default),
            &app(),
            dir.path(),
            &[dir.path().join("none"), dir.path().into()],
            now(),
        )
        .unwrap();
        assert_eq!(d, dir.path().join("demo-diagnostics-20261105-090000.zip"));
        assert!(
            resolve(&args(Destination::Default), &app(), dir.path(), &[], now())
                .unwrap_err()
                .contains("Documents")
        );
        assert!(
            resolve(
                &args(Destination::Path("r.txt".into())),
                &app(),
                dir.path(),
                &[],
                now()
            )
            .unwrap_err()
            .contains(".zip")
        );
        std::fs::write(dir.path().join("r.zip"), "x").unwrap();
        assert!(
            resolve(
                &args(Destination::Path("r.zip".into())),
                &app(),
                dir.path(),
                &[],
                now()
            )
            .unwrap_err()
            .contains("--force")
        );
        let forced = ExportArgs {
            destination: Destination::Path("r.zip".into()),
            force: true,
        };
        assert!(resolve(&forced, &app(), dir.path(), &[], now()).is_ok());
    }

    #[test]
    fn the_report_is_written_without_a_window_and_redacted() {
        use paddock_env::{AppId, MapVars, Os};
        let root = tempfile::tempdir().unwrap();
        let id = AppId::new("demo", "Demo").unwrap();
        let dirs = AppDirs::resolve(
            &id,
            Os::MacOs,
            &MapVars::new().with("DEMO_HOME", root.path()),
            None,
            true,
        )
        .unwrap();
        std::fs::create_dir_all(dirs.logs()).unwrap();
        std::fs::write(
            dirs.logs().join("demo.2026-11-05.log"),
            "{\"@m\":\"opened /Users/sam/a.txt token=hunter2hunter2\"}\n",
        )
        .unwrap();
        let dest = root.path().join("out.zip");
        let saved = write(
            &app(),
            &dirs,
            &crate::builtin::all(),
            &Redactor::new(),
            &dest,
            now(),
        )
        .unwrap();
        assert!(saved.files >= 3 && dest.exists());
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
        let mut log = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("logs/demo.2026-11-05.log").unwrap(),
            &mut log,
        )
        .unwrap();
        assert!(!log.contains("sam") && !log.contains("hunter2"), "{log}");
        assert!(
            write(
                &app(),
                &dirs,
                &[],
                &Redactor::new(),
                Path::new("relative.zip"),
                now()
            )
            .is_err()
        );
    }
}
