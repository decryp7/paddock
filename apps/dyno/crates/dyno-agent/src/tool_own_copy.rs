//! Dyno's own copy of smartctl on macOS (ADR 0027 section 7): the verified disk image is opened read-only, the package inside is
//! expanded into a staging folder, and only `smartctl` and `drivedb.h` are copied into `<app support>/tools/smartmontools-<version>/`.
//! Nothing is installed system-wide and no administrator rights are used; the helper is not involved. The bytes arrive here after
//! the download job checked the pinned size and SHA-256 ([`crate::tool_download`]).
//!
//! **Quarantine, honestly.** Dyno writes no quarantine flag when it downloads, and a file extracted from a mounted image does not
//! inherit one, so normally there is nothing to remove. If `smartctl` does carry `com.apple.quarantine` (the user's own click
//! fetched it from the publisher), Dyno removes that flag from that one file only, and says so in the log. The upstream package is
//! unsigned: after copying, `smartctl --version` runs once; if macOS refuses to run it, the copy is deleted and the message names the
//! cause and the alternative. Logs carry phases and outcomes, never paths of the user's files.

use crate::tool_download::ToolSink;
use dyno_core::helper_requests::ToolKind;
use dyno_core::helper_wire::ToolStatus;
use dyno_core::ports::PlatformError;
use dyno_platform::runner::{tool, ToolRunner};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const HDIUTIL: &str = "/usr/bin/hdiutil";
const PKGUTIL: &str = "/usr/sbin/pkgutil";
const XATTR: &str = "/usr/bin/xattr";
const QUARANTINE: &str = "com.apple.quarantine";
const STEP: Duration = Duration::from_secs(60);
const RUN: Duration = Duration::from_secs(10);

/// The extraction, over a [`ToolRunner`] and two folders.
pub struct OwnCopy {
    runner: Arc<dyn ToolRunner>,
    work: PathBuf,
    tools: PathBuf,
    version: String,
}

fn fail(m: impl Into<String>) -> PlatformError {
    PlatformError::Failed(m.into())
}

fn name_of(version: &str) -> String {
    format!("smartmontools-{version}")
}

/// The version of Dyno's own copy under `tools`, when it is there.
pub fn installed(tools: &Path, version: &str) -> Option<String> {
    tools.join(name_of(version)).join("smartctl").is_file().then(|| version.to_string())
}

/// Removes Dyno's own copies of smartmontools (every `smartmontools-*` folder under `tools`).
///
/// # Errors
/// The folder could not be removed.
pub fn remove(tools: &Path) -> std::io::Result<()> {
    let Ok(entries) = std::fs::read_dir(tools) else { return Ok(()) };
    for e in entries.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with("smartmontools-") && e.path().is_dir() {
            std::fs::remove_dir_all(e.path())?;
        }
    }
    Ok(())
}

fn find_file(dir: &Path, name: &str, depth: u32) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let mut subdirs = Vec::new();
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            subdirs.push(p);
        } else if p.file_name().is_some_and(|n| n == name) {
            return Some(p);
        }
    }
    subdirs.into_iter().find_map(|d| find_file(&d, name, depth - 1))
}

impl OwnCopy {
    /// `work` is the staging folder this install owns; `tools` the folder of Dyno's own tools; `version` the pinned version.
    pub fn new(runner: Arc<dyn ToolRunner>, work: PathBuf, tools: PathBuf, version: String) -> OwnCopy {
        OwnCopy {
            runner,
            work,
            tools,
            version,
        }
    }

    fn run_ok(&self, program: &str, args: &[&str], timeout: Duration, what: &str) -> Result<String, PlatformError> {
        let out = self.runner.run_tool(&tool(program, args), timeout)?;
        if out.ok {
            Ok(out.stdout)
        } else {
            Err(fail(format!(
                "{what} failed ({}).",
                if out.error.is_empty() { "no message" } else { &out.error }
            )))
        }
    }

