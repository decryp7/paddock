//! The supervisor end to end with a fake runner (`examples/fake_runner.rs`): real processes, pipes, logs and
//! graceful stops; a controllable clock so backoff and the job grace period take no real time.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods, missing_docs)]

use pitcrew_core::health::RunnerStatus;
use pitcrew_core::history::HistoryEvent;
use pitcrew_core::history::HistoryKind;
use pitcrew_core::names::RunnerName;
use pitcrew_core::protocol::{Request, Response, RunnerSnapshot, Secret};
use pitcrew_core::time::Timestamp;
use pitcrew_core::update::{LocalMoment, TimeOfDay, Weekday};
use pitcrew_service::launcher::OsLauncher;
use pitcrew_service::supervisor::{Env, Msg, Supervisor};
use pitcrew_service::work::Sources;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

const EXE: &str = std::env::consts::EXE_SUFFIX;

fn fake_runner() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| {
            let deps = std::env::current_exe().unwrap();
            let path = deps
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("examples")
                .join(format!("fake_runner{EXE}"));
            // `cargo test` builds examples, `cargo test --test supervision` does not.
            let status = std::process::Command::new(env!("CARGO"))
                .args(["build", "--quiet", "-p", "pitcrew-service", "--example", "fake_runner"])
                .status()
                .unwrap();
            assert!(status.success() && path.exists(), "the fake runner could not be built");
            path
        })
        .clone()
}

struct Harness {
    sup: Supervisor,
    rx: Receiver<Msg>,
    clock: Arc<AtomicU64>,
    root: PathBuf,
    runner_dir: PathBuf,
    name: RunnerName,
}

impl Harness {
    fn new(test: &str, fake: &str, registered: bool) -> Harness {
        Harness::with_runner_toml(test, fake, registered, "")
    }

    /// `runner_toml` is appended to the runner's table (e.g. an `[runners.env]` table).
    fn with_runner_toml(test: &str, fake: &str, registered: bool, runner_toml: &str) -> Harness {
        pitcrew_service::launch::set_stop_helper(PathBuf::from(env!("CARGO_BIN_EXE_pitcrew-service")));
        let root = std::env::temp_dir().join(format!("pitcrew-it-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let data = root.join("data");
        let runner_dir = root.join("runner");
        std::fs::create_dir_all(runner_dir.join("bin")).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::copy(fake_runner(), runner_dir.join("bin").join(format!("gitea-runner{EXE}")))
            .expect("build the example: cargo test builds it");
        std::fs::write(runner_dir.join("fake.txt"), fake).unwrap();
        std::fs::write(runner_dir.join("config.yaml"), "log:\n  level: info\n").unwrap();
        if registered {
            std::fs::write(runner_dir.join(".runner"), "{}").unwrap();
        }
        let toml = format!(
            "schema_version = 1\n[defaults.update]\nmode = \"manual\"\n[defaults.cleanup]\nautomatic = false\n[[runners]]\nname = \"{test}\"\nkind = \"gitea-runner\"\ndir = '{}'\nsession = \"service\"\nlabels = [\"x\"]\n{runner_toml}",
            runner_dir.display()
        );
        std::fs::write(data.join("pitcrew.toml"), toml).unwrap();
        let clock = Arc::new(AtomicU64::new(pitcrew_service::cleanup::now().0));
        let c = clock.clone();
        let env = Env {
            data_dir: data,
            launcher: Box::new(OsLauncher),
            sources: Sources::public(),
            platform: pitcrew_core::platform::Platform::current().unwrap(),
            base_path: Arc::new(|| Some("/pitcrew/test-path:/usr/bin".to_string())),
            clock: Arc::new(move || Timestamp(c.load(Ordering::SeqCst))),
            local: Arc::new(|_| {
                (
                    LocalMoment {
                        weekday: Weekday::Mon,
                        time: TimeOfDay::new(12, 0).unwrap(),
                    },
                    0,
                )
            }),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let sup = Supervisor::new(env, tx);
        Harness {
            sup,
            rx,
            clock,
            root,
            runner_dir,
            name: RunnerName::parse(test).unwrap(),
        }
    }

    fn now(&self) -> Timestamp {
        Timestamp(self.clock.load(Ordering::SeqCst))
    }

    /// Advances the fake clock by `step` and processes everything that arrived, a few times.
    fn pump(&mut self, step: Duration) {
        self.clock.fetch_add(step.as_millis() as u64, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(30));
        while let Ok(m) = self.rx.try_recv() {
            self.sup.handle(m);
        }
        self.sup.tick(self.now());
    }

    fn until(&mut self, what: &str, step: Duration, mut f: impl FnMut(&mut Harness) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(40);
        while !f(self) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for: {what}; status {:?}",
                self.status()
            );
            self.pump(step);
        }
    }

    fn ask(&mut self, req: Request) -> Response {
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        self.sup.handle(Msg::Request(Box::new(req), tx));
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(r) = rx.try_recv() {
                return r;
            }
            assert!(Instant::now() < deadline, "no answer");
            self.pump(Duration::from_millis(100));
        }
    }

