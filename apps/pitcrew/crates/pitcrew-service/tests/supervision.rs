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
    exe: Arc<AtomicU64>,
}

impl Harness {
    fn new(test: &str, fake: &str, registered: bool) -> Harness {
        Harness::with_runner_toml(test, fake, registered, "")
    }

    /// `runner_toml` is appended to the runner's table (e.g. an `[runners.env]` table).
    fn with_runner_toml(test: &str, fake: &str, registered: bool, runner_toml: &str) -> Harness {
        Harness::build(test, fake, registered, runner_toml, None)
    }

    /// `state_json` is written as the service's `state.json` before it starts.
    fn build(test: &str, fake: &str, registered: bool, runner_toml: &str, state_json: Option<&str>) -> Harness {
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
        if let Some(state) = state_json {
            std::fs::write(data.join("state.json"), state).unwrap();
        }
        let clock = Arc::new(AtomicU64::new(pitcrew_service::cleanup::now().0));
        let c = clock.clone();
        let exe = Arc::new(AtomicU64::new(1));
        let exe2 = exe.clone();
        let env = Env {
            data_dir: data,
            launcher: Box::new(OsLauncher),
            sources: Sources::public(),
            platform: pitcrew_core::platform::Platform::current().unwrap(),
            exe_stamp: Arc::new(move || {
                Some(pitcrew_core::self_update::BinaryStamp {
                    len: exe2.load(Ordering::SeqCst),
                    modified_ms: 0,
                })
            }),
            base_path: Arc::new(|| Some("/pitcrew/test-path:/usr/bin:/bin".to_string())),
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
            exe,
        }
    }

    fn now(&self) -> Timestamp {
        Timestamp(self.clock.load(Ordering::SeqCst))
    }

    /// Ends every process the test started: anything running from or inside the harness folder, and their descendants.
    fn kill_leftover_processes(&self) {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_exe(sysinfo::UpdateKind::Always)
                .with_cwd(sysinfo::UpdateKind::Always),
        );
        let root = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        let mut doomed: std::collections::HashSet<Pid> = sys
            .processes()
            .iter()
            .filter(|(_, p)| {
                let inside = |q: Option<&std::path::Path>| q.is_some_and(|q| q.starts_with(&self.root) || q.starts_with(&root));
                let cmd = p.cmd().iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ");
                inside(p.exe()) || inside(p.cwd()) || cmd.contains(&*self.root.to_string_lossy())
            })
            .map(|(pid, _)| *pid)
            .collect();
        loop {
            let before = doomed.len();
            for (pid, p) in sys.processes() {
                if p.parent().is_some_and(|pp| doomed.contains(&pp)) {
                    doomed.insert(*pid);
                }
            }
            if doomed.len() == before {
                break;
            }
        }
        let me = Pid::from_u32(std::process::id());
        for pid in doomed.into_iter().filter(|p| *p != me) {
            if let Some(p) = sys.process(pid) {
                p.kill();
            }
        }
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