    fn mount(&self, dmg: &Path, base: &Path) -> Result<PathBuf, PlatformError> {
        let out = self
            .run_ok(
                HDIUTIL,
                &["attach", "-readonly", "-nobrowse", "-noverify", "-mountrandom", &base.to_string_lossy(), &dmg.to_string_lossy()],
                STEP,
                "Opening the downloaded disk image",
            )
            .map_err(|e| fail(format!("{e} The download was checked, so this is macOS refusing to open it; try again, or install smartmontools yourself (for example with Homebrew).")))?;
        out.lines()
            .filter_map(|l| l.rsplit('\t').next())
            .map(str::trim)
            .find(|p| Path::new(p).starts_with(base))
            .map(PathBuf::from)
            .ok_or_else(|| fail("The disk image opened but its volume was not found. Try again."))
    }

    fn extract(&self, mount: &Path) -> Result<(), PlatformError> {
        let pkg = std::fs::read_dir(mount)
            .map_err(|e| fail(format!("The disk image could not be read ({}).", e.kind())))?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "pkg"))
            .ok_or_else(|| {
                fail("The disk image holds no package, so nothing was installed. Report this; the pinned file may have changed upstream.")
            })?;
        let expanded = self.work.join("expanded");
        let _ = std::fs::remove_dir_all(&expanded);
        self.run_ok(
            PKGUTIL,
            &["--expand-full", &pkg.to_string_lossy(), &expanded.to_string_lossy()],
            STEP,
            "Unpacking the package",
        )?;
        let smartctl = find_file(&expanded, "smartctl", 8)
            .ok_or_else(|| fail("The package holds no smartctl, so nothing was installed. Report this."))?;
        let drivedb = find_file(&expanded, "drivedb.h", 8);

        let staged = self.tools.join(".installing");
        let _ = std::fs::remove_dir_all(&staged);
        std::fs::create_dir_all(&staged).map_err(|e| {
            fail(format!(
                "Dyno's tools folder could not be made ({}). Check free space and permissions.",
                e.kind()
            ))
        })?;
        let done = self.place(&smartctl, drivedb.as_deref(), &staged);
        if done.is_err() {
            let _ = std::fs::remove_dir_all(&staged);
        }
        done
    }

    fn place(&self, smartctl: &Path, drivedb: Option<&Path>, staged: &Path) -> Result<(), PlatformError> {
        let io = |e: std::io::Error| {
            fail(format!(
                "The tool could not be copied ({}). Check free space and permissions.",
                e.kind()
            ))
        };
        let dest = staged.join("smartctl");
        std::fs::copy(smartctl, &dest).map_err(io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).map_err(io)?;
        }
        if let Some(d) = drivedb {
            std::fs::copy(d, staged.join("drivedb.h")).map_err(io)?;
        }
        // Quarantine: removed from this one file only, and only when it is there.
        let d = dest.to_string_lossy().into_owned();
        if self.runner.run_tool(&tool(XATTR, &["-p", QUARANTINE, &d]), RUN).is_ok_and(|o| o.ok) {
            let _ = self.runner.run_tool(&tool(XATTR, &["-d", QUARANTINE, &d]), RUN);
            log::info!(target: "dyno::tool", "quarantine flag removed from the extracted smartctl");
        }
        // One run proves macOS lets it run (the package is unsigned).
        let ver = self.runner.run_tool(&tool(&d, &["--version"]), RUN).map_err(|e| {
            fail(format!(
                "macOS would not run the copied smartctl ({e}). It was removed. Install smartmontools yourself (for example with Homebrew) instead."
            ))
        })?;
        let want = format!("smartctl {}", self.version);
        if !ver.ok || !ver.stdout.contains(&want) {
            return Err(fail(format!(
                "The copied smartctl did not report version {}, so it was removed. Try again; if it repeats, report it.",
                self.version
            )));
        }
        let final_dir = self.tools.join(name_of(&self.version));
        let _ = std::fs::remove_dir_all(&final_dir);
        std::fs::rename(staged, &final_dir).map_err(io)
    }
}