    fn status(&mut self) -> RunnerSnapshot {
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        self.sup.handle(Msg::Request(Box::new(Request::Status), tx));
        match rx.try_recv().unwrap() {
            Response::Status { runners, .. } => runners.into_iter().next().unwrap(),
            other => panic!("{other:?}"),
        }
    }

    fn history(&mut self) -> Vec<HistoryEvent> {
        match self.ask(Request::History {
            runner: self.name.clone(),
            days: 1,
            utc_offset_minutes: 0,
        }) {
            Response::History { events, .. } => events,
            other => panic!("{other:?}"),
        }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.runner_dir.join("logs").join("runner.log")).unwrap_or_default()
    }

    fn finish(mut self) {
        let _ = self.ask(Request::Stop { runner: self.name.clone() });
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.status().running_since.is_some() && Instant::now() < deadline {
            self.pump(Duration::from_secs(30));
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn count(h: &[HistoryEvent], f: impl Fn(&HistoryKind) -> bool) -> usize {
    h.iter().filter(|e| f(&e.kind)).count()
}

#[test]
fn a_runner_that_exits_is_restarted_after_the_backoff() {
    let mut h = Harness::new("crashy", "mode=exit\ncode=3\n", true);
    h.until("first exit", Duration::from_millis(100), |h| {
        let hist = h.history();
        count(&hist, |k| {
            matches!(
                k,
                HistoryKind::Exited {
                    code: Some(3),
                    expected: false
                }
            )
        }) == 1
    });
    let s = h.status();
    assert!(s.next_start_at.is_some(), "a restart is scheduled: {s:?}");
    assert_eq!(s.restarts_24h, 1);
    assert!(h.log().contains("supervisor: the runner exited with code 3"));
    assert!(h.log().contains("declare successfully"), "the runner's own output is in the log");
    // Ten seconds of backoff pass in one step.
    h.pump(Duration::from_secs(11));
    h.until("second start", Duration::from_millis(100), |h| {
        count(&h.history(), |k| *k == HistoryKind::Started) >= 2
    });
    h.finish();
}

#[test]
fn runners_start_with_the_login_path_and_their_own_env() {
    let mut h = Harness::with_runner_toml("envy", "mode=ok\n", true, "[runners.env]\nCI_FLAG = \"yes\"\n");
    let dump = h.runner_dir.join("env.txt");
    h.until("the runner wrote its environment", Duration::from_millis(100), |_| dump.exists());
    let text = std::fs::read_to_string(&dump).unwrap();
    assert!(text.contains("PATH=/pitcrew/test-path:/usr/bin\n"), "{text}");
    assert!(text.contains("CI_FLAG=yes\n"), "{text}");
    h.finish();
    // A runner's own PATH replaces the login one.
    let mut h = Harness::with_runner_toml("envy2", "mode=ok\n", true, "[runners.env]\nPATH = \"/only/this\"\n");
    let dump = h.runner_dir.join("env.txt");
    h.until("the runner wrote its environment", Duration::from_millis(100), |_| dump.exists());
    assert!(std::fs::read_to_string(&dump).unwrap().contains("PATH=/only/this\n"));
    h.finish();
}

#[test]
fn a_stalled_runner_is_stopped_gracefully_and_restarted() {
    let mut h = Harness::new("stally", "mode=stall\n", true);
    h.until("stall", Duration::from_millis(200), |h| {
        count(&h.history(), |k| matches!(k, HistoryKind::Stalled { .. })) == 1
    });
    h.until("graceful exit", Duration::from_millis(200), |h| {
        count(&h.history(), |k| matches!(k, HistoryKind::Exited { expected: true, .. })) == 1
    });
    assert!(
        h.log().contains("shutdown initiated"),
        "the runner shut down by itself after the stop request"
    );
    assert!(h.log().contains("supervisor: runner stalled"));
    h.pump(Duration::from_secs(11));
    h.until("restart", Duration::from_millis(200), |h| {
        count(&h.history(), |k| *k == HistoryKind::Started) >= 2
    });
    h.finish();
}

#[test]
fn stop_waits_for_the_running_job() {
    let mut h = Harness::new("busy", "mode=job\njob_secs=4\n", true);
    h.until("job seen", Duration::from_secs(6), |h| h.status().current_job.is_some());
    let s = h.status();
    assert_eq!(s.status, RunnerStatus::Busy);
    assert_eq!(s.current_job.unwrap().info.repository.as_deref(), Some("owner/app"));
    assert_eq!(h.ask(Request::Stop { runner: h.name.clone() }), Response::Ok);
    assert_eq!(h.status().status, RunnerStatus::Draining, "it finishes the job first");
    // While the job's process lives, nothing is stopped however much time passes.
    for _ in 0..5 {
        h.pump(Duration::from_secs(60));
        assert!(h.status().running_since.is_some());
        if h.status().status != RunnerStatus::Draining {
            break;
        }
    }
    h.until("stopped after the job", Duration::from_secs(6), |h| {
        h.status().status == RunnerStatus::Stopped
    });
    let hist = h.history();
    assert_eq!(count(&hist, |k| matches!(k, HistoryKind::JobFinished { .. })), 1);
    assert_eq!(count(&hist, |k| matches!(k, HistoryKind::Exited { expected: true, .. })), 1);
    let _ = std::fs::remove_dir_all(&h.root);
}

#[test]
fn an_unregistered_runner_is_registered_from_the_app() {
    let mut h = Harness::new("fresh", "mode=ok\n", false);
    h.pump(Duration::from_secs(1));
    assert_eq!(h.status().status, RunnerStatus::NotRegistered);
    let bad = h.ask(Request::Register {
        runner: h.name.clone(),
        server_url: "https://git.example.com".into(),
        token: Secret::new("bad").unwrap(),
        display_name: None,
    });
    let Response::Error(e) = bad else { panic!("{bad:?}") };
    assert!(e.message.contains("refused to register"), "{}", e.message);
    assert!(!e.message.contains(" bad "), "the token never appears in messages: {}", e.message);
    let ok = h.ask(Request::Register {
        runner: h.name.clone(),
        server_url: "https://git.example.com/".into(),
        token: Secret::new("good-token").unwrap(),
        display_name: Some("Fresh runner".into()),
    });
    assert_eq!(ok, Response::Ok);
    assert!(h.runner_dir.join(".runner").exists());
    let s = h.status();
    assert_eq!(s.server_url.as_deref(), Some("https://git.example.com"));
    h.until("running", Duration::from_millis(200), |h| h.status().status == RunnerStatus::Idle);
    let saved = std::fs::read_to_string(h.root.join("data").join("pitcrew.toml")).unwrap();
    assert!(!saved.contains("good-token"), "the token is never stored");
    h.finish();
}

#[test]
fn registering_a_folder_that_is_registered_already_adopts_it() {
    // The wizard's "existing runner folder" sends AddRunner, then Register: the registered folder is used as it is.
    let mut h = Harness::new("adopt", "mode=ok\n", true);
    h.pump(Duration::from_secs(1));
    let before = std::fs::read(h.runner_dir.join(".runner")).unwrap();
    let r = h.ask(Request::Register {
        runner: h.name.clone(),
        server_url: "https://git.example.com".into(),
        token: Secret::new("unused-token").unwrap(),
        display_name: None,
    });
    assert_eq!(r, Response::Ok);
    assert_eq!(
        std::fs::read(h.runner_dir.join(".runner")).unwrap(),
        before,
        "the registration is kept as it is"
    );
    h.until("running", Duration::from_millis(200), |h| h.status().status == RunnerStatus::Idle);
    h.finish();
}

#[test]
fn settings_and_bad_requests_are_validated() {
    let mut h = Harness::new("cfg", "mode=ok\n", true);
    let Response::Settings { mut config, .. } = h.ask(Request::GetSettings) else {
        panic!()
    };
    config.runners[0].labels = vec!["has space".into()];
    let r = h.ask(Request::SaveSettings { config: config.clone() });
    assert!(matches!(r, Response::Error(_)), "an invalid label is refused: {r:?}");
    config.runners[0].labels = vec!["macos-latest".into()];
    assert_eq!(h.ask(Request::SaveSettings { config }), Response::Ok);
    assert_eq!(h.status().labels, vec!["macos-latest".to_string()]);
    let other = RunnerName::parse("nope").unwrap();
    assert!(matches!(h.ask(Request::Start { runner: other }), Response::Error(_)));
    let r = h.ask(Request::PinVersion {
        runner: h.name.clone(),
        version: Some(pitcrew_core::version::RunnerVersion::parse("5.1.0").unwrap()),
    });
    assert_eq!(r, Response::Ok);
    assert_eq!(h.status().update_policy.pinned.map(|v| v.to_string()).as_deref(), Some("5.1.0"));
    let Response::Diagnostics { sections } = h.ask(Request::Diagnostics) else {
        panic!()
    };
    assert!(sections.iter().any(|s| s.name == "pitcrew.toml"));
    h.finish();
}

#[test]
fn a_manual_cleanup_reports_what_it_did() {
    let mut h = Harness::new("tidy", "mode=ok\n", true);
    std::fs::create_dir_all(h.runner_dir.join("work/old-job")).unwrap();
    std::fs::write(h.runner_dir.join("work/old-job/blob"), vec![0u8; 4096]).unwrap();
    std::fs::write(h.runner_dir.join("bin").join(format!("gitea-runner{EXE}.download")), b"partial").unwrap();
    let Response::Disk { preview, .. } = h.ask(Request::Disk { runner: h.name.clone() }) else {
        panic!()
    };
    assert!(preview.deletions.iter().any(|d| d.entry.path.ends_with(".download")), "{preview:?}");
    h.pump(Duration::from_secs(1));
    let r = h.ask(Request::CleanDisk { runner: h.name.clone() });
    let Response::Cleaned { report } = r else { panic!("{r:?}") };
    assert!(report.deleted >= 1);
    assert!(!h.runner_dir.join("bin").join(format!("gitea-runner{EXE}.download")).exists());
    assert!(h.runner_dir.join("work/old-job").exists(), "a folder changed minutes ago is kept");
    assert!(count(&h.history(), |k| matches!(k, HistoryKind::Cleanup { .. })) == 1);
    h.finish();
}
