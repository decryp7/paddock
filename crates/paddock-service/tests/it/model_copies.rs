//! Two installed copies of one app on macOS (ADR 0056 section 8, WP 6): the real [`Manager`] and the real
//! `LaunchdBundled` backend over one simulated launchd and `SMAppService`, one record folder, two programs. Before the
//! copy's identity was known (`points_here` and `program_exists` were `None`), each copy replaced the other's registration
//! at every start, for ever; a newer copy's registration must stay with that copy.

use paddock_service::backend::LaunchdBundled;
use paddock_service::port::{
    AppService, AppServiceStatus, Output, RegisterError, RunError, System,
};
use paddock_service::record::RecordFile;
use paddock_service::{Hold, Manager, ServiceSpec, Started, Timings};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const LABEL: &str = "net.model.copies";
const OLD_COPY: &str = "/Applications/Model.app/Contents/MacOS/model-app";
const NEW_COPY: &str = "/Users/u/Desktop/Model.app/Contents/MacOS/model-app";

/// What launchd and Background Task Management hold: one registration, made by one copy's program.
#[derive(Default)]
struct Os {
    registered_by: Option<String>,
    registers: Vec<String>,
}

type Shared = Arc<Mutex<Os>>;

struct Launchd(Shared);

impl System for Launchd {
    fn run(&mut self, _program: &str, args: &[&str]) -> Result<Output, RunError> {
        let os = self.0.lock().unwrap();
        Ok(match args.first().copied() {
            Some("print") if os.registered_by.is_some() => {
                Output::ok("x = {\n\tstate = running\n\tpid = 7\n}\n")
            }
            Some("print") => Output {
                code: Some(113),
                stderr: "Could not find service".into(),
                ..Output::default()
            },
            Some("bootout") => {
                drop(os);
                self.0.lock().unwrap().registered_by = None;
                Output::ok("")
            }
            _ => Output::ok(""),
        })
    }
    fn process(&mut self, _pid: u32) -> Option<paddock_proc::ProcessId> {
        // The fake machine's pids are not the host's: no process is looked up on the machine the test runs on (ER-14).
        None
    }
    fn spawn(&mut self, _p: &Path, _a: &[String]) -> Result<(), String> {
        Ok(())
    }
    fn read(&mut self, _p: &Path) -> std::io::Result<String> {
        Err(std::io::ErrorKind::NotFound.into())
    }
    fn write(&mut self, _p: &Path, _t: &str) -> std::io::Result<()> {
        Ok(())
    }
    fn remove(&mut self, _p: &Path) -> std::io::Result<()> {
        Ok(())
    }
    /// Both copies are installed and stay installed.
    fn is_program(&mut self, _p: &Path) -> bool {
        true
    }
    fn pause(&mut self, _d: Duration) {}
}

struct SmAppService {
    os: Shared,
    own: &'static str,
}

impl AppService for SmAppService {
    fn status(&mut self) -> AppServiceStatus {
        if self.os.lock().unwrap().registered_by.is_some() {
            AppServiceStatus::Enabled
        } else {
            AppServiceStatus::NotRegistered
        }
    }
    fn register(&mut self) -> Result<(), RegisterError> {
        let mut os = self.os.lock().unwrap();
        os.registered_by = Some(self.own.to_string());
        os.registers.push(self.own.to_string());
        Ok(())
    }
    fn unregister(&mut self) -> Result<(), String> {
        self.os.lock().unwrap().registered_by = None;
        Ok(())
    }
    fn open_login_items(&mut self) {}
}

fn quick() -> Timings {
    Timings {
        max_cycles: 1,
        settle: Duration::ZERO,
        gone_wait: Duration::from_millis(5),
        gone_poll: Duration::from_millis(1),
        start_wait: Duration::from_millis(50),
        start_poll: Duration::from_millis(1),
        backoff: Duration::ZERO,
    }
}

/// One installed copy: its version and program, the shared OS and the shared record.
fn copy(
    os: &Shared,
    record: &RecordFile,
    dir: &Path,
    version: &str,
    program: &'static str,
) -> Manager {
    let spec = ServiceSpec::builder(LABEL, "Model", "model-agent")
        .build()
        .unwrap();
    let backend = LaunchdBundled::new(
        spec,
        501,
        Box::new(SmAppService {
            os: os.clone(),
            own: program,
        }),
        Box::new(Launchd(os.clone())),
    );
    let answers = os.clone();
    Manager::new("Model", Box::new(backend), dir, LABEL, version, move || {
        answers.lock().unwrap().registered_by.is_some()
    })
    .with_record(record.reopen())
    .with_app_state(|| Some(true), || true, || true)
    .with_timings(quick())
    .with_program(program)
}

