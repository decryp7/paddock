//! [`boot_volume_temp`]: a temp folder on the boot volume for the programs a test starts as their own *responsible process*.
//!
//! macOS attributes a program's file access to its responsible process. A child started with the responsibility disclaim
//! (Pitcrew's runner, Pitcrew ADR 0020) or by launchd (`launchctl submit`) is its own, so the Full Disk Access of whatever
//! started the test does not cover it. When such a program touches a folder on an external disk, TCC (Removable Volumes)
//! asks the user "<program> would like to access files on a removable volume", or denies without asking for a platform
//! binary such as `/bin/sh`. TCC keys an unbundled ad-hoc program by its path, so a test that copies it to a new folder
//! each run asks again each run (docs/lessons.md, 2026-11-04 and 2026-11-11). A test that starts such a program keeps it
//! and its working folder under [`boot_volume_temp`], never under a `TMPDIR` that may sit on an external volume.
//!
//! The rule for "external" is the one in `conformance/ci/boot-volume.sh` (`diskutil info -plist` of the folder's mount, links
//! resolved, says Internal=false), which CI's `job-tmpdir.sh` and Dyno's `check-bundle.sh` use too: this module runs that
//! script rather than repeating it. Outside macOS nothing runs and the temp folder is kept.

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// How long the rule may run (`df`, `diskutil` and `getconf`, about 0.2 s on the CI Mac) before it is stopped and the temp
/// folder is kept.
pub const RULE_BOUND: Duration = Duration::from_secs(20);

/// How many times this process decided [`boot_volume_temp`] (at most once; for the caching test).
static DECISIONS: AtomicUsize = AtomicUsize::new(0);

/// The shared rule, found from this crate's folder. It exists only when the crate is built inside the Paddock repository
/// (every app depends on it by path); a copy of the crate built from the package registry has no `conformance/` next to
/// it, so the rule cannot run and [`boot_volume_temp`] keeps the temp folder with a warning that macOS may prompt.
pub fn rule_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/ci/boot-volume.sh")
}

/// The temp folder (`TMPDIR`) when it is not on an external volume, else the per-user temp folder of the boot volume
/// (`getconf DARWIN_USER_TEMP_DIR`). Outside macOS: the temp folder, and nothing runs.
///
/// Decided once per process: under `cargo nextest` every test is its own process, so the rule runs once per test that
/// calls this; under `cargo test` once per test binary. The reason for a change, or the warning that macOS may prompt, is
/// written with `eprintln!`, so the test harness captures it with the test's output and shows it when the test fails (or
/// with `--nocapture`); the folder itself is in every failure message that names a path below it.
pub fn boot_volume_temp() -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        DECISIONS.fetch_add(1, Ordering::SeqCst);
        let choice = boot_volume_choice(
            cfg!(target_os = "macos"),
            &rule_script(),
            &std::env::temp_dir(),
        );
        if let Some(note) = &choice.note {
            eprintln!("{note}");
        }
        choice.dir
    })
    .clone()
}

/// How many times this process has decided [`boot_volume_temp`]: 0 before the first call, 1 after any number of calls.
pub fn boot_volume_decisions() -> usize {
    DECISIONS.load(Ordering::SeqCst)
}

/// The decision behind [`boot_volume_temp`] for a given system: on macOS the shared rule's answer for `temp` (bounded by
/// [`RULE_BOUND`]), elsewhere `temp` without running anything.
pub fn boot_volume_choice(on_macos: bool, script: &Path, temp: &Path) -> BootVolumeChoice {
    if !on_macos {
        return BootVolumeChoice {
            dir: temp.to_path_buf(),
            note: None,
        };
    }
    boot_volume_temp_with(script, temp, None, RULE_BOUND)
}

/// What [`boot_volume_temp_with`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootVolumeChoice {
    /// The folder to use.
    pub dir: PathBuf,
    /// What the rule said on stderr: why the folder changed, or that macOS may prompt; `None` when it said nothing.
    pub note: Option<String>,
}

/// Runs `script temp <temp>` (the shared rule) and returns its folder. `path` replaces `PATH` for the script, so a test
/// can put fake `df`, `diskutil` and `getconf` first. The script is stopped after `bound`. When it cannot run, is stopped,
/// fails or prints no absolute folder, `temp` is kept and the note says that macOS may prompt. The rule prints a line or
/// two, so its pipes never fill before it ends.
pub fn boot_volume_temp_with(
    script: &Path,
    temp: &Path,
    path: Option<&OsStr>,
    bound: Duration,
) -> BootVolumeChoice {
    let keep = |why: String| BootVolumeChoice {
        dir: temp.to_path_buf(),
        note: Some(format!(
            "::warning::paddock-testkit boot_volume: {why}; keeping {} (if it is on an external volume, macOS may prompt for removable-volume access)",
            temp.display()
        )),
    };
    let mut cmd = Command::new("/bin/bash");
    cmd.arg(script)
        .arg("temp")
        .arg(temp)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(p) = path {
        cmd.env("PATH", p);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => return keep(format!("{} could not run: {e}", script.display())),
    };
    let Some(status) = crate::wait::poll_for(bound, || child.try_wait().ok().flatten()) else {
        // The held child, not a pid (ADR 0033); its pipes are dropped unread.
        let _ = child.kill();
        let _ = child.wait();
        return keep(format!(
            "{} did not finish within {} ms and was stopped",
            script.display(),
            bound.as_millis()
        ));
    };
    let (mut out, mut err) = (String::new(), String::new());
    if let Some(mut s) = child.stdout.take() {
        let _ = s.read_to_string(&mut out);
    }
    if let Some(mut s) = child.stderr.take() {
        let _ = s.read_to_string(&mut err);
    }
    let (dir, stderr) = (out.trim().to_string(), err.trim().to_string());
    if !status.success() || !dir.starts_with('/') {
        return keep(format!(
            "{} ended with {status} and printed {dir:?} ({stderr})",
            script.display()
        ));
    }
    BootVolumeChoice {
        dir: PathBuf::from(dir),
        note: (!stderr.is_empty()).then_some(stderr),
    }
}
