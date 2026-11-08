//! The updater against a fake update server (an in-memory transport): check, download through `paddock-download`, the
//! signature check, cancel in every phase, preferences, the exit install and the services' hooks.

use crate::support;

use paddock_download::{TransportError, TransportErrorKind};
use paddock_jobs::{JobError, JobRegistry, JobState};
use paddock_updater::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use support::*;

struct Rig {
    updater: Arc<Updater>,
    server: Arc<FakeServer>,
    statuses: Arc<Mutex<Vec<UpdateStatus>>>,
    reg: Arc<JobRegistry>,
    sink: Arc<Sink>,
    records: Arc<RecordStore>,
    dir: tempfile::TempDir,
}

fn channels() -> Vec<Channel> {
    vec![
        Channel::new("stable", "Stable", MANIFEST_URL),
        Channel::new("beta", "Beta", BETA_URL),
    ]
}

fn config(dir: &Path, key: Option<UpdateKey>, version: &str) -> UpdaterConfig {
    let mut c = UpdaterConfig::new(version, key, channels(), dir.join("downloads"));
    c.releases_page = Some("https://updates.test/releases".into());
    c
}

fn rig_with(
    version: &str,
    key: Option<UpdateKey>,
    dir: tempfile::TempDir,
    edit: impl FnOnce(Updater) -> Updater,
) -> Rig {
    let server = FakeServer::new();
    let records = Arc::new(RecordStore::new(Some(&dir.path().join("config"))));
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let seen = statuses.clone();
    let updater = Updater::new(
        config(dir.path(), key, version),
        downloader(&server),
        records.clone(),
        Box::new(move |s| seen.lock().unwrap().push(s.clone())),
    );
    let (reg, sink) = jobs();
    Rig {
        updater: Arc::new(edit(updater)),
        server,
        statuses,
        reg,
        sink,
        records,
        dir,
    }
}

fn rig() -> Rig {
    rig_with("1.0.0", Some(key()), tempfile::tempdir().unwrap(), |u| u)
}

impl Rig {
    fn publish(&self, version: &str, sig: &str) {
        self.server
            .json(MANIFEST_URL, &manifest(version, &signature(sig), true));
        self.server.route(PACKAGE_URL, Route::Body(200, package()));
    }

    fn check(&self) -> Result<(), JobError> {
        let (status, ticket) = self.updater.begin_check().unwrap();
        assert!(matches!(status.phase, UpdatePhase::Checking { job: None }));
        let ctx = job(&self.reg);
        self.updater.set_job(ticket, ctx.id().0);
        assert_eq!(
            self.phase(),
            UpdatePhase::Checking {
                job: Some(ctx.id().0)
            }
        );
        ctx.run(|c| self.updater.run_check(c, ticket))
    }

    fn download(&self) -> Result<(), JobError> {
        let (_, ticket) = self.updater.begin_download().unwrap();
        let ticket = ticket.expect("a download to run");
        let ctx = job(&self.reg);
        self.updater.set_job(ticket, ctx.id().0);
        ctx.run(|c| self.updater.run_download(c, ticket))
    }

    fn phase(&self) -> UpdatePhase {
        self.updater.status().phase
    }

    fn downloads(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.dir.path().join("downloads"))
            .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default()
    }
}

#[derive(Default)]
struct FakeInstaller {
    calls: Mutex<Vec<(PathBuf, String, bool)>>,
    fail: Option<UpdateError>,
}

impl Installer for FakeInstaller {
    fn install(&self, pkg: &Package<'_>, relaunch: bool) -> Result<(), UpdateError> {
        let (file, manifest) = (pkg.file, pkg.manifest);
        assert_eq!(
            std::fs::read(file).unwrap(),
            package(),
            "the verified package is handed over"
        );
        self.calls.lock().unwrap().push((
            file.to_path_buf(),
            manifest["version"].as_str().unwrap().to_string(),
            relaunch,
        ));
        self.fail.clone().map_or(Ok(()), Err)
    }
}

#[derive(Default)]
struct FakeHooks {
    seen: Mutex<Vec<String>>,
    fail_stop: bool,
    fail_register: Mutex<bool>,
}

impl ServiceHooks for FakeHooks {
    fn before_install(&self) -> Result<(), String> {
        self.seen.lock().unwrap().push("stop".into());
        if self.fail_stop {
            Err("launchctl said no".into())
        } else {
            Ok(())
        }
    }
    fn on_version_change(&self, previous: Option<&str>, current: &str) -> Result<(), String> {
        self.seen.lock().unwrap().push(format!(
            "register {} -> {current}",
            previous.unwrap_or("none")
        ));
        if *self.fail_register.lock().unwrap() {
            Err("bootout timed out".into())
        } else {
            Ok(())
        }
    }
}

