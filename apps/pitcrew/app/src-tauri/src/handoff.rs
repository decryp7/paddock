//! The hand-off of a job's log to Debrief, the owner's log-analysis app (ADR 0018). Debrief opens the files it gets on
//! its command line (and a running Debrief takes them over). If Debrief is not installed the caller gets a plain
//! "not installed" answer and the extract is still written, so the owner can open it by hand.

use pitcrew_core::debrief_link::{self, DEBRIEF_CLI};
use pitcrew_core::platform::Os;
use pitcrew_core::time::Timestamp;
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Most bytes of the runner log read for an extract (the newest are kept).
pub const MAX_LOG_READ: u64 = 64 * 1024 * 1024;

/// What the app knows about Debrief.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebriefStatus {
    /// Debrief's program was found.
    pub installed: bool,
    /// Where.
    pub program: Option<String>,
    /// The command-line name Pitcrew looks for (for the "not installed" message).
    pub cli_name: String,
}

/// How a hand-off ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum Handoff {
    /// Debrief was started with the file.
    Opened {
        /// The file handed over.
        file: String,
    },
    /// Debrief is not installed; the file is ready to be opened by hand.
    NotInstalled {
        /// The file written.
        file: String,
        /// What to do, in words.
        message: String,
    },
    /// The log has nothing for that job (rotated away, or the job ran before the log began).
    NoLines {
        /// What to do, in words.
        message: String,
    },
}

fn os() -> Os {
    if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Macos
    } else {
        Os::Linux
    }
}

/// Finds Debrief's program on `path` (a `PATH` list), then in the usual install places.
pub fn find(path: Option<&str>, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let file = format!("{DEBRIEF_CLI}{}", std::env::consts::EXE_SUFFIX);
    let on_path = path.into_iter().flat_map(std::env::split_paths).map(|d| d.join(&file));
    on_path.chain(debrief_link::install_candidates(os(), env)).find(|p| p.is_file())
}

/// Debrief's status on this machine.
pub fn status() -> DebriefStatus {
    let path = std::env::var("PATH").ok();
    let found = find(path.as_deref(), &|k| std::env::var(k).ok());
    DebriefStatus {
        installed: found.is_some(),
        program: found.map(|p| p.display().to_string()),
        cli_name: DEBRIEF_CLI.to_string(),
    }
}

