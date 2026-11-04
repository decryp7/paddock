//! `stop_all` against the real Windows APIs (the process table, handles, `SeDebugPrivilege`, the Restart Manager check), with
//! children of this test binary copied into a temporary folder (ER-14: only processes the test started, only its own
//! folder). Run on the Windows VM (`cargo xwin test --no-run`, then the test executable on the guest).

use super::*;
use crate::testchild::{self, Mode};
use std::process::{Child, Stdio};

const NAMES: [&str; 2] = ["pd-app.exe", "pd-agent.exe"];

/// A child that never outlives the test.
struct Guarded(Child);

impl Drop for Guarded {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn copy_into(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let to = dir.join(name);
    std::fs::copy(std::env::current_exe().unwrap(), &to).unwrap();
    to
}

fn start(exe: &Path) -> Guarded {
    let mut c = testchild::child_of(exe, Mode::Sleep);
    c.stdout(Stdio::null()).stderr(Stdio::null());
    Guarded(c.spawn().expect("the copy of the test binary starts"))
}

fn gone(g: &mut Guarded) -> bool {
    // The child was ended by stop_all, so it has exited; wait() returns at once (a deadline of the OS, not a sleep).
    g.0.wait().is_ok()
}

fn here() -> (ProcessId, Option<PathBuf>) {
    (ProcessId::of(std::process::id()).unwrap(), std::env::current_exe().ok())
}

/// Whether this token can enable SeDebugPrivilege (an elevated administrator's can; a standard user's cannot).
fn elevated() -> bool {
    let mut m = WindowsMachine;
    let ok = m.enable_privilege().is_ok();
    m.drop_privilege();
    ok
}

fn images() -> Vec<String> {
    NAMES.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn programs_in_the_folder_are_ended_and_a_lookalike_folder_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app");
    let lookalike = tmp.path().join("app-helper");
    let mut a = start(&copy_into(&app, NAMES[0]));
    let mut b = start(&copy_into(&app, NAMES[1]));
    let mut decoy = start(&copy_into(&lookalike, NAMES[0]));
    let (own, own_image) = here();
    let imgs = images();
    let req = Request { dir: &app, images: &imgs, own, own_image: own_image.as_deref() };
    let was_elevated = elevated();
    let report = stop_all(&mut WindowsMachine, &req, &Bounds::default());
    if !was_elevated {
        // A standard token cannot enable SeDebugPrivilege: stop_all says so and ends nothing (the installer is elevated).
        assert_eq!(report.exit_code(), EXIT_STILL_RUNNING, "{report:?}");
        assert!(matches!(report.privilege_error, Some(StopError::Privilege(_))));
        assert!(a.0.try_wait().unwrap().is_none() && b.0.try_wait().unwrap().is_none());
        return;
    }
    assert_eq!(report.exit_code(), 0, "{report:?}");
    assert!(gone(&mut a) && gone(&mut b), "{report:?}");
    assert!(decoy.0.try_wait().unwrap().is_none(), "the program of the lookalike folder runs on");
    assert_eq!(report.actions.iter().filter(|x| x.outcome == Outcome::Ended).count(), 2, "{report:?}");
    assert!(report.actions.iter().any(|x| matches!(&x.outcome, Outcome::Skipped(w) if w.contains("outside"))), "{report:?}");
    assert!(report.actions.iter().all(|x| x.id.is_some() && x.session.is_some()), "{report:?}");
}

#[test]
fn the_privilege_is_enabled_by_stop_all_and_dropped_again() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app");
    let mut a = start(&copy_into(&app, NAMES[0]));
    let (own, own_image) = here();
    let imgs = images();
    let req = Request { dir: &app, images: &imgs, own, own_image: own_image.as_deref() };
    // An elevated administrator's token holds SeDebugPrivilege (disabled, or enabled in an elevated SSH or PowerShell
    // session): disable it first so a missing enable cannot hide. A standard token does not hold it.
    let mut m = WindowsMachine;
    let elevated = m.enable_privilege().is_ok();
    m.drop_privilege();
    let report = stop_all(&mut m, &req, &Bounds::default());
    if elevated {
        assert_eq!(report.exit_code(), 0, "{report:?}");
        assert!(gone(&mut a), "{report:?}");
        assert!(report.privilege_error.is_none());
        // Dropped again: enabling works from a clean state and nothing was left enabled by stop_all itself.
        assert!(m.enable_privilege().is_ok());
        m.drop_privilege();
    } else {
        assert_eq!(report.exit_code(), EXIT_STILL_RUNNING, "{report:?}");
        assert!(matches!(report.privilege_error, Some(StopError::Privilege(_))));
        assert!(a.0.try_wait().unwrap().is_none());
    }
}

#[test]
fn the_stopper_survives_clearing_its_own_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app");
    let me = copy_into(&app, "pd-self.exe");
    let mut victim = start(&copy_into(&app, "pd-victim.exe"));
    let mut c = testchild::child_of(&me, Mode::StopSelf);
    c.env("PADDOCK_STOP_DIR", &app).env("PADDOCK_STOP_IMAGES", "pd-self.exe,pd-victim.exe").stdout(Stdio::piped());
    let mut stopper = Guarded(c.spawn().unwrap());
    let mut text = String::new();
    {
        use std::io::{BufRead, BufReader};
        let out = stopper.0.stdout.take().unwrap();
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            text.push_str(&line);
            text.push('\n');
            if line.contains("alive") {
                break;
            }
        }
    }
    let lines = testchild::lines(&text);
    assert_eq!(lines.last(), Some(&"alive"), "the stopper reported after clearing: {text}");
    assert!(stopper.0.try_wait().unwrap().is_none(), "the stopper itself was not ended");
    if elevated() {
        assert!(lines[0].starts_with("exit 0"), "{text}");
        assert!(gone(&mut victim));
    } else {
        assert!(lines[0].starts_with("exit 11"), "{text}");
        assert!(victim.0.try_wait().unwrap().is_none());
    }
}