// ---- builds without updates ----

#[test]
fn a_build_without_a_key_says_updates_are_not_available_and_checks_nothing() {
    let r = rig_with("1.0.0", None, tempfile::tempdir().unwrap(), |u| u);
    let s = r.updater.status();
    assert_eq!(s.phase, UpdatePhase::Unavailable);
    assert!(!r.updater.available());
    assert_eq!(s.endpoint_host, None, "nothing would be contacted");
    assert_eq!(
        r.updater.begin_check().unwrap_err(),
        UpdateError::NotAvailable
    );
    assert_eq!(
        r.updater.begin_check().unwrap_err().message(),
        "Updates are not available in this build."
    );
    assert_eq!(r.updater.cancel().unwrap_err(), UpdateError::NotAvailable);
    assert!(!r.updater.auto_check_due());
    assert!(r.server.hits.lock().unwrap().is_empty());
    // No channel, or a running version that is not a semantic version, is the same.
    let dir = tempfile::tempdir().unwrap();
    let none = Updater::new(
        UpdaterConfig::new("1.0.0", Some(key()), vec![], dir.path()),
        downloader(&r.server),
        r.records.clone(),
        Box::new(|_| {}),
    );
    assert_eq!(none.status().phase, UpdatePhase::Unavailable);
    assert_eq!(none.status().channel, "");
    let odd = Updater::new(
        config(dir.path(), Some(key()), "dev"),
        downloader(&r.server),
        r.records.clone(),
        Box::new(|_| {}),
    );
    assert_eq!(odd.status().phase, UpdatePhase::Unavailable);
}

// ---- check ----

#[test]
fn a_check_finds_a_newer_release_for_this_system() {
    let r = rig();
    r.publish("9.9.9", "update.bin.sig");
    let s = r.updater.status();
    assert_eq!(
        (
            s.current_version.as_str(),
            s.channel.as_str(),
            s.endpoint_host.as_deref()
        ),
        ("1.0.0", "stable", Some("updates.test"))
    );
    assert_eq!(
        s.channels.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["stable", "beta"]
    );
    assert_eq!(
        s.releases_page.as_deref(),
        Some("https://updates.test/releases")
    );
    r.check().unwrap();
    let UpdatePhase::Available { info } = r.phase() else {
        panic!("{:?}", r.phase())
    };
    assert_eq!(
        (info.version.as_str(), info.current_version.as_str()),
        ("9.9.9", "1.0.0")
    );
    assert_eq!(
        (info.notes.as_str(), info.size),
        ("Faster.", Some(package().len() as u64))
    );
    assert_eq!(info.pub_date.as_deref(), Some("2026-10-24T08:00:00Z"));
    assert!(r.updater.status().last_checked.is_some());
    assert_eq!(
        r.records.load().last_checked,
        r.updater.status().last_checked,
        "kept for the daily check"
    );
    assert_eq!(r.sink.phases(), [CHECK_PHASE]);
    // Every change was sent as a status.
    let kinds: Vec<_> = r
        .statuses
        .lock()
        .unwrap()
        .iter()
        .map(|s| std::mem::discriminant(&s.phase))
        .collect();
    assert_eq!(kinds.len(), 3, "checking, its job, available");
}

#[test]
fn the_same_or_an_older_release_is_up_to_date() {
    for v in ["1.0.0", "0.9.0"] {
        let r = rig();
        r.publish(v, "update.bin.sig");
        r.check().unwrap();
        assert_eq!(r.phase(), UpdatePhase::UpToDate, "{v}");
    }
}

