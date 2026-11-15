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
//! The rule for "external" is `paddock_env::boot_volume` (`diskutil info -plist` of the folder's mount, links resolved, says
//! Internal=false), the one rule that `dev ci boot-volume` (CI's job-tmpdir step, Dyno's `check-bundle.sh`) uses too: this
//! module only runs `df`, `diskutil` and `getconf` for it, each bounded. Outside macOS nothing runs and the temp folder is kept.

use paddock_env::boot_volume::{Probe, boot_volume_temp as rule_temp, mount_from_df};
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// How long each of `df`, `diskutil` and `getconf` may run (about 0.2 s on the CI Mac) before it is stopped and the folder is judged
/// internal, so the temp folder is kept.
pub const RULE_BOUND: Duration = Duration::from_secs(20);

/// How many times this process decided [`boot_volume_temp`] (at most once; for the caching test).
static DECISIONS: AtomicUsize = AtomicUsize::new(0);

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
        let choice = boot_volume_choice(cfg!(target_os = "macos"), &std::env::temp_dir());
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

/// The decision behind [`boot_volume_temp`] for a given system: on macOS the shared rule's answer for `temp` (each program bounded by
/// [`RULE_BOUND`]), elsewhere `temp` without running anything.
pub fn boot_volume_choice(on_macos: bool, temp: &Path) -> BootVolumeChoice {
    if !on_macos {
        return BootVolumeChoice {
            dir: temp.to_path_buf(),
            note: None,
        };
    }
    boot_volume_temp_with(temp, None, RULE_BOUND)
}

/// What [`boot_volume_temp_with`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootVolumeChoice {
    /// The folder to use.
    pub dir: PathBuf,
    /// What the rule said: why the folder changed, or that macOS may prompt; `None` when it said nothing.
    pub note: Option<String>,
}

/// The machine, asked through the programs the rule names, each stopped after its own bound.
struct SystemProbe {
    path: Option<OsString>,
    bound: Duration,
}

impl SystemProbe {
    /// The program's standard output when it ran in time and succeeded.
    fn run(&self, program: &str, args: &[&OsStr]) -> Option<String> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(p) = &self.path {
            cmd.env("PATH", p);
        }
        let mut child = cmd.spawn().ok()?;
        // The rule's programs print a few lines, so the pipe never fills before they end.
        let Some(status) = crate::wait::poll_for(self.bound, || child.try_wait().ok().flatten())
        else {
            // The held child, not a pid (ADR 0033).
            let _ = child.kill();
            let _ = child.wait();
            return None;
        };
        let mut out = String::new();
        child.stdout.take()?.read_to_string(&mut out).ok()?;
        status.success().then_some(out)
    }
}

impl Probe for SystemProbe {
    fn is_dir(&self, p: &str) -> bool {
        Path::new(p).is_dir()
    }

    fn physical(&self, p: &str) -> Option<String> {
        std::fs::canonicalize(p).ok()?.to_str().map(str::to_string)
    }

    fn mount_of(&self, dir: &str) -> Option<String> {
        mount_from_df(&self.run("df", &[OsStr::new("-P"), OsStr::new(dir)])?)
    }

    fn plist_of(&self, mount: &str) -> Option<String> {
        self.run(
            "diskutil",
            &[OsStr::new("info"), OsStr::new("-plist"), OsStr::new(mount)],
        )
    }

    fn user_temp(&self) -> Option<String> {
        self.run("getconf", &[OsStr::new("DARWIN_USER_TEMP_DIR")])
            .map(|t| t.trim_end_matches('\n').to_string())
    }
}

/// Applies the shared rule to `temp`. `path` replaces `PATH` for the programs it runs (`df`, `diskutil`, `getconf`), so a test can
/// put fakes first; each program is stopped after `bound`. When a program cannot run or is stopped the folder is judged internal,
/// so `temp` is kept, and the note says why; when `temp` is on an external volume with no internal boot-volume folder to move to,
/// `temp` is kept and the note says that macOS may prompt.
pub fn boot_volume_temp_with(
    temp: &Path,
    path: Option<&OsStr>,
    bound: Duration,
) -> BootVolumeChoice {
    let Some(given) = temp.to_str() else {
        return BootVolumeChoice {
            dir: temp.to_path_buf(),
            note: Some(format!(
                "::warning::paddock-testkit boot_volume: {} is not valid text, so it cannot be judged; keeping it (if it is on an external volume, macOS may prompt for removable-volume access)",
                temp.display()
            )),
        };
    };
    let probe = SystemProbe {
        path: path.map(OsString::from),
        bound,
    };
    let (dir, notes) = rule_temp(&probe, Some(given), None);
    BootVolumeChoice {
        dir: PathBuf::from(dir),
        note: (!notes.is_empty()).then(|| notes.join("\n")),
    }
}
