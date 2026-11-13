#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Long operations as jobs, end to end over the fake platform and a real IPC connection: a start answers at once, the
//! job reports phases that only grow, Cancel stops it and leaves the state as it was, and a limit is named. Gates stand
//! in for slow sources, so nothing here waits on a clock. Files are created only in temp folders.

use dyno_agent::cli::{ask, connect, run_job};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::extras::files::{FinderMode, FinderQuery};
use dyno_core::issues::{Issue, IssueKind, IssueSource, IssueSourceId, IssuesQuery, ReadControl};
use dyno_core::jobs::{JobOutput, JobSpec};
use dyno_core::ports::PlatformError;
use dyno_core::protocol::{ErrorCode, Message, Reply, Request};
use dyno_core::time::Timestamp;
use dyno_platform::fake::{process, Fake};
use paddock_ipc::{BodyOptions, BodyPeer};
use paddock_jobs::wire::{WireSnapshot, WireState};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// A door a test opens: sources wait at it until then.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
    waiting: Mutex<u32>,
    /// Signalled when a caller arrives, so a test parks instead of spinning.
    arrived: Condvar,
}

impl Gate {
    fn pass(&self) {
        self.arrive();
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.cv.wait(open).unwrap();
        }
    }

    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.cv.notify_all();
    }

    /// Tells a waiting test that a caller got this far, without holding the caller.
    fn arrive(&self) {
        *self.waiting.lock().unwrap() += 1;
        self.arrived.notify_all();
    }

    /// Returns when `n` callers have reached the gate.
    fn wait_for_callers(&self, n: u32) {
        let mut waiting = self.waiting.lock().unwrap();
        while *waiting < n {
            waiting = self.arrived.wait(waiting).unwrap();
        }
    }
}

struct Slow {
    id: IssueSourceId,
    gate: Arc<Gate>,
    issues: Vec<Issue>,
}

impl IssueSource for Slow {
    fn id(&self) -> IssueSourceId {
        self.id
    }
    fn read(&self, _since: Timestamp, _until: Timestamp) -> Result<Vec<Issue>, PlatformError> {
        self.gate.pass();
        Ok(self.issues.clone())
    }
}

struct Harness {
    running: Option<Running>,
    dirs: Dirs,
    _tmp: paddock_testkit::PrivateTemp,
}