    fn finish_stopped_usage(mut self) {
        let _ = self.ask(Request::Stop { runner: self.name.clone() });
        self.until("stopped", Duration::from_secs(6), |h| h.status().running_since.is_none());
        self.pump(Duration::from_secs(6));
        assert!(self.status().usage.is_none(), "a stopped runner uses nothing");
        let _ = std::fs::remove_dir_all(&self.root);
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

impl Drop for Harness {
    /// A failing test never leaves its fake runner (or the processes it started) running: everything whose program,
    /// working folder or command line is in this test's folder is killed with its descendants, then the folder goes.
    fn drop(&mut self) {
        self.kill_leftover_processes();
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
    assert!(text.contains("PATH=/pitcrew/test-path:/usr/bin:/bin\n"), "{text}");
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
    // The job lasts long enough that a loaded CI runner still sees it running between two status calls.
    let mut h = Harness::new("busy", "mode=job\njob_secs=10\n", true);
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
    h.until("stopped after the job", Duration::from_secs(25), |h| {
        h.status().status == RunnerStatus::Stopped
    });
    let hist = h.history();
    assert_eq!(count(&hist, |k| matches!(k, HistoryKind::JobFinished { .. })), 1);
    assert_eq!(count(&hist, |k| matches!(k, HistoryKind::Exited { expected: true, .. })), 1);
    let _ = std::fs::remove_dir_all(&h.root);
}

#[test]
fn usage_follows_the_runner_and_the_processes_it_started() {
    let mut h = Harness::new("meter", "mode=job\njob_secs=6\n", true);
    // The runner alone is one process; on Windows its console host (`conhost.exe`) makes two.
    let base = if cfg!(windows) { 2 } else { 1 };
    // The job's child (the stand-in for a compiler) is counted with the runner.
    h.until("the job's process is counted", Duration::from_secs(6), |h| {
        h.status().usage.is_some_and(|u| u.processes > base)
    });
    let during = h.status().usage.unwrap();
    assert!(during.mem_bytes > 0, "{during:?}");
    assert!(during.cpu_permille <= 1000);
    // It exits, and drops out of the count while the runner stays. The child lives `job_secs` (on Windows `ping -n 7`
    // takes about 7 s, and a loaded runner is slower), so the deadline leaves room. On Windows the runner also has its
    // console host, which stays.
    h.until("the exited child is no longer counted", Duration::from_secs(20), |h| {
        h.status().usage.is_some_and(|u| u.processes == base)
    });
    // The history has the runner's series and the machine series carries the runners' share.
    let name = h.name.clone();
    h.until("a usage sample is stored", Duration::from_secs(31), |h| {
        match h.ask(Request::Metrics {
            runner: Some(name.clone()),
            range_hours: 1,
        }) {
            Response::Metrics { usage, .. } => !usage.is_empty(),
            other => panic!("{other:?}"),
        }
    });
    let Response::Metrics { machine, usage, .. } = h.ask(Request::Metrics {
        runner: Some(name),
        range_hours: 1,
    }) else {
        panic!("metrics")
    };
    assert!(usage.iter().all(|p| p.usage.mem_bytes > 0));
    assert!(
        machine.last().unwrap().runners.mem_bytes > 0,
        "the machine point holds the runners' share"
    );
    // Stopped: no usage is reported for it.
    h.finish_stopped_usage();
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

#[test]
fn a_replaced_service_program_restarts_the_service_only_when_no_job_runs() {
    let mut h = Harness::new("selfup", "mode=job\njob_secs=3\n", true);
    h.until("job seen", Duration::from_secs(6), |h| h.status().current_job.is_some());
    let t = |h: &Harness, extra: u64| Timestamp(h.now().0 + extra);
    // Unchanged: never.
    assert!(!h.sup.restart_for_new_program(t(&h, 60_000)));
    // Replaced while a job runs: wait.
    h.exe.store(2, Ordering::SeqCst);
    assert!(!h.sup.restart_for_new_program(t(&h, 120_000)));
    // Checked at most every 30 s.
    assert!(!h.sup.restart_for_new_program(t(&h, 121_000)));
    // The job ended: restart (the service manager starts the new program, which adopts the runners).
    h.until("job done", Duration::from_secs(6), |h| h.status().current_job.is_none());
    for _ in 0..6 {
        h.pump(Duration::from_secs(60));
    }
    assert!(h.sup.restart_for_new_program(t(&h, 600_000)));
    h.finish();
}

// ---- leftover processes (ADR 0012) and runner configuration (ADR 0011) -------------------------------------------

use pitcrew_core::process_track::Fate;
use pitcrew_core::protocol::{ErrorCode, ProcessList, RestartPlan};
use pitcrew_core::runner_config::ConfigEdit;
use pitcrew_core::runner_settings::RunnerSettings;

/// Whether a process still runs (a zombie nobody reaped does not count).
fn alive(pid: u32) -> bool {
    let out = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    let s = s.trim();
    !s.is_empty() && !s.starts_with('Z')
}

impl Harness {
    fn processes(&mut self) -> ProcessList {
        match self.ask(Request::Processes { runner: self.name.clone() }) {
            Response::Processes { list } => list,
            other => panic!("{other:?}"),
        }
    }

    fn leftover_pid(&mut self, name: &str) -> Option<u32> {
        self.processes()
            .entries
            .iter()
            .find(|e| e.name == name && e.leftover.is_some())
            .map(|e| e.pid)
    }

    /// Runs until the job has ended and its leftovers are classified.
    fn until_leftovers(&mut self, n: u32) {
        self.until("the job started", Duration::from_millis(100), |h| h.status().current_job.is_some());
        self.until("the leftovers are noticed", Duration::from_secs(6), |h| h.status().leftovers >= n);
    }

    fn process_events(&mut self) -> Vec<HistoryKind> {
        self.history()
            .into_iter()
            .map(|e| e.kind)
            .filter(|k| matches!(k, HistoryKind::ProcessStopped { .. }))
            .collect()
    }
}

#[cfg(unix)]
#[test]
fn a_leftover_is_listed_and_stopped_after_the_keep_time_when_the_runner_is_idle() {
    let mut h = Harness::new("lefty", "mode=leftover\njob_secs=2\nleftovers=sleep:600\n", true);
    h.until_leftovers(1);
    let list = h.processes();
    assert!(list.cleanup && list.idle);
    let row = list.entries.iter().find(|e| e.leftover.is_some()).unwrap();
    assert_eq!(row.name, "sleep");
    assert_eq!(row.job.as_ref().and_then(|j| j.repository.as_deref()), Some("owner/app"));
    let since = row.leftover.as_ref().unwrap().since;
    assert_eq!(
        row.leftover.as_ref().unwrap().fate,
        Fate::StopsAt {
            at: Timestamp(since.0 + 600_000)
        }
    );
    assert!(list.entries.iter().any(|e| e.is_runner && e.leftover.is_none()));
    let pid = row.pid;
    // The job's record: peaks and the leftover count.
    let jobs = h
        .history()
        .into_iter()
        .filter_map(|e| match e.kind {
            HistoryKind::JobFinished { job } => Some(job),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0].peak_mem_bytes > 0 && jobs[0].leftovers == 1, "{:?}", jobs[0]);
    // Not before the time...
    for _ in 0..5 {
        h.pump(Duration::from_secs(60));
    }
    assert!(alive(pid), "still within the keep time");
    // ...then asked to stop, once.
    h.until("stopped", Duration::from_secs(60), |h| !h.process_events().is_empty());
    assert!(!alive(pid));
    let HistoryKind::ProcessStopped {
        name,
        pid: p,
        rule,
        by,
        forced,
        ..
    } = h.process_events().remove(0)
    else {
        panic!()
    };
    assert_eq!(
        (name.as_str(), p, by, forced),
        ("sleep", pid, pitcrew_core::history::StopReason::Cleanup, false)
    );
    assert!(rule.contains("default keep time (10 min)"), "{rule}");
    assert!(
        h.log().contains("stopped sleep (process") && h.log().contains("by cleanup"),
        "{}",
        h.log()
    );
    assert_eq!(h.status().leftovers, 0);
    h.finish();
}

#[cfg(unix)]
#[test]
fn rules_set_the_keep_time_per_process_and_never_means_never() {
    let rules = "[runners.processes]\nkeep = { amount = 1, unit = \"minutes\" }\n[[runners.processes.rules]]\npattern = \"tail\"\nkeep = \"never\"\n";
    let mut h = Harness::with_runner_toml("rulesy", "mode=leftover\njob_secs=2\nleftovers=sleep:600,tail\n", true, rules);
    h.until_leftovers(2);
    let sleep = h.leftover_pid("sleep").unwrap();
    let tail = h.leftover_pid("tail").unwrap();
    let list = h.processes();
    let fate = |n: &str| {
        list.entries
            .iter()
            .find(|e| e.name == n)
            .unwrap()
            .leftover
            .as_ref()
            .unwrap()
            .clone()
    };
    assert_eq!(fate("tail").fate, Fate::Never);
    assert_eq!(fate("tail").rule.as_deref(), Some("tail"));
    assert!(matches!(fate("sleep").fate, Fate::StopsAt { .. }));
    h.until("the sleep leftover is stopped", Duration::from_secs(30), |h| {
        !h.process_events().is_empty()
    });
    for _ in 0..4 {
        h.pump(Duration::from_secs(3 * 3600));
    }
    assert!(!alive(sleep));
    assert!(alive(tail), "a never rule is never stopped, however long it lives");
    assert_eq!(h.process_events().len(), 1);
    // The user can still stop it.
    let started = h.processes().entries.iter().find(|e| e.pid == tail).unwrap().started_at;
    let r = h.ask(Request::StopProcess {
        runner: h.name.clone(),
        pid: tail,
        started_at: started,
        allow_runner: false,
    });
    assert_eq!(r, Response::Ok);
    h.until("stopped by the user", Duration::from_secs(5), |h| h.process_events().len() == 2);
    assert!(!alive(tail));
    let mine = h.process_events().into_iter().find(|k| {
        matches!(
            k,
            HistoryKind::ProcessStopped {
                by: pitcrew_core::history::StopReason::User,
                ..
            }
        )
    });
    let Some(HistoryKind::ProcessStopped { rule, .. }) = mine else {
        panic!("no user stop recorded")
    };
    assert_eq!(rule, "you");
    h.finish();
}

#[cfg(unix)]
#[test]
fn keep_longer_postpones_the_stop_and_cleanup_can_be_turned_off() {
    let mut h = Harness::new("keeper", "mode=leftover\njob_secs=2\nleftovers=sleep:600\n", true);
    h.until_leftovers(1);
    let row = h.processes().entries.into_iter().find(|e| e.leftover.is_some()).unwrap();
    let r = h.ask(Request::KeepProcess {
        runner: h.name.clone(),
        pid: row.pid,
        started_at: row.started_at.unwrap(),
        extra: pitcrew_core::process_policy::KeepFor::hours(1),
    });
    assert_eq!(r, Response::Ok);
    h.pump(Duration::from_secs(11 * 60));
    h.pump(Duration::from_secs(6));
    assert!(alive(row.pid), "an hour was added");
    // The extension is in the state file, so a service restart does not lose it.
    h.pump(Duration::from_secs(60));
    let saved = std::fs::read_to_string(h.root.join("data").join("state.json")).unwrap();
    assert!(
        saved.contains(&format!("\"pid\":{}", row.pid)) && saved.contains("\"keeps\""),
        "{saved}"
    );
    // The runner itself cannot be "kept longer": only leftovers can.
    let me = h.processes().entries.into_iter().find(|e| e.is_runner).unwrap();
    let r = h.ask(Request::KeepProcess {
        runner: h.name.clone(),
        pid: me.pid,
        started_at: me.started_at.unwrap(),
        extra: pitcrew_core::process_policy::KeepFor::hours(1),
    });
    assert!(matches!(r, Response::Error(ref e) if e.code == ErrorCode::BadRequest), "{r:?}");
    // Turning cleanup off in the settings keeps leftovers listed, with no countdown, and stops nothing.
    let Response::Settings { mut config, .. } = h.ask(Request::GetSettings) else {
        panic!()
    };
    config.defaults.processes.cleanup = false;
    assert_eq!(h.ask(Request::SaveSettings { config }), Response::Ok);
    h.pump(Duration::from_secs(6));
    assert_eq!(h.leftover_pid("sleep"), Some(row.pid));
    assert_eq!(
        h.processes()
            .entries
            .iter()
            .find(|e| e.pid == row.pid)
            .unwrap()
            .leftover
            .as_ref()
            .unwrap()
            .fate,
        Fate::Off
    );
    h.pump(Duration::from_secs(5 * 3600));
    h.pump(Duration::from_secs(6));
    assert!(alive(row.pid));
    let _ = ErrorCode::Busy;
    h.ask(Request::Stop { runner: h.name.clone() });
    h.finish();
    unsafe_kill(row.pid);
}

#[cfg(unix)]
fn unsafe_kill(pid: u32) {
    let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
}

#[cfg(unix)]
#[test]
fn nothing_is_stopped_while_a_job_runs() {
    let mut h = Harness::new("busyleft", "mode=leftover\njob_secs=2\nleftovers=sleep:600\n", true);
    h.until_leftovers(1);
    let pid = h.leftover_pid("sleep").unwrap();
    let name = h.name.clone();
    // A new job keeps the runner busy past the keep time (the runner says "task started" again and again).
    for _ in 0..80 {
        h.sup.handle(Msg::Line {
            runner: name.clone(),
            generation: 1,
            text: r#"time="2026-10-22T06:57:19+08:00" level=info msg="task started" job=next"#.into(),
        });
        h.pump(Duration::from_secs(10));
    }
    assert!(h.status().current_job.is_some());
    assert!(alive(pid), "a busy runner's leftovers wait");
    assert!(!h.processes().idle);
    // The job ends; the overdue leftover is stopped.
    h.until("stopped once idle", Duration::from_secs(6), |h| !h.process_events().is_empty());
    assert!(!alive(pid));
    h.finish();
}

#[cfg(unix)]
#[test]
fn a_stop_outside_the_runners_tree_is_refused() {
    let mut h = Harness::new("fence", "mode=leftover\njob_secs=2\nleftovers=sleep:600\n", true);
    h.until_leftovers(1);
    let stranger = std::process::Command::new("sleep").arg("120").spawn();
    let mut stranger = stranger.unwrap();
    let refuse = |h: &mut Harness, pid: u32, allow: bool| match h.ask(Request::StopProcess {
        runner: h.name.clone(),
        pid,
        started_at: None,
        allow_runner: allow,
    }) {
        Response::Error(e) => {
            assert_eq!(e.code, ErrorCode::BadRequest);
            e.message
        }
        other => panic!("{other:?}"),
    };
    assert!(refuse(&mut h, stranger.id(), false).contains("not part of this runner's process tree"));
    assert!(
        refuse(&mut h, std::process::id(), true).contains("not part of this runner's process tree"),
        "not even the service"
    );
    assert!(refuse(&mut h, 1, true).contains("not part of"));
    let runner_pid = h.processes().runner_pid.unwrap();
    assert!(refuse(&mut h, runner_pid, false).contains("the runner itself"));
    assert!(stranger.try_wait().unwrap().is_none(), "the stranger was not touched");
    assert!(alive(runner_pid));
    // A pid that the app saw start at another time is refused (a reused process id).
    let row = h.processes().entries.into_iter().find(|e| e.leftover.is_some()).unwrap();
    let r = h.ask(Request::StopProcess {
        runner: h.name.clone(),
        pid: row.pid,
        started_at: Some(Timestamp(5_000)),
        allow_runner: false,
    });
    assert!(
        matches!(r, Response::Error(ref e) if e.message.contains("different process")),
        "{r:?}"
    );
    assert!(alive(row.pid));
    let _ = stranger.kill();
    let _ = stranger.wait();
    h.finish();
    unsafe_kill(row.pid);
}

fn write_config(h: &Harness, text: &str) {
    std::fs::write(h.runner_dir.join("config.yaml"), text).unwrap();
}

fn get_config(h: &mut Harness) -> (String, String) {
    match h.ask(Request::GetRunnerConfig { runner: h.name.clone() }) {
        Response::RunnerConfig { text, fingerprint, .. } => (text, fingerprint),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_saved_configuration_restarts_the_runner_after_its_job_and_the_labels_are_verified() {
    let mut h = Harness::new("cfgsave", "mode=job\njob_secs=4\n", true);
    let original = "# my runner\nlog:\n  level: info # keep this\nrunner:\n  capacity: 1\n";
    write_config(&h, original);
    h.until("job seen", Duration::from_millis(200), |h| h.status().current_job.is_some());
    let before = h.status().running_since;
    let (text, fp) = get_config(&mut h);
    assert_eq!(text, original);
    let Response::RunnerConfig { view, restart_pending, .. } = h.ask(Request::GetRunnerConfig { runner: h.name.clone() }) else {
        panic!()
    };
    assert_eq!(view.settings.capacity, Some(1));
    assert!(!restart_pending);
    // A preview writes nothing.
    let edit = ConfigEdit::Form {
        settings: RunnerSettings {
            capacity: Some(2),
            labels: vec!["x:host".into()],
            log_level: Some(pitcrew_core::runner_settings::LogLevel::Info),
            ..Default::default()
        },
    };
    let Response::RunnerConfigPreview { preview } = h.ask(Request::PreviewRunnerConfig {
        runner: h.name.clone(),
        edit: edit.clone(),
    }) else {
        panic!()
    };
    assert!(preview.can_save() && preview.restart_needed && preview.diff.is_changed());
    assert_eq!(std::fs::read_to_string(h.runner_dir.join("config.yaml")).unwrap(), original);
    // A stale base is refused.
    let r = h.ask(Request::SaveRunnerConfig {
        runner: h.name.clone(),
        edit: edit.clone(),
        base: "stale".into(),
    });
    assert!(matches!(r, Response::Error(ref e) if e.code == ErrorCode::Busy), "{r:?}");
    // Invalid values are refused by the service itself.
    let bad = ConfigEdit::Form {
        settings: RunnerSettings {
            capacity: Some(0),
            ..Default::default()
        },
    };
    let r = h.ask(Request::SaveRunnerConfig {
        runner: h.name.clone(),
        edit: bad,
        base: fp.clone(),
    });
    assert!(
        matches!(r, Response::Error(ref e) if e.code == ErrorCode::BadRequest && e.message.contains("capacity")),
        "{r:?}"
    );
    let r = h.ask(Request::SaveRunnerConfig {
        runner: h.name.clone(),
        edit: ConfigEdit::Raw { text: "runner: [".into() },
        base: fp.clone(),
    });
    assert!(matches!(r, Response::Error(ref e) if e.message.contains("not valid YAML")), "{r:?}");
    assert_eq!(
        std::fs::read_to_string(h.runner_dir.join("config.yaml")).unwrap(),
        original,
        "nothing was written"
    );
    // The real save.
    let r = h.ask(Request::SaveRunnerConfig {
        runner: h.name.clone(),
        edit,
        base: fp,
    });
    let Response::RunnerConfigSaved {
        backup,
        restart,
        changed_keys,
        ..
    } = r
    else {
        panic!("{r:?}")
    };
    assert_eq!(restart, RestartPlan::AfterJob);
    assert!(changed_keys.contains(&"runner.capacity".to_string()));
    let saved = std::fs::read_to_string(h.runner_dir.join("config.yaml")).unwrap();
    assert!(
        saved.contains("# my runner") && saved.contains("level: info # keep this") && saved.contains("capacity: 2"),
        "{saved}"
    );
    assert_eq!(std::fs::read_to_string(h.runner_dir.join(backup.unwrap())).unwrap(), original);
    assert!(!h.log().contains("capacity: 2"), "the file's text is not logged");
    // The runner finishes its job first; it is not restarted under it.
    assert_eq!(h.status().pending, Some(pitcrew_core::controller::Pending::Restart));
    assert_eq!(h.status().running_since, before);
    h.until("restarted after the job", Duration::from_secs(6), |h| {
        count(&h.history(), |k| *k == HistoryKind::Started) >= 2
    });
    h.until("labels verified", Duration::from_secs(6), |h| {
        count(&h.history(), |k| matches!(k, HistoryKind::LabelsVerified { .. })) == 1
    });
    assert!(h.log().contains("declared the labels from its configuration: x"), "{}", h.log());
    assert_eq!(
        count(&h.history(), |k| matches!(k, HistoryKind::ConfigSaved { restart: true, .. })),
        1
    );
    // A comment-only change needs no restart.
    let (_, fp) = get_config(&mut h);
    let r = h.ask(Request::SaveRunnerConfig {
        runner: h.name.clone(),
        edit: ConfigEdit::Raw {
            text: saved.replace("# my runner", "# my renamed runner"),
        },
        base: fp,
    });
    assert!(
        matches!(
            r,
            Response::RunnerConfigSaved {
                restart: RestartPlan::NotNeeded,
                ..
            }
        ),
        "{r:?}"
    );
    assert!(h.status().pending.is_none());
    h.finish();
}

#[test]
fn a_label_mismatch_after_the_restart_is_reported_and_a_stopped_runner_just_saves() {
    let mut h = Harness::new("cfgmis", "mode=ok\ndeclare=other\n", true);
    h.until("running", Duration::from_millis(200), |h| h.status().status == RunnerStatus::Idle);
    let (_, fp) = get_config(&mut h);
    let edit = ConfigEdit::Form {
        settings: RunnerSettings {
            labels: vec!["wanted:host".into()],
            ..Default::default()
        },
    };
    let r = h.ask(Request::SaveRunnerConfig {
        runner: h.name.clone(),
        edit,
        base: fp,
    });
    assert!(
        matches!(
            r,
            Response::RunnerConfigSaved {
                restart: RestartPlan::AfterJob,
                ..
            }
        ),
        "{r:?}"
    );
    h.until("mismatch", Duration::from_secs(6), |h| {
        count(&h.history(), |k| matches!(k, HistoryKind::LabelsMismatch { .. })) == 1
    });
    assert!(
        h.log().contains("declared [other] but its configuration asks for [wanted]"),
        "{}",
        h.log()
    );
    assert_eq!(count(&h.history(), |k| matches!(k, HistoryKind::LabelsVerified { .. })), 0);
    // Stopped: the next start uses the file.
    h.ask(Request::Stop { runner: h.name.clone() });
    h.until("stopped", Duration::from_secs(6), |h| h.status().running_since.is_none());
    let (_, fp) = get_config(&mut h);
    let edit = ConfigEdit::Form {
        settings: RunnerSettings {
            labels: vec!["wanted:host".into()],
            capacity: Some(3),
            ..Default::default()
        },
    };
    let r = h.ask(Request::SaveRunnerConfig {
        runner: h.name.clone(),
        edit,
        base: fp,
    });
    assert!(
        matches!(
            r,
            Response::RunnerConfigSaved {
                restart: RestartPlan::NotRunning,
                ..
            }
        ),
        "{r:?}"
    );
    let _ = std::fs::remove_dir_all(&h.root);
}

fn set_defaults(h: &mut Harness, f: impl FnOnce(&mut pitcrew_core::config::Defaults)) {
    let Response::Settings { mut config, .. } = h.ask(Request::GetSettings) else {
        panic!()
    };
    f(&mut config.defaults);
    assert_eq!(h.ask(Request::SaveSettings { config }), Response::Ok);
}

#[test]
fn a_nearly_full_disk_drains_the_runner_and_it_resumes_when_space_recovers() {
    let mut h = Harness::new("diskguard", "mode=idle\n", true);
    h.until("running", Duration::from_millis(200), |h| h.status().running_since.is_some());
    assert!(h.status().hold.is_none());
    // Any real disk is "nearly full" against these thresholds.
    set_defaults(&mut h, |d| {
        d.disk_guard.drain_below_mb = 1_000_000_000;
        d.disk_guard.resume_above_mb = 2_000_000_000;
        d.disk_guard.reserve_mb = 0;
    });
    h.until("held", Duration::from_secs(6), |h| h.status().hold.is_some());
    let s = h.status();
    assert_eq!(s.hold.as_ref().unwrap().reasons, vec![pitcrew_core::hold::HoldReason::LowDisk]);
    h.until("paused", Duration::from_secs(6), |h| h.status().status == RunnerStatus::Paused);
    assert!(count(&h.history(), |k| matches!(k, HistoryKind::Held { .. })) == 1);
    // Space "recovers": the thresholds drop below the real free space. The disk is read at most once a minute.
    set_defaults(&mut h, |d| {
        d.disk_guard.drain_below_mb = 2;
        d.disk_guard.resume_above_mb = 3;
        d.disk_guard.reserve_mb = 1;
    });
    h.pump(Duration::from_secs(61));
    h.until("resumed", Duration::from_secs(6), |h| {
        h.status().hold.is_none() && h.status().status != RunnerStatus::Paused
    });
    assert_eq!(count(&h.history(), |k| matches!(k, HistoryKind::Released { .. })), 1);
    h.finish();
}

#[test]
fn a_pause_the_user_made_is_not_lifted_by_a_hold_that_ends() {
    let mut h = Harness::new("userpause", "mode=idle\n", true);
    h.until("running", Duration::from_millis(200), |h| h.status().running_since.is_some());
    set_defaults(&mut h, |d| {
        d.pause.mode = pitcrew_core::pause::PauseMode::Schedule;
        // The harness clock reads Monday 12:00.
        d.pause.windows = vec![pitcrew_core::update::UpdateWindow {
            days: vec![],
            start: pitcrew_core::update::TimeOfDay::new(11, 0).unwrap(),
            end: pitcrew_core::update::TimeOfDay::new(13, 0).unwrap(),
        }];
    });
    h.until("held by the window", Duration::from_secs(6), |h| {
        h.status()
            .hold
            .is_some_and(|x| x.reasons == vec![pitcrew_core::hold::HoldReason::Schedule])
    });
    assert_eq!(h.status().hold.unwrap().minutes_left, Some(60));
    h.until("paused", Duration::from_secs(6), |h| h.status().status == RunnerStatus::Paused);
    // The user resumes by hand inside the window: Pitcrew leaves it running.
    assert_eq!(h.ask(Request::Resume { runner: h.name.clone() }), Response::Ok);
    h.until("running again", Duration::from_secs(6), |h| {
        h.status().status != RunnerStatus::Paused
    });
    for _ in 0..3 {
        h.pump(Duration::from_secs(6));
        let evs: Vec<String> = h.history().iter().map(|e| format!("{:?}", e.kind)).collect();
        assert_ne!(h.status().status, RunnerStatus::Paused, "{:?} {evs:?}", h.status().hold);
    }
    // And a pause of the user's own stays when the window closes.
    assert_eq!(h.ask(Request::Drain { runner: h.name.clone() }), Response::Ok);
    h.until("paused", Duration::from_secs(6), |h| h.status().status == RunnerStatus::Paused);
    set_defaults(&mut h, |d| d.pause.mode = pitcrew_core::pause::PauseMode::Off);
    h.pump(Duration::from_secs(6));
    h.pump(Duration::from_secs(6));
    assert_eq!(h.status().status, RunnerStatus::Paused);
    assert_eq!(count(&h.history(), |k| matches!(k, HistoryKind::Released { .. })), 0);
    h.finish();
}

#[test]
fn an_update_that_keeps_failing_is_reported_as_a_loop_with_its_cause() {
    let now = pitcrew_service::cleanup::now().0;
    let failed = |i: u64| {
        format!(
            r#"{{"at":{},"kind":"updateFinished","outcome":{{"result":"failed","plan":{{"from":"1.0.0","to":"1.1.0"}},"reason":"no space left","needsAttention":false}}}}"#,
            now - i * 60_000
        )
    };
    let state = format!(
        r#"{{"version":1,"runners":{{"loopy":{{"latest":"1.1.0","history":[{},{},{}]}}}}}}"#,
        failed(3),
        failed(2),
        failed(1)
    );
    let mut h = Harness::build("loopy", "mode=idle\n", true, "", Some(&state));
    h.pump(Duration::from_secs(6));
    let s = h.status();
    let l = s.update_loop.expect("a loop is reported");
    assert_eq!(l.target.to_string(), "1.1.0");
    assert!(l.cause.contains("no space left"), "{}", l.cause);
    h.finish();
}