fn failed_check(setup: impl FnOnce(&FakeServer)) -> UpdateError {
    let r = rig();
    setup(&r.server);
    assert!(
        matches!(r.check(), Err(JobError::Failed(_))),
        "a failed check fails its job"
    );
    match r.phase() {
        UpdatePhase::Failed { error, info: None } => error,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_check_that_fails_says_why_with_a_next_step() {
    assert_eq!(failed_check(|_| {}), UpdateError::NoRelease, "404");
    assert!(
        matches!(failed_check(|s| s.route(MANIFEST_URL, Route::Body(500, vec![]))), UpdateError::Unreachable { detail } if detail.contains("HTTP 500"))
    );
    let offline = TransportError::new(
        TransportErrorKind::Connect,
        "failed to lookup address information",
    );
    assert_eq!(
        failed_check(|s| s.route(MANIFEST_URL, Route::Fail(offline))),
        UpdateError::Offline
    );
    assert!(matches!(
        failed_check(|s| s.json(MANIFEST_URL, "{")),
        UpdateError::InvalidManifest { .. }
    ));
    assert!(matches!(
        failed_check(|s| s.route(MANIFEST_URL, Route::Body(200, vec![0xff, 0xfe]))),
        UpdateError::InvalidManifest { detail } if detail.contains("UTF-8")
    ));
    let elsewhere = r#"{"version":"9.9.9","platforms":{"plan9-mips":{"url":"https://updates.test/x","signature":"dW50cnVzdGVkIGNvbW1lbnQ6IHg="}}}"#;
    assert!(matches!(
        failed_check(|s| s.json(MANIFEST_URL, elsewhere)),
        UpdateError::NoPlatform { .. }
    ));
}

#[test]
fn a_check_can_be_cancelled_without_waiting_for_the_server() {
    let r = rig();
    r.server.route(MANIFEST_URL, Route::Hang);
    let (_, ticket) = r.updater.begin_check().unwrap();
    let ctx = job(&r.reg);
    r.updater.set_job(ticket, ctx.id().0);
    // Cancel from the UI: the updater names the job, the adapter cancels it.
    assert_eq!(r.updater.cancel().unwrap(), CancelOutcome::Job(ctx.id().0));
    r.reg.cancel(ctx.id());
    // The request never answers until released: returning at all proves it was not waited for.
    assert!(matches!(
        ctx.run(|c| r.updater.run_check(c, ticket)),
        Err(JobError::Cancelled(_))
    ));
    r.server.release();
    assert_eq!(r.phase(), UpdatePhase::Idle);
    // A check whose job is not registered yet is cancelled at once.
    r.updater.begin_check().unwrap();
    assert_eq!(r.updater.cancel().unwrap(), CancelOutcome::Done);
    let s = r.updater.status();
    assert!(s.phase == UpdatePhase::Idle);
}

#[test]
fn a_stale_result_changes_nothing() {
    let r = rig();
    r.publish("9.9.9", "update.bin.sig");
    let (_, old) = r.updater.begin_check().unwrap();
    r.updater.cancel().unwrap();
    let (_, current) = r.updater.begin_check().unwrap();
    let ctx = job(&r.reg);
    r.updater.set_job(old, 99);
    assert_eq!(
        r.phase(),
        UpdatePhase::Checking { job: None },
        "the old ticket's job is not shown"
    );
    let _ = ctx.run(|c| r.updater.run_check(c, old));
    assert_eq!(
        r.phase(),
        UpdatePhase::Checking { job: None },
        "the old check's result is dropped"
    );
    assert!(r.records.load().last_checked.is_none());
    let ctx = job(&r.reg);
    let _ = ctx.run(|c| r.updater.run_check(c, current));
    assert!(matches!(r.phase(), UpdatePhase::Available { .. }));
}

// ---- download and signature ----

#[test]
fn a_download_runs_through_paddock_download_and_the_signature_check() {
    let r = rig();
    r.publish("9.9.9", "update.bin.sig");
    r.check().unwrap();
    r.download().unwrap();
    let UpdatePhase::Ready {
        info,
        install_on_quit: false,
    } = r.phase()
    else {
        panic!("{:?}", r.phase())
    };
    assert_eq!(info.version, "9.9.9");
    assert_eq!(
        r.sink.phases(),
        [
            CHECK_PHASE,
            "connect",
            "download",
            "verify",
            SIGNATURE_PHASE
        ]
    );
    let files = r.downloads();
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].file_name().unwrap(),
        "update-9.9.9-App_9.9.9.tar.gz"
    );
    assert_eq!(std::fs::read(&files[0]).unwrap(), package());
    // Checking again now is refused; dropping the verified download deletes it and keeps the update available.
    assert!(matches!(
        r.updater.begin_check(),
        Err(UpdateError::Busy { .. })
    ));
    assert_eq!(r.updater.cancel().unwrap(), CancelOutcome::Done);
    let s = r.updater.status();
    assert!(matches!(s.phase, UpdatePhase::Available { .. }));
    assert!(r.downloads().is_empty());
}