impl Harness {
    fn start(sources: Vec<Box<dyn IssueSource>>) -> Harness {
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        fake.add_process(process(100, "calm", 1.0));
        let mut platform = fake.platform();
        platform.issues = sources;
        let running = Running::start(dirs.clone(), platform, Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
        Harness {
            running: Some(running),
            dirs,
            _tmp: tmp,
        }
    }

    fn client(&self) -> BodyPeer<Message> {
        connect(&self.dirs).unwrap().0
    }

    fn shared(&self) -> Arc<dyno_agent::agent::Shared> {
        self.running.as_ref().unwrap().shared.clone()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

fn start(peer: &BodyPeer<Message>, spec: JobSpec) -> Result<WireSnapshot, String> {
    match ask(peer, Request::StartJob { spec })? {
        Reply::Job(s) => Ok(*s),
        other => panic!("{other:?}"),
    }
}

fn status(peer: &BodyPeer<Message>, id: u64) -> WireSnapshot {
    let Reply::Jobs(all) = ask(peer, Request::Jobs).unwrap() else {
        panic!("jobs")
    };
    all.into_iter().find(|s| s.id == id).expect("the job is listed")
}

fn wait_final(peer: &BodyPeer<Message>, id: u64) -> WireSnapshot {
    for _ in 0..2000 {
        let s = status(peer, id);
        if s.is_terminal() {
            return s;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the job did not end");
}

fn issue(key: &str) -> Issue {
    Issue::new(
        IssueSourceId::MacReports,
        Timestamp(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
        ),
        IssueKind::AppCrash,
        "Mail",
        "t",
        "evidence",
        key,
    )
}

fn slow_issues(gate: &Arc<Gate>) -> Vec<Box<dyn IssueSource>> {
    vec![
        Box::new(Slow {
            id: IssueSourceId::MacReports,
            gate: gate.clone(),
            issues: vec![issue("a")],
        }),
        Box::new(Slow {
            id: IssueSourceId::MacLog,
            gate: gate.clone(),
            issues: vec![],
        }),
    ]
}

#[test]
fn a_start_answers_at_once_while_the_work_is_still_going_and_the_phase_has_a_count() {
    let gate = Arc::new(Gate::default());
    let h = Harness::start(slow_issues(&gate));
    let peer = h.client();
    let first = start(
        &peer,
        JobSpec::RescanIssues {
            query: IssuesQuery { days: 14 },
        },
    )
    .unwrap();
    assert!(!first.is_terminal(), "the start does not wait for the logs");
    gate.wait_for_callers(1);
    let running = status(&peer, first.id);
    let WireState::Running(Some(p)) = running.state else {
        panic!("{running:?}")
    };
    // The read of the first source: a count that starts at 0 and has no total (a growing count, never a static 0 of N).
    assert_eq!((p.id.as_str(), p.done, p.total), ("source-MacReports", 0, None));
    assert_eq!(running.title, "Reading system logs");
    // The same work asked again answers the running job; another window of the same kind is refused with a reason.
    assert_eq!(
        start(
            &peer,
            JobSpec::RescanIssues {
                query: IssuesQuery { days: 14 }
            }
        )
        .unwrap()
        .id,
        first.id
    );
    let busy = start(
        &peer,
        JobSpec::RescanIssues {
            query: IssuesQuery { days: 7 },
        },
    )
    .unwrap_err();
    assert!(busy.contains("already running"), "{busy}");
    // The result of a job that has not ended is not there yet.
    assert!(ask(&peer, Request::JobResult { id: first.id }).unwrap_err().contains("no result"));
    gate.open();
    let done = wait_final(&peer, first.id);
    assert_eq!(done.state, WireState::Done);
    let Reply::JobOutput(out) = ask(&peer, Request::JobResult { id: first.id }).unwrap() else {
        panic!("output")
    };
    assert!(matches!(*out, JobOutput::Issues(o) if o.total == 1));
}

#[test]
fn cancelling_a_log_read_keeps_what_was_there_and_the_next_read_works() {
    let gate = Arc::new(Gate::default());
    let h = Harness::start(slow_issues(&gate));
    let peer = h.client();
    let job = start(
        &peer,
        JobSpec::RescanIssues {
            query: IssuesQuery { days: 14 },
        },
    )
    .unwrap();
    gate.wait_for_callers(1);
    // The read in progress cannot be interrupted and says so; Cancel is still accepted and takes effect when it ends.
    let mid = status(&peer, job.id);
    assert!(!mid.cancellable && mid.not_cancellable_reason.is_some_and(|r| r.contains("Cancel takes effect")));
    ask(&peer, Request::CancelJob { id: job.id }).unwrap();
    gate.open();
    let end = wait_final(&peer, job.id);
    assert!(matches!(end.state, WireState::Cancelled(_)), "{:?}", end.state);
    assert!(
        ask(&peer, Request::JobResult { id: job.id }).is_err(),
        "a cancelled job has no result"
    );
    // Nothing was stored and the sources still read "not read yet": the cancel changed nothing.
    let Reply::Issues(o) = ask(
        &peer,
        Request::Issues {
            query: IssuesQuery { days: 14 },
        },
    )
    .unwrap() else {
        panic!("issues")
    };
    assert_eq!(o.total, 0);
    assert!(h.shared().issues.never_scanned());
    // The scan flag came down: a new read runs and finds the issue.
    let JobOutput::Issues(o) = run_job(
        &peer,
        JobSpec::RescanIssues {
            query: IssuesQuery { days: 14 },
        },
        Duration::from_secs(30),
    )
    .unwrap() else {
        panic!("issues")
    };
    assert_eq!(o.total, 1);
}

#[test]
fn a_limit_is_named_in_the_result_and_a_bad_folder_fails_with_what_to_fix() {
    let h = Harness::start(vec![]);
    let peer = h.client();
    let tmp = paddock_testkit::private_tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap().join("deep");
    let mut dir = root.clone();
    for i in 0..(dyno_core::extras::files::MAX_DEPTH + 2) {
        dir = dir.join(format!("d{i}"));
    }
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("far.bin"), vec![1u8; 2 * 1024 * 1024]).unwrap();
    let query = FinderQuery {
        folder: root.to_string_lossy().into_owned(),
        mode: FinderMode::Large,
        min_bytes: 0,
    };
    let JobOutput::Finder(r) = run_job(&peer, JobSpec::FindFiles { query }, Duration::from_secs(30)).unwrap() else {
        panic!("finder")
    };
    let hit = r.limit.as_ref().unwrap();
    assert_eq!(hit.limit, dyno_core::extras::files::FinderLimit::Depth);
    assert!(hit.message.contains("Choose a folder nearer"), "{}", hit.message);
    assert!(r.large.is_empty(), "the file below the depth limit is left out");
    // A blank folder is refused before any work is queued.
    let blank = FinderQuery {
        folder: " ".into(),
        mode: FinderMode::Both,
        min_bytes: 0,
    };
    let refused = ask(
        &peer,
        Request::StartJob {
            spec: JobSpec::FindFiles { query: blank },
        },
    )
    .unwrap_err();
    assert!(refused.contains("Choose a folder"), "{refused}");
    // A folder that is not there is a failed job whose message says what to do.
    let gone = FinderQuery {
        folder: root.join("not-here").to_string_lossy().into_owned(),
        mode: FinderMode::Both,
        min_bytes: 0,
    };
    let err = run_job(&peer, JobSpec::FindFiles { query: gone }, Duration::from_secs(30)).unwrap_err();
    assert!(err.contains("cannot be opened"), "{err}");
}

#[test]
fn only_the_last_few_finished_jobs_are_kept_and_an_unknown_id_is_not_found() {
    let h = Harness::start(vec![]);
    let peer = h.client();
    // Another job that has not ended, as the agent's own start-up job (the match index of the lists) can still be on a slow machine
    // (Windows CI run 7510: 17 listed against 16): it is listed besides the finished ones and never counts among them.
    let _alive = h.shared().jobs.register_external("test", "A job that has not ended", None);
    let tmp = paddock_testkit::private_tempdir().unwrap();
    let f = tmp.path().join("p.bin");
    std::fs::write(&f, b"abc").unwrap();
    for _ in 0..(dyno_core::jobs::KEPT_FINISHED + 4) {
        let spec = JobSpec::FileHash {
            source: dyno_core::weblinks::HashSource::Path(f.to_string_lossy().into_owned()),
        };
        run_job(&peer, spec, Duration::from_secs(30)).unwrap();
    }
    let Reply::Jobs(all) = ask(&peer, Request::Jobs).unwrap() else {
        panic!("jobs")
    };
    // Only the last few jobs that ended are kept; a job that has not ended is listed besides them.
    let ended: Vec<_> = all.iter().filter(|j| j.is_terminal()).collect();
    assert_eq!(ended.len(), dyno_core::jobs::KEPT_FINISHED, "{all:?}");
    assert!(all.iter().any(|j| !j.is_terminal()), "the job that has not ended is listed");
    assert!(all.windows(2).all(|w| w[0].id < w[1].id && w[0].seq >= 1));
    let err = peer
        .call(Message::Request(Box::new(Request::JobResult { id: 9999 })), Duration::from_secs(5))
        .unwrap();
    assert!(matches!(err, Message::Reply(r) if matches!(*r, Err(ref e) if e.code == ErrorCode::NotFound)));
    // Cancelling an unknown or finished job is harmless.
    assert!(matches!(
        ask(&peer, Request::CancelJob { id: 9999 }).unwrap(),
        Reply::JobCancelled(false)
    ));
}

/// A source that streams its tool: it reports a growing count of records and, like the real one, stops at once when cancelled
/// (the real one kills `log show`; this one has no child to kill, only the same signal).
struct Streaming {
    started: Arc<Gate>,
    finished: Arc<Mutex<Option<bool>>>,
}

impl IssueSource for Streaming {
    fn id(&self) -> IssueSourceId {
        IssueSourceId::MacLog
    }
    fn read(&self, since: Timestamp, until: Timestamp) -> Result<Vec<Issue>, PlatformError> {
        self.read_controlled(since, until, &dyno_core::issues::NoControl)
    }
    fn controllable(&self) -> bool {
        true
    }
    fn read_controlled(&self, _since: Timestamp, _until: Timestamp, control: &dyn ReadControl) -> Result<Vec<Issue>, PlatformError> {
        // Cancel wakes this read through the registered stop, as it kills a child process.
        let stopped = Arc::new((Mutex::new(false), Condvar::new()));
        let s = stopped.clone();
        let _guard = control.on_cancel(Box::new(move || {
            *s.0.lock().unwrap() = true;
            s.1.notify_all();
        }));
        let mut records = 0u64;
        loop {
            records += 100;
            control.parsed(records);
            if records == 300 {
                self.started.arrive();
            }
            let stop = stopped.0.lock().unwrap();
            let (stop, _) = stopped.1.wait_timeout(stop, Duration::from_millis(5)).unwrap();
            if *stop {
                *self.finished.lock().unwrap() = Some(control.cancelled());
                return Err(PlatformError::Failed("log was stopped.".into()));
            }
        }
    }
}

#[test]
fn cancelling_a_streaming_log_read_stops_its_tool_at_once_with_a_growing_count_and_changes_nothing() {
    let started = Arc::new(Gate::default());
    let finished = Arc::new(Mutex::new(None));
    let h = Harness::start(vec![Box::new(Streaming {
        started: started.clone(),
        finished: finished.clone(),
    })]);
    let peer = h.client();
    let job = start(
        &peer,
        JobSpec::RescanIssues {
            query: IssuesQuery { days: 14 },
        },
    )
    .unwrap();
    started.wait_for_callers(1);
    // The count grew while the tool streamed, and the read declares nothing uninterruptible.
    let mid = status(&peer, job.id);
    let WireState::Running(Some(p)) = mid.state.clone() else {
        panic!("{mid:?}")
    };
    assert_eq!(p.id, "source-MacLog");
    assert!(p.done >= 100 && p.total.is_none(), "{p:?}");
    assert!(mid.cancellable, "a streamed read can be interrupted: {mid:?}");
    // The gate was never opened: the read ends only because Cancel reached it.
    let cancelled_at = std::time::Instant::now();
    ask(&peer, Request::CancelJob { id: job.id }).unwrap();
    let end = wait_final(&peer, job.id);
    assert!(matches!(end.state, WireState::Cancelled(_)), "{:?}", end.state);
    assert!(cancelled_at.elapsed() < Duration::from_secs(5), "Cancel did not wait for the tool");
    assert_eq!(*finished.lock().unwrap(), Some(true));
    assert!(
        h.shared().issues.never_scanned(),
        "nothing was stored and the time of the last read did not move"
    );
}

#[test]
fn a_rescan_waits_for_the_scheduled_read_instead_of_answering_with_the_old_state_and_cancel_wakes_the_wait() {
    let gate = Arc::new(Gate::default());
    let h = Harness::start(slow_issues(&gate));
    let shared = h.shared();
    // The scheduled read (no job) is in progress, held at the gate.
    let scheduled = {
        let s = shared.clone();
        std::thread::spawn(move || s.issues.scan(&s, None))
    };
    gate.wait_for_callers(1);
    let peer = h.client();
    let waiting = start(
        &peer,
        JobSpec::RescanIssues {
            query: IssuesQuery { days: 14 },
        },
    )
    .unwrap();
    for _ in 0..400 {
        if matches!(&status(&peer, waiting.id).state, WireState::Running(Some(p)) if p.id == "waiting") {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let WireState::Running(Some(p)) = status(&peer, waiting.id).state else {
        panic!("running")
    };
    assert_eq!(p.id, "waiting", "the rescan says what it waits for");
    // Cancel wakes the wait: the gate is still closed, yet the job ends.
    ask(&peer, Request::CancelJob { id: waiting.id }).unwrap();
    assert!(matches!(wait_final(&peer, waiting.id).state, WireState::Cancelled(_)));
    // Another rescan waits too, and when the scheduled read ends it answers with what that read stored.
    let again = start(
        &peer,
        JobSpec::RescanIssues {
            query: IssuesQuery { days: 14 },
        },
    )
    .unwrap();
    gate.open();
    assert!(scheduled.join().unwrap().unwrap());
    assert_eq!(wait_final(&peer, again.id).state, WireState::Done);
    let Reply::JobOutput(out) = ask(&peer, Request::JobResult { id: again.id }).unwrap() else {
        panic!("output")
    };
    assert!(
        matches!(*out, JobOutput::Issues(o) if o.total == 1),
        "the scheduled read's issue is in the answer"
    );
}

/// A read that has its results but has not stored them is still running: a second caller finds it running and leaves at once, and a
/// rescan that waits for it answers with what it stored. The read is held between its sources and the store (`scan_hooked`), so the
/// moment is exact. Before the fix the flag came down (and the waiters were woken) before the store, so the second caller started a
/// read of its own and a waiting rescan could answer from an empty store.
#[test]
fn a_running_read_stays_running_until_its_results_are_stored() {
    let sources = Arc::new(Gate::default());
    sources.open();
    let h = Harness::start(slow_issues(&sources));
    let shared = h.shared();
    let held = Arc::new(Gate::default());
    let scheduled = {
        let (s, held) = (shared.clone(), held.clone());
        std::thread::spawn(move || s.issues.scan_hooked(&s, None, &|| {}, &|| held.pass()))
    };
    // The scheduled read has read both sources and waits before storing them.
    held.wait_for_callers(1);
    assert!(shared.issues.never_scanned(), "nothing is stored yet");
    assert!(
        !shared.issues.scan(&shared, None).unwrap(),
        "the read has not stored its results, so it is still running: a second caller leaves it alone"
    );
    held.open();
    assert!(scheduled.join().unwrap().unwrap());
    assert!(!shared.issues.never_scanned());
}

#[test]
fn every_status_of_a_job_is_pushed_to_a_connected_app_in_order_and_the_end_is_never_dropped() {
    let h = Harness::start(vec![]);
    let d = dyno_agent::discovery::Discovery::read(&h.dirs.discovery()).unwrap();
    let conn = dyno_agent::wire::connect(&d.endpoint, &d.token().unwrap()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let peer = BodyPeer::start(
        conn,
        BodyOptions::new("the agent").on_push(move |m| {
            if let Message::Job(j) = m {
                let _ = tx.send(*j);
            }
        }),
    )
    .unwrap();
    let tmp = paddock_testkit::private_tempdir().unwrap();
    let f = tmp.path().join("p.bin");
    std::fs::write(&f, vec![1u8; 300_000]).unwrap();
    let job = start(
        &peer,
        JobSpec::FileHash {
            source: dyno_core::weblinks::HashSource::Path(f.to_string_lossy().into_owned()),
        },
    )
    .unwrap();
    let mut heard = Vec::new();
    loop {
        let j = rx.recv_timeout(Duration::from_secs(10)).expect("the end of the job was pushed");
        if j.id != job.id {
            continue;
        }
        let end = j.is_terminal();
        heard.push(j);
        if end {
            break;
        }
    }
    assert!(
        heard.windows(2).all(|w| w[0].seq < w[1].seq),
        "newest wins, in order: {:?}",
        heard.iter().map(|j| j.seq).collect::<Vec<_>>()
    );
    assert_eq!(heard.last().unwrap().state, WireState::Done);
    let counts: Vec<u64> = heard
        .iter()
        .filter_map(|j| {
            if let WireState::Running(Some(p)) = &j.state {
                Some(p.done)
            } else {
                None
            }
        })
        .collect();
    assert!(counts.windows(2).all(|w| w[0] <= w[1]), "{counts:?}");
}

/// Runs `f` and fails (instead of hanging the run) when it does not return within a few seconds.
fn within<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_secs(5))
        .unwrap_or_else(|_| panic!("{what} did not return: it is waiting on itself"))
}

/// A job whose token was cancelled before it began (the user pressed Cancel while it was still queued behind a long read).
fn cancelled_job() -> paddock_jobs::JobContext {
    struct Quiet;
    impl paddock_jobs::EventSink for Quiet {
        fn emit(&self, _: &paddock_jobs::JobSnapshot) {}
    }
    let ctx = paddock_jobs::JobRegistry::new(Arc::new(Quiet)).register("rescan-issues", "Reading system logs", None);
    ctx.cancel_token().cancel();
    ctx
}

#[test]
fn a_rescan_cancelled_before_it_waits_for_the_running_read_returns_cancelled_instead_of_blocking_on_itself() {
    let gate = Arc::new(Gate::default());
    let h = Harness::start(slow_issues(&gate));
    let shared = h.shared();
    let scheduled = {
        let s = shared.clone();
        std::thread::spawn(move || s.issues.scan(&s, None))
    };
    gate.wait_for_callers(1);
    let s = shared.clone();
    let out = within("a cancelled rescan", move || s.issues.scan(&s, Some(&cancelled_job())));
    assert!(matches!(out, Err(paddock_jobs::JobError::Cancelled(_))), "{out:?}");
    gate.open();
    assert!(scheduled.join().unwrap().unwrap());
}

#[test]
fn a_rescan_that_loses_the_race_to_the_scheduled_read_waits_for_it_instead_of_answering_at_once() {
    let gate = Arc::new(Gate::default());
    let h = Harness::start(slow_issues(&gate));
    let shared = h.shared();
    let scheduled: Arc<Mutex<Option<std::thread::JoinHandle<()>>>> = Arc::default();
    let (s, g, sched) = (shared.clone(), gate.clone(), scheduled.clone());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let ctx = cancelled_job_open();
        // The scheduled read starts after the first look found none running and before this rescan claimed it.
        let out = s.issues.scan_with(&s, Some(&ctx), &|| {
            let s2 = s.clone();
            *sched.lock().unwrap() = Some(std::thread::spawn(move || {
                let _ = s2.issues.scan(&s2, None);
            }));
            g.wait_for_callers(1);
        });
        let _ = tx.send(out.map_err(|e| e.to_string()));
    });
    assert!(
        rx.recv_timeout(Duration::from_millis(400)).is_err(),
        "the rescan answered while the scheduled read was still running"
    );
    gate.open();
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        Ok(false),
        "and answers once that read has ended"
    );
    scheduled.lock().unwrap().take().unwrap().join().unwrap();
}

/// A job that is not cancelled.
fn cancelled_job_open() -> paddock_jobs::JobContext {
    struct Quiet;
    impl paddock_jobs::EventSink for Quiet {
        fn emit(&self, _: &paddock_jobs::JobSnapshot) {}
    }
    paddock_jobs::JobRegistry::new(Arc::new(Quiet)).register("rescan-issues", "Reading system logs", None)
}

/// The thread name and QoS class of each read.
#[cfg(target_os = "macos")]
type Reads = Arc<Mutex<Vec<(Option<String>, u32)>>>;

/// Reads its own thread's QoS class: a source runs on the thread of the job that reads it.
#[cfg(target_os = "macos")]
struct ClassProbe(Reads);

#[cfg(target_os = "macos")]
impl IssueSource for ClassProbe {
    fn id(&self) -> IssueSourceId {
        IssueSourceId::MacReports
    }
    fn read(&self, _since: Timestamp, _until: Timestamp) -> Result<Vec<Issue>, PlatformError> {
        let thread = std::thread::current().name().map(str::to_string);
        self.0.lock().unwrap().push((thread, dyno_platform::priority::current_qos_class()));
        Ok(Vec::new())
    }
}

/// Every agent thread but the server runs at the gentle level (rule 52), the job pool's workers included: a job the person
/// started may wait behind a busy machine but is never starved, and is never ahead of the person's own work.
#[cfg(target_os = "macos")]
#[test]
fn a_job_runs_at_the_gentle_level() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let h = Harness::start(vec![Box::new(ClassProbe(seen.clone()))]);
    let peer = h.client();
    let job = start(
        &peer,
        JobSpec::RescanIssues {
            query: IssuesQuery { days: 14 },
        },
    )
    .unwrap();
    assert!(matches!(wait_final(&peer, job.id).state, WireState::Done), "the read ends");
    let seen = seen.lock().unwrap().clone();
    // The scheduled read runs on `dyno-issues`; the job's read is the one on any other thread.
    assert!(
        seen.iter().any(|(t, _)| t.as_deref() != Some("dyno-issues")),
        "the job read the source: {seen:?}"
    );
    assert!(seen.iter().all(|(_, c)| *c == dyno_platform::priority::UTILITY), "{seen:?}");
}