fn start(m: &mut Manager) -> Started {
    m.on_start(&AtomicBool::new(false))
}

#[test]
fn two_copies_do_not_replace_each_others_registration_at_every_start() {
    let dir = tempfile::tempdir().unwrap();
    let os: Shared = Arc::default();
    let record = RecordFile::memory();
    let mut older = copy(&os, &record, dir.path(), "1.1.0", OLD_COPY);
    let mut newer = copy(&os, &record, dir.path(), "1.2.0", NEW_COPY);

    // The newer copy registers first; the older one, started afterwards and again and again, leaves it alone.
    assert!(
        matches!(start(&mut newer), Started::Repaired(..)),
        "the newer copy registers"
    );
    for round in 1..=4 {
        let seen = start(&mut older);
        assert!(
            matches!(&seen, Started::Left(Hold::NewerOwner(v)) if v == "1.2.0"),
            "round {round}: the older copy must leave the newer copy's registration alone, got {seen:?}"
        );
        let again = start(&mut newer);
        assert!(
            !matches!(again, Started::Repaired(..)),
            "round {round}: the newer copy leaves its own registration alone, got {again:?}"
        );
    }
    let os = os.lock().unwrap();
    assert_eq!(
        os.registers,
        vec![NEW_COPY.to_string()],
        "one register, by the newer copy"
    );
    assert_eq!(os.registered_by.as_deref(), Some(NEW_COPY));
}

#[test]
fn a_newer_copy_takes_over_once_and_the_older_one_then_stays_out() {
    let dir = tempfile::tempdir().unwrap();
    let os: Shared = Arc::default();
    let record = RecordFile::memory();
    let mut older = copy(&os, &record, dir.path(), "1.1.0", OLD_COPY);
    let mut newer = copy(&os, &record, dir.path(), "1.2.0", NEW_COPY);

    assert!(matches!(start(&mut older), Started::Repaired(..)));
    for _ in 0..3 {
        start(&mut newer);
        start(&mut older);
    }
    let os = os.lock().unwrap();
    assert_eq!(
        os.registers,
        vec![OLD_COPY.to_string(), NEW_COPY.to_string()],
        "the older copy, then the newer once"
    );
    assert_eq!(os.registered_by.as_deref(), Some(NEW_COPY));
}

#[test]
fn a_registration_whose_program_is_gone_is_taken_over_even_by_an_older_copy() {
    struct Gone(Launchd);
    impl System for Gone {
        fn run(&mut self, p: &str, a: &[&str]) -> Result<Output, RunError> {
            self.0.run(p, a)
        }
        fn spawn(&mut self, p: &Path, a: &[String]) -> Result<(), String> {
            self.0.spawn(p, a)
        }
        fn read(&mut self, p: &Path) -> std::io::Result<String> {
            self.0.read(p)
        }
        fn write(&mut self, p: &Path, t: &str) -> std::io::Result<()> {
            self.0.write(p, t)
        }
        fn remove(&mut self, p: &Path) -> std::io::Result<()> {
            self.0.remove(p)
        }
        fn is_program(&mut self, p: &Path) -> bool {
            p != Path::new(NEW_COPY)
        }
        fn pause(&mut self, d: Duration) {
            self.0.pause(d);
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let os: Shared = Arc::default();
    let record = RecordFile::memory();
    let mut newer = copy(&os, &record, dir.path(), "1.2.0", NEW_COPY);
    assert!(matches!(start(&mut newer), Started::Repaired(..)));
    // The newer copy was deleted: the older one's look finds its program missing.
    let spec = ServiceSpec::builder(LABEL, "Model", "model-agent")
        .build()
        .unwrap();
    let backend = LaunchdBundled::new(
        spec,
        501,
        Box::new(SmAppService {
            os: os.clone(),
            own: OLD_COPY,
        }),
        Box::new(Gone(Launchd(os.clone()))),
    );
    let answers = os.clone();
    let mut older = Manager::new(
        "Model",
        Box::new(backend),
        dir.path(),
        LABEL,
        "1.1.0",
        move || answers.lock().unwrap().registered_by.is_some(),
    )
    .with_record(record.reopen())
    .with_app_state(|| Some(true), || true, || true)
    .with_timings(quick())
    .with_program(OLD_COPY);
    assert!(matches!(start(&mut older), Started::Repaired(..)));
    assert_eq!(os.lock().unwrap().registered_by.as_deref(), Some(OLD_COPY));
}