impl ToolSink for OwnCopy {
    fn install(&self, kind: ToolKind, bytes: Vec<u8>) -> Result<ToolStatus, PlatformError> {
        std::fs::create_dir_all(&self.work).map_err(|e| fail(format!("The staging folder could not be made ({}).", e.kind())))?;
        let dmg = self.work.join("smartmontools.dmg");
        let base = self.work.join("mount");
        let _ = std::fs::remove_file(&dmg);
        std::fs::create_dir_all(&base).map_err(|e| fail(format!("The staging folder could not be made ({}).", e.kind())))?;
        std::fs::write(&dmg, &bytes).map_err(|e| fail(format!("The download could not be saved ({}). Check free space.", e.kind())))?;
        let started = std::time::Instant::now();
        let result = self.mount(&dmg, &base).and_then(|mount| {
            let r = self.extract(&mount);
            // Always let go of the image, whatever happened.
            let _ = self
                .runner
                .run_tool(&tool(HDIUTIL, &["detach", &mount.to_string_lossy(), "-quiet"]), STEP);
            r
        });
        let _ = std::fs::remove_dir_all(&self.work);
        match &result {
            Ok(()) => log::info!(target: "dyno::tool", "own copy installed: version={} ms={}", self.version, started.elapsed().as_millis()),
            Err(_) => log::warn!(target: "dyno::tool", "own copy not installed: ms={}", started.elapsed().as_millis()),
        }
        result.map(|()| ToolStatus {
            tool: kind,
            installed: Some(self.version.clone()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_platform::runner::{Tool, ToolOutput};
    use std::sync::Mutex;

    #[derive(Clone, Copy, PartialEq)]
    enum Fault {
        None,
        AttachFails,
        NoPackage,
        NoSmartctl,
        WontRun,
        WrongVersion,
        Quarantined,
    }

    struct Fake {
        fault: Fault,
        calls: Mutex<Vec<String>>,
    }

    fn ok(out: &str) -> Result<ToolOutput, PlatformError> {
        Ok(ToolOutput {
            ok: true,
            stdout: out.into(),
            error: String::new(),
        })
    }

    impl ToolRunner for Fake {
        fn run_tool(&self, t: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            // The file name by the host's own separator: the staged tool's path is built with backslashes on Windows.
            let prog = std::path::Path::new(&t.program)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.calls
                .lock()
                .unwrap()
                .push(format!("{} {}", prog, t.args.first().cloned().unwrap_or_default()));
            match (prog.as_str(), t.args.first().map(String::as_str)) {
                ("hdiutil", Some("attach")) => {
                    if self.fault == Fault::AttachFails {
                        return Ok(ToolOutput {
                            ok: false,
                            stdout: String::new(),
                            error: "image not recognized".into(),
                        });
                    }
                    let base = PathBuf::from(&t.args[5]);
                    let vol = base.join("dmg.ABC123");
                    std::fs::create_dir_all(&vol).unwrap();
                    if self.fault != Fault::NoPackage {
                        std::fs::write(vol.join("smartmontools-7.5-r5714.pkg"), b"pkg").unwrap();
                    }
                    ok(&format!(
                        "/dev/disk9\tGUID_partition_scheme\n/dev/disk9s1\tApple_HFS\t{}\n",
                        vol.display()
                    ))
                }
                ("hdiutil", Some("detach")) => ok(""),
                ("pkgutil", Some("--expand-full")) => {
                    let dest = PathBuf::from(&t.args[2]);
                    if self.fault != Fault::NoSmartctl {
                        let d = dest.join("smartmontools.pkg/Payload/usr/local/sbin");
                        std::fs::create_dir_all(&d).unwrap();
                        std::fs::write(d.join("smartctl"), b"#!/bin/sh\n").unwrap();
                    }
                    let s = dest.join("smartmontools.pkg/Payload/usr/local/share/smartmontools");
                    std::fs::create_dir_all(&s).unwrap();
                    std::fs::write(s.join("drivedb.h"), b"// drivedb").unwrap();
                    ok("")
                }
                ("xattr", Some("-p")) => {
                    if self.fault == Fault::Quarantined {
                        ok("0083;5f;Safari;")
                    } else {
                        Ok(ToolOutput {
                            ok: false,
                            stdout: String::new(),
                            error: "No such xattr: com.apple.quarantine".into(),
                        })
                    }
                }
                ("xattr", Some("-d")) => ok(""),
                ("smartctl", Some("--version")) => match self.fault {
                    Fault::WontRun => Err(PlatformError::Failed("Killed: 9".into())),
                    Fault::WrongVersion => ok("smartctl 7.4 2023-08-01 r5530 [Darwin]"),
                    _ => ok("smartctl 7.5 2025-04-30 r5714 [Darwin 25.0.0 arm64] (local build)"),
                },
                other => panic!("unexpected tool {other:?}"),
            }
        }
    }

    struct Rig {
        own: OwnCopy,
        fake: Arc<Fake>,
        dir: paddock_testkit::PrivateTemp,
    }

    fn rig(fault: Fault) -> Rig {
        let dir = paddock_testkit::private_tempdir().unwrap();
        let fake = Arc::new(Fake {
            fault,
            calls: Mutex::new(Vec::new()),
        });
        let own = OwnCopy::new(fake.clone(), dir.path().join("work"), dir.path().join("tools"), "7.5".into());
        Rig { own, fake, dir }
    }

    fn calls(r: &Rig) -> Vec<String> {
        r.fake.calls.lock().unwrap().clone()
    }

    #[test]
    fn smartctl_and_drivedb_are_copied_into_dynos_own_folder_run_once_and_the_image_is_let_go() {
        let r = rig(Fault::None);
        let st = r.own.install(ToolKind::Smartmontools, vec![1, 2, 3]).unwrap();
        assert_eq!(st.installed.as_deref(), Some("7.5"));
        let folder = r.dir.path().join("tools/smartmontools-7.5");
        assert!(folder.join("smartctl").is_file() && folder.join("drivedb.h").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(folder.join("smartctl")).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        let c = calls(&r);
        assert_eq!(c.first().map(String::as_str), Some("hdiutil attach"));
        assert!(c.contains(&"pkgutil --expand-full".to_string()) && c.contains(&"smartctl --version".to_string()));
        assert_eq!(c.last().map(String::as_str), Some("hdiutil detach"), "the image is let go last");
        assert!(!r.dir.path().join("work").exists(), "the staging folder is gone");
        assert!(!r.dir.path().join("tools/.installing").exists());
        assert!(
            !c.iter().any(|x| x.starts_with("xattr -d")),
            "no quarantine flag, nothing to remove"
        );
        assert_eq!(installed(&r.dir.path().join("tools"), "7.5").as_deref(), Some("7.5"));
    }

    #[test]
    fn a_quarantine_flag_on_the_extracted_file_is_removed_from_that_file_only() {
        let r = rig(Fault::Quarantined);
        r.own.install(ToolKind::Smartmontools, vec![1]).unwrap();
        let c = calls(&r);
        assert_eq!(c.iter().filter(|x| x.starts_with("xattr -d")).count(), 1);
    }

    #[test]
    fn every_failure_leaves_nothing_behind_says_what_happened_and_still_lets_go_of_the_image() {
        for (fault, word) in [
            (Fault::AttachFails, "macOS refusing to open it"),
            (Fault::NoPackage, "no package"),
            (Fault::NoSmartctl, "no smartctl"),
            (Fault::WontRun, "would not run"),
            (Fault::WrongVersion, "did not report version 7.5"),
        ] {
            let r = rig(fault);
            let e = r.own.install(ToolKind::Smartmontools, vec![1]).unwrap_err().to_string();
            assert!(e.contains(word), "{e}");
            assert!(installed(&r.dir.path().join("tools"), "7.5").is_none(), "{word}: no copy remains");
            assert!(!r.dir.path().join("tools/.installing").exists(), "{word}: no staging remains");
            assert!(!r.dir.path().join("work").exists(), "{word}: no work folder remains");
            if fault != Fault::AttachFails {
                assert_eq!(calls(&r).last().map(String::as_str), Some("hdiutil detach"), "{word}");
            }
        }
    }

    #[test]
    fn a_second_install_replaces_the_first_and_removing_deletes_the_folder() {
        let r = rig(Fault::None);
        r.own.install(ToolKind::Smartmontools, vec![1]).unwrap();
        r.own.install(ToolKind::Smartmontools, vec![2]).unwrap();
        let tools = r.dir.path().join("tools");
        assert!(installed(&tools, "7.5").is_some());
        std::fs::create_dir_all(tools.join("other-tool")).unwrap();
        remove(&tools).unwrap();
        assert!(installed(&tools, "7.5").is_none());
        assert!(tools.join("other-tool").is_dir(), "only smartmontools folders are removed");
        remove(&r.dir.path().join("nothing-here")).unwrap();
    }
}