fn refused(sig: &str) -> String {
    let r = rig();
    r.publish("9.9.9", sig);
    r.check().unwrap();
    assert!(matches!(r.download(), Err(JobError::Failed(_))));
    assert!(r.downloads().is_empty(), "a refused package is deleted");
    match r.phase() {
        UpdatePhase::Failed {
            error: UpdateError::SignatureInvalid { detail },
            info: Some(_),
        } => {
            assert!(
                matches!(r.updater.begin_download(), Err(UpdateError::Busy { .. })),
                "check again first"
            );
            assert!(r.updater.begin_check().is_ok());
            detail
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_bad_signature_is_refused_and_the_package_deleted() {
    assert_eq!(
        refused("other-key.sig"),
        "the update was signed with another key than this app's"
    );
    assert!(
        refused("other-version.sig").contains("made for version 9.9.8"),
        "an older signed package under a newer number"
    );
    assert!(refused("no-version.sig").contains("does not name the version"));
    // The bytes changed on the way.
    let r = rig();
    r.publish("9.9.9", "update.bin.sig");
    let mut tampered = package();
    tampered[0] ^= 0xff;
    r.server.route(PACKAGE_URL, Route::Body(200, tampered));
    r.check().unwrap();
    assert!(r.download().is_err());
    assert!(matches!(
        r.phase(),
        UpdatePhase::Failed {
            error: UpdateError::SignatureInvalid { .. },
            ..
        }
    ));
}

#[test]
fn a_failed_download_can_be_tried_again() {
    let r = rig();
    r.publish("9.9.9", "update.bin.sig");
    r.server.route(PACKAGE_URL, Route::Body(404, vec![]));
    r.check().unwrap();
    assert!(r.download().is_err());
    assert!(matches!(
        r.phase(),
        UpdatePhase::Failed {
            error: UpdateError::Unreachable { .. },
            info: Some(_)
        }
    ));
    r.server.route(PACKAGE_URL, Route::Body(200, package()));
    r.download().unwrap();
    assert!(matches!(r.phase(), UpdatePhase::Ready { .. }));
}

#[test]
fn an_update_over_the_size_limit_is_refused_before_downloading() {
    let dir = tempfile::tempdir().unwrap();
    let server = FakeServer::new();
    let mut c = config(dir.path(), Some(key()), "1.0.0");
    c.max_download_bytes = 1000;
    let u = Updater::new(
        c,
        downloader(&server),
        Arc::new(RecordStore::new(None)),
        Box::new(|_| {}),
    );
    server.json(
        MANIFEST_URL,
        &manifest("9.9.9", &signature("update.bin.sig"), true),
    );
    let (reg, _) = jobs();
    let (_, t) = u.begin_check().unwrap();
    job(&reg).run(|c| u.run_check(c, t)).unwrap();
    let (status, ticket) = u.begin_download().unwrap();
    assert_eq!(ticket, None, "nothing to run");
    assert!(matches!(
        status.phase,
        UpdatePhase::Failed {
            error: UpdateError::TooLarge { limit: 1000 },
            ..
        }
    ));
    assert_eq!(server.hits(PACKAGE_URL), 0);
    // Without a size in the release information the downloader stops at the limit.
    server.json(
        MANIFEST_URL,
        &manifest("9.9.9", &signature("update.bin.sig"), false),
    );
    server.route(PACKAGE_URL, Route::Body(200, package()));
    let (_, t) = u.begin_check().unwrap();
    job(&reg).run(|c| u.run_check(c, t)).unwrap();
    let (_, t) = u.begin_download().unwrap();
    assert!(job(&reg).run(|c| u.run_download(c, t.unwrap())).is_err());
    assert!(matches!(
        u.status().phase,
        UpdatePhase::Failed {
            error: UpdateError::TooLarge { limit: 1000 },
            ..
        }
    ));
}

#[test]
fn a_download_can_be_cancelled_in_every_phase_and_the_update_stays_available() {
    // Before it started (no job yet): at once.
    let r = rig();
    r.publish("9.9.9", "update.bin.sig");
    r.check().unwrap();
    r.updater.begin_download().unwrap();
    assert_eq!(r.updater.cancel().unwrap(), CancelOutcome::Done);
    let s = r.updater.status();
    assert!(matches!(s.phase, UpdatePhase::Available { .. }));
    // While downloading: through its job.
    let (_, t) = r.updater.begin_download().unwrap();
    let ctx = job(&r.reg);
    r.updater.set_job(t.unwrap(), ctx.id().0);
    assert_eq!(r.updater.cancel().unwrap(), CancelOutcome::Job(ctx.id().0));
    r.reg.cancel(ctx.id());
    assert!(matches!(
        ctx.run(|c| r.updater.run_download(c, t.unwrap())),
        Err(JobError::Cancelled(_))
    ));
    assert!(matches!(r.phase(), UpdatePhase::Available { .. }));
    // During the signature check (the sink cancels when that phase begins): the file is deleted.
    let (_, t) = r.updater.begin_download().unwrap();
    let ctx = job(&r.reg);
    *r.sink.cancel_at.lock().unwrap() = Some((SIGNATURE_PHASE.into(), ctx.cancel_token().clone()));
    assert!(matches!(
        ctx.run(|c| r.updater.run_download(c, t.unwrap())),
        Err(JobError::Cancelled(_))
    ));
    assert!(matches!(r.phase(), UpdatePhase::Available { .. }));
    assert!(r.downloads().is_empty());
    assert!(matches!(
        ctx.reporter().snapshot().state,
        JobState::Cancelled { .. }
    ));
}

#[test]
fn work_without_a_found_update_or_out_of_turn_is_refused() {
    let r = rig();
    assert!(matches!(
        r.updater.begin_download(),
        Err(UpdateError::Busy { .. })
    ));
    assert!(matches!(r.updater.cancel(), Err(UpdateError::Busy { .. })));
    assert!(matches!(
        r.updater.install_on_quit(true),
        Err(UpdateError::Busy { .. })
    ));
    // A job body run without its update (a wiring bug) fails without changing the phase.
    let (_, ticket) = r.updater.begin_check().unwrap();
    assert!(matches!(
        job(&r.reg).run(|c| r.updater.run_download(c, ticket)),
        Err(JobError::Failed(_))
    ));
    assert!(matches!(r.phase(), UpdatePhase::Checking { .. }));
}

// ---- preferences ----

#[test]
fn preferences_are_validated_kept_and_a_channel_change_forgets_the_update() {
    let r = rig();
    r.publish("9.9.9", "update.bin.sig");
    r.check().unwrap();
    let bad = r
        .updater
        .set_preferences(Preferences {
            auto_check: true,
            channel: "nightly".into(),
        })
        .unwrap_err();
    assert_eq!(
        bad,
        UpdateError::Invalid {
            detail: "There is no release channel 'nightly'.".into()
        }
    );
    let s = r
        .updater
        .set_preferences(Preferences {
            auto_check: true,
            channel: "stable".into(),
        })
        .unwrap();
    assert!(s.auto_check);
    assert!(
        matches!(s.phase, UpdatePhase::Available { .. }),
        "the same channel keeps the update"
    );
    let s = r
        .updater
        .set_preferences(Preferences {
            auto_check: true,
            channel: "beta".into(),
        })
        .unwrap();
    assert_eq!((s.phase, s.channel.as_str()), (UpdatePhase::Idle, "beta"));
    // The same again announces nothing new and keeps the state.
    let again = r
        .updater
        .set_preferences(Preferences {
            auto_check: true,
            channel: "beta".into(),
        })
        .unwrap();
    assert_eq!(again.channel, "beta");
    // Without a store they live for the run: the next updater starts with the defaults.
    let r2 = rig_with("1.0.0", Some(key()), r.dir, |u| u);
    let s = r2.updater.status();
    assert_eq!((s.auto_check, s.channel.as_str()), (false, "stable"));
    // Busy while a download runs.
    let r = rig();
    r.publish("9.9.9", "update.bin.sig");
    r.check().unwrap();
    r.updater.begin_download().unwrap();
    assert!(matches!(
        r.updater.set_preferences(Preferences {
            auto_check: false,
            channel: "beta".into()
        }),
        Err(UpdateError::Busy { .. })
    ));
    // Without updates the preferences are still kept.
    let r = rig_with("1.0.0", None, tempfile::tempdir().unwrap(), |u| u);
    assert!(
        r.updater
            .set_preferences(Preferences {
                auto_check: true,
                channel: "beta".into()
            })
            .is_ok()
    );
}

/// A store that keeps what it was given and applies it when told (as the settings section does).
#[derive(Default)]
struct Kept {
    saved: Mutex<Vec<Preferences>>,
    refuse: std::sync::atomic::AtomicBool,
}

impl PreferenceStore for Kept {
    fn save(&self, prefs: &Preferences) -> Result<(), UpdateError> {
        if self.refuse.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(UpdateError::Invalid {
                detail: "refused by the store".into(),
            });
        }
        self.saved.lock().unwrap().push(prefs.clone());
        Ok(())
    }
}

#[test]
fn with_a_store_the_preferences_go_through_it_and_take_effect_when_it_applies_them() {
    let r = rig();
    let kept = Arc::new(Kept::default());
    assert!(r.updater.set_preference_store(kept.clone()));
    assert!(
        !r.updater.set_preference_store(Arc::new(Kept::default())),
        "the first store stays"
    );
    let beta = Preferences {
        auto_check: true,
        channel: "beta".into(),
    };
    let s = r.updater.set_preferences(beta.clone()).unwrap();
    assert_eq!(
        (s.auto_check, s.channel.as_str()),
        (false, "stable"),
        "not yet applied"
    );
    assert_eq!(*kept.saved.lock().unwrap(), vec![beta.clone()]);
    let s = r.updater.apply_preferences(beta).unwrap();
    assert_eq!((s.auto_check, s.channel.as_str()), (true, "beta"));
    // An unknown channel never reaches the store; a refusing store refuses the change.
    assert!(matches!(
        r.updater.set_preferences(Preferences {
            auto_check: true,
            channel: "nightly".into()
        }),
        Err(UpdateError::Invalid { .. })
    ));
    kept.refuse.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(
        r.updater
            .set_preferences(Preferences {
                auto_check: false,
                channel: "stable".into()
            })
            .is_err()
    );
    assert_eq!(kept.saved.lock().unwrap().len(), 1);
    // Busy while a download runs: a channel change is refused before the store hears of it.
    kept.refuse
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let r = rig();
    let kept = Arc::new(Kept::default());
    r.updater.set_preference_store(kept.clone());
    r.publish("9.9.9", "update.bin.sig");
    r.check().unwrap();
    r.updater.begin_download().unwrap();
    assert!(matches!(
        r.updater.set_preferences(Preferences {
            auto_check: false,
            channel: "beta".into()
        }),
        Err(UpdateError::Busy { .. })
    ));
    assert!(kept.saved.lock().unwrap().is_empty());
    // The same channel is fine while busy.
    assert!(
        r.updater
            .set_preferences(Preferences {
                auto_check: true,
                channel: "stable".into()
            })
            .is_ok()
    );
}

#[test]
fn the_automatic_check_follows_the_preference_and_the_clock() {
    let now = Arc::new(Mutex::new(1_000_000u64));
    let clock = now.clone();
    let r = rig_with(
        "1.0.0",
        Some(key()),
        tempfile::tempdir().unwrap(),
        move |u| u.with_clock(move || *clock.lock().unwrap()),
    );
    assert!(
        !r.updater.auto_check_due(),
        "off by default (and the app just started)"
    );
    r.updater
        .set_preferences(Preferences {
            auto_check: true,
            channel: "stable".into(),
        })
        .unwrap();
    assert!(!r.updater.auto_check_due(), "not in the first minute");
    r.publish("1.0.0", "update.bin.sig");
    r.check().unwrap();
    assert_eq!(r.updater.status().last_checked, Some(1_000_000));
}

// ---- install ----

fn ready_rig(installer: Option<Arc<FakeInstaller>>, hooks: Option<Arc<dyn ServiceHooks>>) -> Rig {
    let r = rig_with(
        "1.0.0",
        Some(key()),
        tempfile::tempdir().unwrap(),
        |mut u| {
            if let Some(i) = installer {
                u = u.with_installer(i);
            }
            if let Some(h) = hooks {
                u = u.with_service_hooks(h);
            }
            u
        },
    );
    r.publish("9.9.9", "update.bin.sig");
    r.check().unwrap();
    r.download().unwrap();
    r
}

#[test]
fn restart_to_update_installs_at_exit_after_stopping_the_services() {
    let installer = Arc::new(FakeInstaller::default());
    let hooks = Arc::new(FakeHooks::default());
    let r = ready_rig(
        Some(installer.clone()),
        Some(hooks.clone() as Arc<dyn ServiceHooks>),
    );
    assert!(matches!(
        r.updater.prepare_restart().unwrap().phase,
        UpdatePhase::Installing { .. }
    ));
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Installed {
            relaunch: !cfg!(windows)
        }
    );
    let calls = installer.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!((calls[0].1.as_str(), calls[0].2), ("9.9.9", true));
    assert_eq!(hooks.seen.lock().unwrap().as_slice(), ["stop"]);
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Nothing,
        "only once"
    );
}

