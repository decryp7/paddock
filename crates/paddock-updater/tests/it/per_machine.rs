//! A per-machine copy (ADR 0053 section 4): no install on quit, the elevated hand-off with its record, the Failed states at the
//! next start (declined prompt, refused signature, an installer that stopped), and the wait for another user's installer.

use crate::support::*;

use paddock_updater::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Elev {
    outcome: ElevatedOutcome,
    runs: Mutex<Vec<Vec<String>>>,
}

impl Elevator for Elev {
    fn run(&self, _program: &Path, args: &[String], _patience: Duration) -> ElevatedOutcome {
        self.runs.lock().unwrap().push(args.to_vec());
        self.outcome.clone()
    }
}

struct Contract;
impl SetupContract for Contract {
    fn check_download(&self, _path: &str) -> Result<(), String> {
        Ok(())
    }
    fn sentence(&self, code: i32) -> Option<String> {
        (code == 26).then(|| {
            "The downloaded update did not pass the signature check. Download it again.".into()
        })
    }
}

struct Lock {
    installing: AtomicBool,
    ends_in_wait: bool,
    waited: Mutex<Vec<Duration>>,
}

impl InstallerLock for Lock {
    fn installing(&self) -> bool {
        self.installing.load(Ordering::SeqCst)
    }
    fn wait(&self, bound: Duration) -> bool {
        self.waited.lock().unwrap().push(bound);
        self.ends_in_wait
    }
}

struct Rig {
    updater: Updater,
    elev: Arc<Elev>,
    records: Arc<RecordStore>,
    dir: tempfile::TempDir,
}

fn config(dir: &Path, version: &str) -> UpdaterConfig {
    let mut c = UpdaterConfig::new(
        version,
        Some(key()),
        vec![Channel::new("stable", "Stable", MANIFEST_URL)],
        dir.join("downloads"),
    );
    c.scope = InstallScope::PerMachine;
    c.install_dir = Some(PathBuf::from(r"C:\Program Files\Dyno"));
    c.administrator = Some(false);
    c
}

fn updater(
    dir: &Path,
    records: &Arc<RecordStore>,
    server: &Arc<FakeServer>,
    version: &str,
) -> Updater {
    Updater::new(
        config(dir, version),
        downloader(server),
        records.clone(),
        Box::new(|_| {}),
    )
}

/// A per-machine updater with a verified 9.9.9 download, installing through an `ElevatedHandoff` over `outcome`.
fn ready(outcome: ElevatedOutcome) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let server = FakeServer::new();
    server.json(
        MANIFEST_URL,
        &manifest("9.9.9", &signature("update.bin.sig"), true),
    );
    server.route(PACKAGE_URL, Route::Body(200, package()));
    let records = Arc::new(RecordStore::new(Some(&dir.path().join("config"))));
    let elev = Arc::new(Elev {
        outcome,
        runs: Mutex::new(Vec::new()),
    });
    let handoff = ElevatedHandoff::new(r"C:\Program Files\Dyno", elev.clone(), Arc::new(Contract));
    let updater = updater(dir.path(), &records, &server, "1.0.0").with_installer(Arc::new(handoff));
    let (reg, _sink) = jobs();
    let (_, t) = updater.begin_check().unwrap();
    let ctx = job(&reg);
    ctx.run(|c| updater.run_check(c, t)).unwrap();
    let (_, t) = updater.begin_download().unwrap();
    let ctx = job(&reg);
    ctx.run(|c| updater.run_download(c, t.unwrap())).unwrap();
    Rig {
        updater,
        elev,
        records,
        dir,
    }
}

#[test]
fn install_on_quit_does_not_exist_for_a_per_machine_copy() {
    let r = ready(ElevatedOutcome::Exited(0));
    let s = r.updater.status();
    assert!(!s.install.install_on_quit_available);
    assert_eq!(s.install.scope, InstallScope::PerMachine);
    let e = r.updater.install_on_quit(true).unwrap_err();
    assert_eq!(e, UpdateError::PerMachine);
    assert_eq!(
        e.message(),
        "This copy is installed for all users, so updates are installed when you click Install."
    );
    assert!(matches!(
        r.updater.status().phase,
        UpdatePhase::Ready {
            install_on_quit: false,
            ..
        }
    ));
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Nothing,
        "nothing is installed at quit"
    );
    assert!(r.elev.runs.lock().unwrap().is_empty());
}

#[test]
fn a_standard_users_status_carries_the_administrator_text() {
    let r = ready(ElevatedOutcome::Exited(0));
    assert_eq!(
        r.updater.status().install.administrator_notice.as_deref(),
        Some(
            "An administrator must install this update. Windows will ask for an administrator's name and password."
        )
    );
}

#[test]
fn install_hands_the_download_and_the_signature_text_to_the_elevated_program_and_keeps_the_record()
{
    let r = ready(ElevatedOutcome::Exited(0));
    r.updater.prepare_restart().unwrap();
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Installed { relaunch: false }
    );
    let runs = r.elev.runs.lock().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0][0], "apply-update");
    assert!(
        runs[0].contains(&signature("update.bin.sig")),
        "the signature goes as base64 text"
    );
    assert!(
        runs[0]
            .windows(2)
            .any(|w| w[0] == "--version" && w[1] == "9.9.9")
    );
    let rec = r.records.load();
    assert_eq!(
        rec.handoff.as_ref().map(|h| h.version.as_str()),
        Some("9.9.9")
    );
    assert!(rec.failed_install.is_none());
}

