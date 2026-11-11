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
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The shared rule, found from this crate's folder in the Paddock repository.
pub fn rule_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/ci/boot-volume.sh")
}

/// The temp folder (`TMPDIR`) when it is not on an external volume, else the per-user temp folder of the boot volume
/// (`getconf DARWIN_USER_TEMP_DIR`). Decided once per process; the reason for a change, or a warning that macOS may
/// prompt when no boot-volume folder could be found, is printed to stderr once (ER-27). Outside macOS: the temp folder.
pub fn boot_volume_temp() -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let temp = std::env::temp_dir();
        if !cfg!(target_os = "macos") {
            return temp;
        }
        let choice = boot_volume_temp_with(&rule_script(), &temp, None);
        if let Some(note) = &choice.note {
            eprintln!("{note}");
        }
        choice.dir
    })
    .clone()
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
/// can put fake `df`, `diskutil` and `getconf` first. When the script cannot run or prints no absolute folder, `temp` is
/// kept and the note says that macOS may prompt.
pub fn boot_volume_temp_with(script: &Path, temp: &Path, path: Option<&OsStr>) -> BootVolumeChoice {
    let keep = |why: String| BootVolumeChoice {
        dir: temp.to_path_buf(),
        note: Some(format!(
            "::warning::paddock-testkit boot_volume: {why}; keeping {} (if it is on an external volume, macOS may prompt for removable-volume access)",
            temp.display()
        )),
    };
    let mut cmd = Command::new("/bin/bash");
    cmd.arg(script).arg("temp").arg(temp);
    if let Some(p) = path {
        cmd.env("PATH", p);
    }
    let out = match cmd.output() {
        Ok(out) => out,
        Err(e) => return keep(format!("{} could not run: {e}", script.display())),
    };
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || !dir.starts_with('/') {
        return keep(format!(
            "{} ended with {} and printed {dir:?} ({stderr})",
            script.display(),
            out.status
        ));
    }
    BootVolumeChoice {
        dir: PathBuf::from(dir),
        note: (!stderr.is_empty()).then_some(stderr),
    }
}