#[test]
fn install_on_quit_installs_without_a_relaunch_and_can_be_switched_off() {
    let installer = Arc::new(FakeInstaller::default());
    let r = ready_rig(Some(installer.clone()), None);
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Nothing,
        "nothing asked"
    );
    let s = r.updater.install_on_quit(true).unwrap();
    assert!(matches!(
        s.phase,
        UpdatePhase::Ready {
            install_on_quit: true,
            ..
        }
    ));
    r.updater.install_on_quit(false).unwrap();
    assert_eq!(r.updater.install_at_exit(), ExitAction::Nothing);
    r.updater.install_on_quit(true).unwrap();
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Installed { relaunch: false }
    );
    assert!(!installer.calls.lock().unwrap()[0].2, "no relaunch");
    assert!(matches!(r.phase(), UpdatePhase::Installing { .. }));
}

#[test]
fn a_staged_file_altered_after_download_is_not_installed_and_is_fetched_again() {
    let installer = Arc::new(FakeInstaller::default());
    let hooks = Arc::new(FakeHooks::default());
    let r = ready_rig(
        Some(installer.clone()),
        Some(hooks.clone() as Arc<dyn ServiceHooks>),
    );
    let staged: Vec<_> = std::fs::read_dir(r.dir.path().join("downloads"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .collect();
    assert_eq!(staged.len(), 1, "one staged package");
    let mut bytes = std::fs::read(&staged[0]).unwrap();
    bytes.push(0);
    std::fs::write(&staged[0], bytes).unwrap();
    r.updater.prepare_restart().unwrap();
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Failed { relaunch: true }
    );
    assert!(
        installer.calls.lock().unwrap().is_empty(),
        "nothing altered is installed"
    );
    assert!(
        hooks.seen.lock().unwrap().is_empty(),
        "the services are left running"
    );
    assert!(!staged[0].exists(), "the altered file is deleted");
    assert!(matches!(
        r.phase(),
        UpdatePhase::Failed {
            error: UpdateError::SignatureInvalid { .. },
            ..
        }
    ));
    // The next check offers the update again and the download fetches a fresh, valid copy.
    r.check().unwrap();
    r.download().unwrap();
    assert!(staged[0].is_file());
    r.updater.prepare_restart().unwrap();
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Installed {
            relaunch: !cfg!(windows)
        }
    );
    assert_eq!(installer.calls.lock().unwrap().len(), 1);
}