#[test]
fn a_declined_prompt_starts_the_old_version_and_shows_the_permission_text_at_the_next_start() {
    let r = ready(ElevatedOutcome::Declined);
    r.updater.prepare_restart().unwrap();
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Failed { relaunch: true }
    );
    assert!(
        r.records.load().handoff.is_none(),
        "no installer runs, so nothing to settle"
    );
    // The old version starts again: its updater shows the recorded failure once.
    let again = updater(r.dir.path(), &r.records, &FakeServer::new(), "1.0.0");
    match again.status().phase {
        UpdatePhase::Failed { error, .. } => {
            assert_eq!(error, UpdateError::Declined);
            assert_eq!(
                error.message(),
                "Not installed: Windows permission was declined."
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(r.records.load().failed_install.is_none(), "shown once");
}

#[test]
fn a_refused_signature_is_shown_with_its_sentence_at_the_next_start() {
    let r = ready(ElevatedOutcome::Exited(26));
    r.updater.prepare_restart().unwrap();
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Failed { relaunch: true }
    );
    let again = updater(r.dir.path(), &r.records, &FakeServer::new(), "1.0.0");
    let UpdatePhase::Failed { error, .. } = again.status().phase else {
        panic!("expected Failed")
    };
    assert_eq!(
        error.message(),
        "Not installed: The downloaded update did not pass the signature check. Download it again."
    );
}

#[test]
fn a_wait_that_runs_out_exits_like_a_hand_off_and_keeps_the_record() {
    let r = ready(ElevatedOutcome::StillRunning);
    r.updater.prepare_restart().unwrap();
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Installed { relaunch: false }
    );
    assert!(r.records.load().handoff.is_some());
}

#[test]
fn the_new_version_clears_the_record_at_its_first_start() {
    let r = ready(ElevatedOutcome::Exited(0));
    r.updater.prepare_restart().unwrap();
    r.updater.install_at_exit();
    let new = updater(r.dir.path(), &r.records, &FakeServer::new(), "9.9.9");
    assert_eq!(new.status().phase, UpdatePhase::Idle);
    assert!(r.records.load().handoff.is_none());
}

fn old_version_after_a_hand_off(lock: Arc<Lock>) -> (Updater, Rig) {
    let r = ready(ElevatedOutcome::Exited(0));
    r.updater.prepare_restart().unwrap();
    r.updater.install_at_exit();
    let old =
        updater(r.dir.path(), &r.records, &FakeServer::new(), "1.0.0").with_installer_lock(lock);
    (old, r)
}

#[test]
fn the_old_version_waits_while_the_installer_runs_then_shows_that_it_stopped() {
    let lock = Arc::new(Lock {
        installing: AtomicBool::new(true),
        ends_in_wait: true,
        waited: Mutex::new(Vec::new()),
    });
    let (old, r) = old_version_after_a_hand_off(lock.clone());
    assert_eq!(
        old.status().phase,
        UpdatePhase::InstallerRunning {
            version: "9.9.9".into()
        },
        "An update is being installed"
    );
    assert!(
        old.begin_check().is_err(),
        "nothing starts while the installer runs"
    );
    assert!(old.settle_handoff(Duration::from_secs(7)));
    assert_eq!(
        lock.waited.lock().unwrap().as_slice(),
        [Duration::from_secs(7)],
        "a wait with the required bound"
    );
    let UpdatePhase::Failed { error, info } = old.status().phase else {
        panic!("expected Failed")
    };
    assert!(info.is_none());
    let m = error.message();
    assert!(
        m.starts_with("Not installed: The installer stopped before it finished."),
        "{m}"
    );
    assert!(m.contains("setup.log"), "{m}");
    assert!(r.records.load().handoff.is_none(), "the record is cleared");
    assert!(old.begin_check().is_ok(), "Install again is possible");
}

#[test]
fn a_wait_that_times_out_leaves_the_record_for_the_next_start() {
    let lock = Arc::new(Lock {
        installing: AtomicBool::new(true),
        ends_in_wait: false,
        waited: Mutex::new(Vec::new()),
    });
    let (old, r) = old_version_after_a_hand_off(lock);
    assert!(!old.settle_handoff(Duration::from_millis(1)));
    assert!(matches!(
        old.status().phase,
        UpdatePhase::InstallerRunning { .. }
    ));
    assert!(r.records.load().handoff.is_some());
}

#[test]
fn an_installer_that_is_already_gone_is_not_waited_for() {
    let lock = Arc::new(Lock {
        installing: AtomicBool::new(false),
        ends_in_wait: false,
        waited: Mutex::new(Vec::new()),
    });
    let (old, _r) = old_version_after_a_hand_off(lock.clone());
    assert!(old.settle_handoff(Duration::from_secs(1)));
    assert!(lock.waited.lock().unwrap().is_empty());
    assert!(matches!(old.status().phase, UpdatePhase::Failed { .. }));
}

#[test]
fn a_per_user_copy_is_untouched_by_all_of_this() {
    let dir = tempfile::tempdir().unwrap();
    let records = Arc::new(RecordStore::new(Some(dir.path())));
    let u = Updater::new(
        UpdaterConfig::new(
            "1.0.0",
            Some(key()),
            vec![Channel::new("stable", "Stable", MANIFEST_URL)],
            dir.path(),
        ),
        downloader(&FakeServer::new()),
        records,
        Box::new(|_| {}),
    );
    let s = u.status();
    assert_eq!(s.install.scope, InstallScope::PerUser);
    assert!(s.install.install_on_quit_available && s.install.administrator_notice.is_none());
    assert!(!u.settle_handoff(Duration::from_secs(1)));
}