/// The newest [`MAX_LOG_READ`] bytes of a file as text.
pub fn read_tail(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    if len > MAX_LOG_READ {
        f.seek(SeekFrom::Start(len - MAX_LOG_READ))?;
    }
    let mut bytes = Vec::new();
    f.take(MAX_LOG_READ).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Cuts the job out of the runner log (`log`, also its rotated `.1` when the job is older), writes it to `out_dir` and
/// opens it in Debrief when found at `program`. `start` runs the program (replaceable in tests).
pub fn hand_off(
    log: &Path,
    runner: &str,
    label: &str,
    window: Option<(Timestamp, Timestamp)>,
    out_dir: &Path,
    program: Option<&Path>,
    start: &dyn Fn(&Path, &Path) -> Result<(), String>,
) -> Result<Handoff, String> {
    let mut text = read_tail(log).map_err(|e| format!("the runner's log could not be read: {e}"))?;
    if let Some((from, to)) = window {
        let (mut cut, mut any) = debrief_link::extract_window(&text, from, to);
        if !any {
            let mut older = log.as_os_str().to_os_string();
            older.push(".1");
            if let Ok(prev) = read_tail(Path::new(&older)) {
                (cut, any) = debrief_link::extract_window(&prev, from, to);
            }
        }
        if !any {
            return Ok(Handoff::NoLines {
                message: "The runner's log has no lines for this job any more (it was rotated away). Open the whole log instead.".into(),
            });
        }
        text = cut;
    }
    std::fs::create_dir_all(out_dir).map_err(|e| format!("the hand-off folder could not be made: {e}"))?;
    let file = out_dir.join(debrief_link::handoff_file_name(runner, label));
    std::fs::write(&file, text).map_err(|e| format!("the log could not be written for Debrief: {e}"))?;
    let shown = file.display().to_string();
    match program {
        Some(p) => {
            start(p, &file)?;
            Ok(Handoff::Opened { file: shown })
        }
        None => Ok(Handoff::NotInstalled {
            file: shown,
            message: format!("Debrief was not found (looked for `{DEBRIEF_CLI}`). Install Debrief, or open this file in it by hand."),
        }),
    }
}

/// Starts `program file` without waiting for it (Debrief runs on).
pub fn start(program: &Path, file: &Path) -> Result<(), String> {
    // The program is Debrief's own, found by name on PATH or in an install folder, started detached: the app must not
    // wait for a window that stays open.
    #[allow(clippy::disallowed_methods)]
    let mut cmd = std::process::Command::new(program);
    cmd.arg(file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("Debrief could not be started: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("pitcrew-handoff-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    const LOG: &str =
        "time=\"2026-10-22T12:10:00+00:00\" level=info msg=\"task started\"\ntime=\"2026-10-22T12:11:00+00:00\" level=info msg=\"done\"\n";

    fn ts(s: &str) -> Timestamp {
        pitcrew_core::time::parse_rfc3339(s).unwrap()
    }

    #[test]
    fn a_job_is_cut_out_written_and_opened_when_debrief_is_found() {
        let d = temp("open");
        let log = d.join("runner.log");
        std::fs::write(&log, LOG).unwrap();
        let started = RefCell::new(vec![]);
        let fake = Path::new("/fake/debrief");
        let r = hand_off(
            &log,
            "mac",
            "build 7",
            Some((ts("2026-10-22T12:10:00Z"), ts("2026-10-22T12:11:00Z"))),
            &d.join("out"),
            Some(fake),
            &|p, f| {
                started.borrow_mut().push((p.to_path_buf(), f.to_path_buf()));
                Ok(())
            },
        )
        .unwrap();
        let Handoff::Opened { file } = r else { panic!("{r:?}") };
        assert!(file.ends_with("mac-build_7.log"));
        assert_eq!(started.borrow().len(), 1);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), LOG);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn without_debrief_the_file_is_still_written_and_the_message_says_what_to_do() {
        let d = temp("missing");
        let log = d.join("runner.log");
        std::fs::write(&log, LOG).unwrap();
        let r = hand_off(&log, "mac", "all", None, &d.join("out"), None, &|_, _| panic!("must not start")).unwrap();
        let Handoff::NotInstalled { file, message } = r else {
            panic!("{r:?}")
        };
        assert!(Path::new(&file).exists() && message.contains(DEBRIEF_CLI));
        // A window with no lines, a missing log and a failing start are answered, not panicked on.
        let none = hand_off(
            &log,
            "mac",
            "x",
            Some((ts("2020-01-01T00:00:00Z"), ts("2020-01-01T01:00:00Z"))),
            &d.join("out"),
            None,
            &|_, _| Ok(()),
        )
        .unwrap();
        assert!(matches!(none, Handoff::NoLines { .. }));
        assert!(hand_off(&d.join("nope.log"), "mac", "x", None, &d.join("out"), None, &|_, _| Ok(())).is_err());
        let failing = hand_off(&log, "mac", "y", None, &d.join("out"), Some(Path::new("/x")), &|_, _| {
            Err("boom".into())
        });
        assert_eq!(failing.unwrap_err(), "boom");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_older_rotated_log_is_tried_when_the_job_is_not_in_the_current_one() {
        let d = temp("rotated");
        let log = d.join("runner.log");
        std::fs::write(&log, "time=\"2026-10-23T09:00:00+00:00\" msg=\"new\"\n").unwrap();
        std::fs::write(d.join("runner.log.1"), LOG).unwrap();
        let r = hand_off(
            &log,
            "m",
            "old",
            Some((ts("2026-10-22T12:10:00Z"), ts("2026-10-22T12:11:00Z"))),
            &d.join("out"),
            None,
            &|_, _| Ok(()),
        )
        .unwrap();
        assert!(matches!(r, Handoff::NotInstalled { .. }));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn debrief_is_found_on_path_or_in_install_places_only_when_it_is_a_file() {
        let d = temp("find");
        let file = d.join(format!("{DEBRIEF_CLI}{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&file, "x").unwrap();
        let path = std::env::join_paths([&d]).unwrap().to_string_lossy().into_owned();
        assert_eq!(find(Some(&path), &|_| None), Some(file));
        assert_eq!(
            find(Some("/definitely/not/here"), &|_| None).filter(|p| p.starts_with("/definitely")),
            None
        );
        let s = status();
        assert_eq!(s.cli_name, DEBRIEF_CLI);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn only_the_newest_part_of_a_huge_log_is_read() {
        let d = temp("tail");
        let f = d.join("a.log");
        std::fs::write(&f, "hello\n").unwrap();
        assert_eq!(read_tail(&f).unwrap(), "hello\n");
        assert!(read_tail(&d.join("none")).is_err());
        let _ = std::fs::remove_dir_all(d);
    }
}