#[test]
fn a_failed_install_is_recorded_and_shown_at_the_next_start() {
    let installer = Arc::new(FakeInstaller {
        fail: Some(UpdateError::InstallFailed {
            detail: "disk on fire".into(),
        }),
        ..Default::default()
    });
    let r = ready_rig(Some(installer), None);
    r.updater.prepare_restart().unwrap();
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Failed { relaunch: true }
    );
    assert!(matches!(
        r.phase(),
        UpdatePhase::Failed {
            error: UpdateError::InstallFailed { .. },
            ..
        }
    ));
    let next = rig_with("1.0.0", Some(key()), r.dir, |u| u);
    match next.phase() {
        UpdatePhase::Failed {
            error: UpdateError::InstallFailed { detail },
            info: None,
        } => assert_eq!(detail, "version 9.9.9: disk on fire"),
        other => panic!("{other:?}"),
    }
    assert!(next.records.load().failed_install.is_none(), "shown once");
}

#[test]
fn services_that_cannot_be_stopped_stop_the_install() {
    let installer = Arc::new(FakeInstaller::default());
    let hooks = Arc::new(FakeHooks {
        fail_stop: true,
        ..Default::default()
    });
    let r = ready_rig(
        Some(installer.clone()),
        Some(hooks as Arc<dyn ServiceHooks>),
    );
    r.updater.install_on_quit(true).unwrap();
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Failed { relaunch: false }
    );
    assert!(installer.calls.lock().unwrap().is_empty());
    assert!(
        matches!(r.phase(), UpdatePhase::Failed { error: UpdateError::InstallFailed { detail }, .. } if detail.contains("launchctl said no"))
    );
}

