#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Long operations as jobs, end to end over the fake platform and a real IPC connection: a start answers at once, the
//! job reports phases that only grow, Cancel stops it and leaves the state as it was, and a limit is named. Gates stand
//! in for slow sources, so nothing here waits on a clock. Files are created only in temp folders.

use dyno_agent::cli::{ask, connect, run_job};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::extras::files::{FinderMode, FinderQuery};
use dyno_core::issues::{Issue, IssueKind, IssueSource, IssueSourceId, IssuesQuery};
use dyno_core::jobs::{JobOutput, JobSpec, JobState, JobStatus};
use dyno_core::ports::PlatformError;
use dyno_core::protocol::{ErrorCode, Message, Reply, Request};
use dyno_core::time::Timestamp;
use dyno_ipc::Peer;
use dyno_platform::fake::{process, Fake};
use dyno_platform::issues::ToolRunner;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// A door a test opens: sources wait at it until then.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
    waiting: Mutex<u32>,
}

impl Gate {
    fn pass(&self) {
        *self.waiting.lock().unwrap() += 1;
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.cv.wait(open).unwrap();
        }
    }

    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.cv.notify_all();
    }

    /// Returns when `n` callers have reached the gate.
    fn wait_for_callers(&self, n: u32) {
        while *self.waiting.lock().unwrap() < n {
            std::thread::yield_now();
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

struct SlowTool(Arc<Gate>);

impl ToolRunner for SlowTool {
    fn run(&self, _program: &str, _args: &[String], _timeout: Duration) -> Result<String, PlatformError> {
        self.0.pass();
        Err(PlatformError::Unsupported("scripted".into()))
    }
}

struct Harness {
    running: Option<Running>,
    dirs: Dirs,
    _tmp: tempfile::TempDir,
}

impl Harness {
    fn start(sources: Vec<Box<dyn IssueSource>>) -> Harness {
        let tmp = tempfile::tempdir().unwrap();
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

    fn client(&self) -> Peer<Message> {
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

fn start(peer: &Peer<Message>, spec: JobSpec) -> Result<JobStatus, String> {
    match ask(peer, Request::StartJob { spec })? {
        Reply::Job(s) => Ok(*s),
        other => panic!("{other:?}"),
    }
}

fn status(peer: &Peer<Message>, id: u64) -> JobStatus {
    let Reply::Jobs(all) = ask(peer, Request::Jobs).unwrap() else {
        panic!("jobs")
    };
    all.into_iter().find(|s| s.id == id).expect("the job is listed")
}

fn wait_final(peer: &Peer<Message>, id: u64) -> JobStatus {
    for _ in 0..2000 {
        let s = status(peer, id);
        if s.state.is_final() {
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
    assert!(!first.state.is_final(), "the start does not wait for the logs");
    gate.wait_for_callers(1);
    let running = status(&peer, first.id);
    let JobState::Running { phase: Some(p) } = running.state else {
        panic!("{running:?}")
    };
    assert_eq!((p.id.as_str(), p.done, p.total), ("read", 0, Some(2)));
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
    assert_eq!(done.state, JobState::Done);
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
    assert_eq!(end.state, JobState::Cancelled);
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
fn cancelling_a_backup_check_between_tools_keeps_the_report_and_clears_checking() {
    let gate = Arc::new(Gate::default());
    let h = Harness::start(vec![]);
    let s = h.shared();
    s.extras.use_os("macos");
    s.extras.use_backup_tools(Arc::new(SlowTool(gate.clone())));
    let peer = h.client();
    let job = start(&peer, JobSpec::RefreshBackup).unwrap();
    gate.wait_for_callers(1);
    let JobState::Running { phase: Some(p) } = status(&peer, job.id).state else {
        panic!("running")
    };
    assert_eq!(p.id, "tools");
    assert_eq!(p.done, 0);
    assert!(p.total.is_some_and(|t| t >= 2), "more than one tool is checked: {p:?}");
    ask(&peer, Request::CancelJob { id: job.id }).unwrap();
    gate.open();
    let end = wait_final(&peer, job.id);
    assert_eq!(end.state, JobState::Cancelled);
    assert!(s.extras.backup_report().checked_at.is_none(), "the report was not replaced");
    assert!(!s.extras.backup_report().checking, "checking came down");
    // A later check runs to the end.
    let JobOutput::Backup(r) = run_job(&peer, JobSpec::RefreshBackup, Duration::from_secs(30)).unwrap() else {
        panic!("backup")
    };
    assert!(r.checked_at.is_some() && !r.checking);
}

#[test]
fn a_limit_is_named_in_the_result_and_a_bad_folder_fails_with_what_to_fix() {
    let h = Harness::start(vec![]);
    let peer = h.client();
    let tmp = tempfile::tempdir().unwrap();
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
    let tmp = tempfile::tempdir().unwrap();
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
    assert_eq!(all.len(), dyno_core::jobs::KEPT_FINISHED);
    assert!(all.windows(2).all(|w| w[0].id < w[1].id && w[0].seq >= 1));
    let err = peer
        .call(Message::Request(Box::new(Request::JobResult { id: 9999 })), Duration::from_secs(5))
        .unwrap();
    assert!(matches!(err, Message::Reply(r) if matches!(*r, Err(ref e) if e.code == ErrorCode::NotFound)));
    // Cancelling an unknown or finished job is harmless.
    assert!(matches!(ask(&peer, Request::CancelJob { id: 9999 }).unwrap(), Reply::Ok));
}