#[test]
fn without_an_installer_restart_is_refused_and_install_on_quit_fails_at_exit() {
    let r = ready_rig(None, None);
    assert!(matches!(
        r.updater.prepare_restart(),
        Err(UpdateError::InstallFailed { .. })
    ));
    r.updater.install_on_quit(true).unwrap();
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Failed { relaunch: false }
    );
    // A verified download deleted meanwhile is reported, not installed.
    let installer = Arc::new(FakeInstaller::default());
    let r = ready_rig(Some(installer.clone()), None);
    for f in r.downloads() {
        std::fs::remove_file(f).unwrap();
    }
    r.updater.install_on_quit(true).unwrap();
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Failed { relaunch: false }
    );
    assert!(installer.calls.lock().unwrap().is_empty());
    assert!(
        matches!(r.phase(), UpdatePhase::Failed { error: UpdateError::InstallFailed { detail }, .. } if detail.contains("no longer there"))
    );
}

// ---- services across updates (rule 24) ----

#[test]
fn services_are_registered_once_per_version_change_and_a_failure_is_retried() {
    let dir = tempfile::tempdir().unwrap();
    let hooks = Arc::new(FakeHooks::default());
    *hooks.fail_register.lock().unwrap() = true;
    let start = |version: &str, dir: tempfile::TempDir| {
        let h = hooks.clone();
        let r = rig_with(version, Some(key()), dir, move |u| u.with_service_hooks(h));
        (r.updater.on_start(), r.dir)
    };
    let (out, dir) = start("1.0.0", dir);
    assert_eq!(out, ServicesOutcome::Failed("bootout timed out".into()));
    *hooks.fail_register.lock().unwrap() = false;
    let (out, dir) = start("1.0.0", dir);
    assert_eq!(
        out,
        ServicesOutcome::Registered { previous: None },
        "retried at the next start"
    );
    let (out, dir) = start("1.0.0", dir);
    assert_eq!(out, ServicesOutcome::Unchanged);
    let (out, _) = start("1.1.0", dir);
    assert_eq!(
        out,
        ServicesOutcome::Registered {
            previous: Some("1.0.0".into())
        }
    );
    assert_eq!(
        hooks.seen.lock().unwrap().as_slice(),
        [
            "register none -> 1.0.0",
            "register none -> 1.0.0",
            "register 1.0.0 -> 1.1.0"
        ]
    );
    assert_eq!(
        rig().updater.on_start(),
        ServicesOutcome::Unchanged,
        "no hooks"
    );
}

/// Services that only need registering: stopping them before an install is the default (nothing).
struct RegisterOnly;

impl ServiceHooks for RegisterOnly {
    fn on_version_change(&self, _previous: Option<&str>, _current: &str) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn services_without_a_stop_step_do_not_hold_up_the_install() {
    let installer = Arc::new(FakeInstaller::default());
    let r = ready_rig(Some(installer.clone()), Some(Arc::new(RegisterOnly)));
    r.updater.install_on_quit(true).unwrap();
    assert_eq!(
        r.updater.install_at_exit(),
        ExitAction::Installed { relaunch: false }
    );
    assert_eq!(installer.calls.lock().unwrap().len(), 1);
}

#[test]
fn work_whose_job_could_not_start_is_abandoned() {
    let r = rig();
    r.publish("9.9.9", "update.bin.sig");
    let (_, ticket) = r.updater.begin_check().unwrap();
    r.updater.abandon(ticket);
    assert_eq!(r.phase(), UpdatePhase::Idle);
    r.check().unwrap();
    let (_, ticket) = r.updater.begin_download().unwrap();
    r.updater.abandon(ticket.unwrap());
    assert!(matches!(r.phase(), UpdatePhase::Available { .. }));
    r.updater.abandon(ticket.unwrap());
    assert!(
        matches!(r.phase(), UpdatePhase::Available { .. }),
        "a stale ticket changes nothing"
    );
}

#[test]
fn a_transport_that_panics_is_a_failed_check_not_a_crash() {
    let r = rig();
    r.server.route(MANIFEST_URL, Route::Panic);
    assert!(r.check().is_err());
    assert!(
        matches!(r.phase(), UpdatePhase::Failed { error: UpdateError::Unreachable { detail }, .. } if detail.contains("without an answer"))
    );
}

#[test]
fn a_job_body_on_an_updater_without_updates_fails_quietly() {
    let a = rig();
    let (_, ticket) = a.updater.begin_check().unwrap();
    let (reg, _) = jobs();
    // No channel: nothing to ask.
    let dir = tempfile::tempdir().unwrap();
    let none = Updater::new(
        UpdaterConfig::new("1.0.0", Some(key()), vec![], dir.path()),
        downloader(&a.server),
        a.records.clone(),
        Box::new(|_| {}),
    );
    assert!(
        matches!(job(&reg).run(|c| none.run_check(c, ticket)), Err(JobError::Failed(m)) if m.contains("not available"))
    );
    // A running version that is not a semantic version cannot be compared.
    a.publish("9.9.9", "update.bin.sig");
    let odd = Updater::new(
        config(dir.path(), Some(key()), "dev"),
        downloader(&a.server),
        a.records.clone(),
        Box::new(|_| {}),
    );
    assert!(job(&reg).run(|c| odd.run_check(c, ticket)).is_err());
    assert_eq!(
        (none.status().phase, odd.status().phase),
        (UpdatePhase::Unavailable, UpdatePhase::Unavailable)
    );
}

#[test]
fn the_automatic_check_is_due_after_the_start_up_delay_and_a_day() {
    let now = Arc::new(Mutex::new(1_000_000u64));
    let clock = now.clone();
    let dir = tempfile::tempdir().unwrap();
    let server = FakeServer::new();
    let mut c = config(dir.path(), Some(key()), "1.0.0");
    c.auto_check_delay_secs = 0;
    let u = Updater::new(
        c,
        downloader(&server),
        Arc::new(RecordStore::new(None)),
        Box::new(|_| {}),
    )
    .with_clock(move || *clock.lock().unwrap());
    assert!(!u.auto_check_due(), "off by default");
    u.set_preferences(Preferences {
        auto_check: true,
        channel: "stable".into(),
    })
    .unwrap();
    assert!(u.auto_check_due());
    server.json(
        MANIFEST_URL,
        &manifest("1.0.0", &signature("update.bin.sig"), true),
    );
    let (reg, _) = jobs();
    let (_, t) = u.begin_check().unwrap();
    job(&reg).run(|c| u.run_check(c, t)).unwrap();
    assert!(!u.auto_check_due(), "checked just now");
    *now.lock().unwrap() += state::AUTO_CHECK_INTERVAL_SECS;
    assert!(u.auto_check_due(), "a day later");
}
