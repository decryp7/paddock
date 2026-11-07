//! `stop_all` over a fake machine: the decisions, not the operating system.

use super::*;
use crate::StartTime;

const DIR: &str = r"C:\Program Files\Dyno";

fn id(pid: u32) -> ProcessId {
    ProcessId::new(pid, StartTime::from_micros(u64::from(pid) * 1000))
}

#[derive(Clone)]
struct Proc {
    id: ProcessId,
    session: u32,
    image: PathBuf,
    alive: bool,
    /// `TerminateProcess` has no effect.
    immune: bool,
    /// Opening it needs `SeDebugPrivilege` (every other user's process does).
    needs_privilege: bool,
    /// The terminate handle sees another start time (a pid handed on between the two opens).
    swapped_on_end: bool,
    /// Opening it is refused even with the privilege.
    denied: bool,
    /// The handle gives no image path.
    no_image: bool,
    /// `TerminateProcess` fails.
    terminate_fails: bool,
    /// The process table does not list it, but Restart Manager does.
    hidden_from_table: bool,
    /// Restart Manager lists it although its program is not one of the listed ones (a system component mapping the files).
    rm_lists_anyway: bool,
    /// The second open (to end) finds it gone.
    gone_on_end: bool,
    /// It has a window: a close request reaches something.
    has_window: bool,
    /// It closes by itself when asked.
    closes_on_request: bool,
    /// The second open (to end) is refused.
    denied_on_end: bool,
}

/// The file name of a Windows path (the host may not split on a backslash).
fn base_name(p: &Path) -> String {
    p.to_string_lossy()
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn proc(pid: u32, session: u32, image: &str) -> Proc {
    Proc {
        id: id(pid),
        session,
        image: PathBuf::from(image),
        alive: true,
        immune: false,
        needs_privilege: true,
        swapped_on_end: false,
        denied: false,
        no_image: false,
        terminate_fails: false,
        hidden_from_table: false,
        rm_lists_anyway: false,
        gone_on_end: false,
        has_window: false,
        closes_on_request: false,
        denied_on_end: false,
    }
}

struct FakeHeld {
    id: ProcessId,
    image: Option<PathBuf>,
    session: u32,
}

impl Held for FakeHeld {
    fn id(&self) -> ProcessId {
        self.id
    }
    fn image(&self) -> Option<&Path> {
        self.image.as_deref()
    }
    fn session(&self) -> Option<u32> {
        Some(self.session)
    }
}

#[derive(Default)]
struct Fake {
    procs: Vec<Proc>,
    rm_fails: bool,
    privilege_fails: bool,
    privilege_on: bool,
    events: Vec<String>,
    registered: Vec<Vec<PathBuf>>,
    wait_ok: bool,
    table_fails: bool,
    vanish_after_table: bool,
    /// The time a wait on a handle counts as having taken (the fake does not sleep).
    wait_elapses: Option<Duration>,
    waits: Vec<Duration>,
}

impl Fake {
    fn new(procs: Vec<Proc>) -> Fake {
        Fake {
            procs,
            wait_ok: true,
            ..Fake::default()
        }
    }
    fn alive(&self, pid: u32) -> bool {
        self.procs.iter().any(|p| p.id.pid() == pid && p.alive)
    }
    fn count(&self, prefix: &str) -> usize {
        self.events.iter().filter(|e| e.starts_with(prefix)).count()
    }
}

impl Machine for Fake {
    type Held = FakeHeld;

    fn rm_list(&mut self, files: &[PathBuf]) -> Result<Vec<RmProcess>, StopError> {
        self.events.push("rm_list".into());
        self.registered.push(files.to_vec());
        if self.rm_fails {
            return Err(StopError::RestartManager {
                call: "RmStartSession",
                code: 5,
            });
        }
        Ok(self
            .procs
            .iter()
            .filter(|p| {
                p.alive
                    && (p.rm_lists_anyway
                        || files.iter().any(|f| {
                            f.to_string_lossy()
                                .eq_ignore_ascii_case(&p.image.to_string_lossy())
                        }))
            })
            .map(|p| RmProcess {
                id: p.id,
                session: Some(p.session),
                name: base_name(&p.image),
                image: Some(base_name(&p.image)),
            })
            .collect())
    }

    fn processes(&mut self) -> Result<Vec<Candidate>, StopError> {
        self.events.push("processes".into());
        if self.table_fails {
            return Err(StopError::Table(5));
        }
        let table: Vec<Candidate> = self
            .procs
            .iter()
            .filter(|p| p.alive && !p.hidden_from_table)
            .map(|p| Candidate {
                pid: p.id.pid(),
                image: base_name(&p.image),
            })
            .collect();
        if self.vanish_after_table {
            for p in &mut self.procs {
                p.alive = false;
            }
        }
        Ok(table)
    }

    fn enable_privilege(&mut self) -> Result<(), StopError> {
        self.events.push("enable".into());
        if self.privilege_fails {
            return Err(StopError::Privilege(1300));
        }
        self.privilege_on = true;
        Ok(())
    }

    fn drop_privilege(&mut self) {
        self.events.push("drop".into());
        self.privilege_on = false;
    }

    fn open(&mut self, pid: u32, for_end: bool) -> Result<FakeHeld, OpenFailure> {
        self.events.push(format!(
            "open:{pid}:{}",
            if for_end { "end" } else { "query" }
        ));
        let p = self
            .procs
            .iter()
            .find(|p| p.id.pid() == pid && p.alive)
            .ok_or(OpenFailure::Gone)?;
        if for_end && p.gone_on_end {
            return Err(OpenFailure::Gone);
        }
        if for_end && p.denied_on_end {
            return Err(OpenFailure::Denied(5));
        }
        if (p.needs_privilege && !self.privilege_on) || p.denied {
            return Err(OpenFailure::Denied(5));
        }
        let mut id = p.id;
        if for_end && p.swapped_on_end {
            id = ProcessId::new(pid, StartTime::from_micros(1));
        }
        Ok(FakeHeld {
            id,
            image: (!p.no_image).then(|| p.image.clone()),
            session: p.session,
        })
    }

    fn request_close(&mut self, held: &FakeHeld) -> bool {
        self.events.push(format!("close:{}", held.id.pid()));
        let mut asked = false;
        for p in &mut self.procs {
            if p.id.pid() == held.id.pid() && p.has_window {
                asked = true;
                if p.closes_on_request {
                    p.alive = false;
                }
            }
        }
        asked
    }

    fn terminate(&mut self, held: &FakeHeld) -> Result<(), StopError> {
        self.events.push(format!("terminate:{}", held.id.pid()));
        assert!(
            self.privilege_on,
            "terminate runs with the privilege enabled"
        );
        if self
            .procs
            .iter()
            .any(|p| p.id.pid() == held.id.pid() && p.terminate_fails)
        {
            return Err(StopError::Terminate(5));
        }
        for p in &mut self.procs {
            if p.id.pid() == held.id.pid() && !p.immune {
                p.alive = false;
            }
        }
        Ok(())
    }

    fn wait_exit(&mut self, held: &FakeHeld, bound: Duration) -> (bool, Duration) {
        self.events.push(format!("wait:{}", held.id.pid()));
        self.waits.push(bound);
        (
            self.wait_ok && !self.alive(held.id.pid()),
            self.wait_elapses.unwrap_or_default(),
        )
    }
}

fn images() -> Vec<String> {
    vec![
        "Dyno.exe".into(),
        "dyno-agent.exe".into(),
        "paddock-setup.exe".into(),
    ]
}

/// A logger that accepts everything and prints nothing, so the log lines' arguments are evaluated like in production.
struct Quiet;
impl log::Log for Quiet {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        let _ = format!("{}", record.args());
    }
    fn flush(&self) {}
}

fn init_log() {
    static Q: Quiet = Quiet;
    if log::set_logger(&Q).is_ok() {
        log::set_max_level(log::LevelFilter::Trace);
    }
}

fn run(f: &mut Fake) -> StopReport {
    init_log();
    let imgs = images();
    let req = Request {
        dir: Path::new(DIR),
        images: &imgs,
        own: id(9000),
        own_image: None,
        own_session: None,
        spare: &[],
    };
    stop_all(f, &req, &Bounds::default())
}

fn dyno(pid: u32, session: u32) -> Proc {
    proc(pid, session, r"C:\Program Files\Dyno\dyno.exe")
}

#[test]
fn every_sessions_programs_are_ended_by_handle_whatever_restart_manager_does() {
    // Restart Manager's shutdown ends nothing while another session holds a file (measured), so it is not used: a failing
    // Restart Manager changes nothing about the result (PM-6).
    let mut f = Fake::new(vec![
        dyno(10, 1),
        dyno(11, 2),
        proc(12, 3, r"C:\Program Files\Dyno\dyno-agent.exe"),
    ]);
    f.rm_fails = true;
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 0, "{r:?}");
    assert_eq!(r.actions.len(), 3);
    assert!(
        r.actions
            .iter()
            .all(|a| a.outcome == Outcome::Ended && a.id.is_some()),
        "{r:?}"
    );
    assert_eq!(
        r.actions
            .iter()
            .filter_map(|a| a.session)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(matches!(
        r.restart_manager_error,
        Some(StopError::RestartManager { code: 5, .. })
    ));
    assert!((10..=12).all(|p| !f.alive(p)));
    assert!(f.events.iter().all(|e| e != "rm_shutdown"));
}

#[test]
fn the_check_registers_the_full_path_of_every_listed_image() {
    let mut f = Fake::new(vec![]);
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 0);
    let files = &f.registered[0];
    assert_eq!(files.len(), 3);
    assert!(
        files
            .iter()
            .any(|p| p == Path::new(r"C:\Program Files\Dyno\Dyno.exe"))
    );
    assert_eq!(
        f.count("enable"),
        1,
        "only the check needs the privilege when there are no candidates"
    );
}

#[test]
fn the_privilege_is_enabled_before_the_first_open_and_dropped_after_each_entry() {
    // Every process here needs the privilege even to be queried, as another user's process does.
    let mut f = Fake::new(vec![dyno(11, 2), dyno(12, 3)]);
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 0, "{r:?}");
    let ev: Vec<&str> = f.events.iter().map(String::as_str).collect();
    let mut depth = 0;
    for e in &ev {
        match *e {
            "enable" => depth += 1,
            "drop" => depth -= 1,
            e if e.starts_with("open") || e.starts_with("terminate") => {
                assert_eq!(depth, 1, "{e} outside the privilege: {ev:?}")
            }
            _ => {}
        }
        assert!((0..=1).contains(&depth), "unbalanced at {e}: {ev:?}");
    }
    assert_eq!(depth, 0);
    assert_eq!(
        (f.count("enable"), f.count("drop")),
        (3, 3),
        "two entries and the check"
    );
    assert!(!f.privilege_on);
}

#[test]
fn a_program_of_the_same_name_in_a_folder_that_starts_with_the_install_folder_is_left_alone() {
    let helper = proc(20, 1, r"C:\Program Files\Dyno Helper\dyno.exe");
    let mut f = Fake::new(vec![helper]);
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 0, "{r:?}");
    assert_eq!(f.count("terminate"), 0);
    assert!(f.alive(20));
    assert!(
        matches!(&r.actions[0].outcome, Outcome::Skipped(why) if why.contains("outside")),
        "{r:?}"
    );
    assert_eq!(f.count("enable"), f.count("drop"));
}

#[test]
fn the_folder_is_compared_by_components_and_without_case() {
    assert!(is_directly_in(
        Path::new(r"c:\PROGRAM FILES\dyno\Dyno.EXE"),
        Path::new(DIR)
    ));
    assert!(is_directly_in(
        Path::new(r"\\?\C:\Program Files\Dyno\a.exe"),
        Path::new(r"C:\Program Files\Dyno\")
    ));
    assert!(!is_directly_in(
        Path::new(r"C:\Program Files\Dyno Helper\a.exe"),
        Path::new(DIR)
    ));
    assert!(!is_directly_in(
        Path::new(r"C:\Program Files\Dyno\sub\a.exe"),
        Path::new(DIR)
    ));
    assert!(!is_directly_in(
        Path::new(r"C:\Program Files\Dyno"),
        Path::new(DIR)
    ));
    assert!(!is_directly_in(Path::new("a.exe"), Path::new("")));
}

#[test]
fn the_process_never_ends_itself() {
    let me = proc(9000, 1, r"C:\Program Files\Dyno\paddock-setup.exe");
    let mut f = Fake::new(vec![dyno(10, 1), me]);
    let r = run(&mut f);
    assert!(f.alive(9000), "own process survived");
    assert_eq!(f.count("terminate:9000"), 0);
    assert_eq!(f.count("open:9000"), 0);
    assert_eq!(r.exit_code(), 0, "the check ignores this process: {r:?}");
}

#[test]
fn the_own_image_is_left_out_of_the_registration() {
    let mut f = Fake::new(vec![]);
    let imgs = images();
    let own_image = PathBuf::from(r"C:\Program Files\Dyno\paddock-setup.exe");
    let req = Request {
        dir: Path::new(DIR),
        images: &imgs,
        own: id(9000),
        own_image: Some(&own_image),
        own_session: None,
        spare: &[],
    };
    stop_all(&mut f, &req, &Bounds::default());
    assert_eq!(f.registered[0].len(), 2);
    assert!(f.registered[0].iter().all(|p| {
        !p.to_string_lossy()
            .to_lowercase()
            .ends_with("paddock-setup.exe")
    }));
}

#[test]
fn a_privilege_that_cannot_be_enabled_is_reported_with_what_still_runs() {
    let mut f = Fake::new(vec![dyno(10, 1), dyno(11, 2)]);
    f.privilege_fails = true;
    let r = run(&mut f);
    assert_eq!(r.exit_code(), EXIT_STILL_RUNNING);
    assert_eq!(r.still_running, vec!["dyno.exe".to_string()], "named once");
    assert_eq!(r.privilege_error, Some(StopError::Privilege(1300)));
    assert_eq!(f.count("terminate"), 0);
    assert_eq!(
        f.count("enable"),
        1,
        "no retry per entry once it is known to fail"
    );
}

#[test]
fn a_process_that_survives_is_named_and_exits_11() {
    let mut p = dyno(10, 1);
    p.immune = true;
    let mut f = Fake::new(vec![p]);
    f.wait_ok = false;
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 11);
    assert_eq!(r.still_running, vec!["dyno.exe".to_string()]);
    assert!(matches!(
        r.actions.last().unwrap().outcome,
        Outcome::Failed(_)
    ));
    assert!(!f.privilege_on, "dropped even on failure");
}

#[test]
fn a_program_the_table_missed_but_restart_manager_lists_is_still_running() {
    let mut p = dyno(10, 4);
    p.hidden_from_table = true;
    let mut f = Fake::new(vec![p]);
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 11, "{r:?}");
    assert_eq!(r.still_running, vec!["dyno.exe".to_string()]);
}

#[test]
fn a_pid_handed_on_between_the_two_opens_is_not_terminated() {
    let mut p = dyno(10, 1);
    p.swapped_on_end = true;
    let mut f = Fake::new(vec![p]);
    let r = run(&mut f);
    assert_eq!(f.count("terminate"), 0);
    assert!(
        matches!(&r.actions[0].outcome, Outcome::Skipped(why) if why.contains("identity")),
        "{r:?}"
    );
    assert!(!f.privilege_on);
}

#[test]
fn a_program_that_is_not_listed_is_never_touched() {
    let other = proc(30, 1, r"C:\Program Files\Dyno\notepad.exe");
    let mut f = Fake::new(vec![other]);
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 0);
    assert_eq!(f.count("open"), 0);
    assert!(r.actions.is_empty());
}

#[test]
fn a_process_that_exits_before_it_is_opened_is_left_out_not_failed() {
    let mut f = Fake::new(vec![dyno(10, 1)]);
    f.vanish_after_table = true;
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 0, "{r:?}");
    assert!(
        matches!(&r.actions[0].outcome, Outcome::Skipped(w) if w.contains("exited")),
        "{r:?}"
    );
    assert_eq!(f.count("terminate"), 0);
    assert!(!f.privilege_on);
}

#[test]
fn an_open_the_system_refuses_is_a_failure_that_names_the_program() {
    let mut p = dyno(10, 1);
    p.denied = true;
    let mut f = Fake::new(vec![p]);
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 11);
    assert!(
        matches!(&r.actions[0].outcome, Outcome::Failed(w) if w.contains("opened")),
        "{r:?}"
    );
    assert!(!f.privilege_on);
}

#[test]
fn a_handle_without_an_image_path_is_a_failure_not_a_guess() {
    let mut p = dyno(10, 1);
    p.no_image = true;
    let mut f = Fake::new(vec![p]);
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 11);
    assert!(
        matches!(&r.actions[0].outcome, Outcome::Failed(w) if w.contains("image path")),
        "{r:?}"
    );
    assert_eq!(f.count("terminate"), 0);
}

#[test]
fn a_terminate_the_system_refuses_is_a_failure() {
    let mut p = dyno(10, 1);
    p.terminate_fails = true;
    let mut f = Fake::new(vec![p]);
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 11);
    assert!(
        matches!(&r.actions[0].outcome, Outcome::Failed(w) if w.contains("TerminateProcess")),
        "{r:?}"
    );
}

#[test]
fn an_unreadable_process_table_still_reports_what_restart_manager_sees() {
    let mut f = Fake::new(vec![dyno(10, 1)]);
    f.table_fails = true;
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 11, "{r:?}");
    assert_eq!(r.still_running, vec!["dyno.exe".to_string()]);
}

#[test]
fn the_end_wait_is_bounded_by_what_is_left_of_the_total() {
    let mut f = Fake::new(vec![dyno(10, 1), dyno(11, 2)]);
    let imgs = images();
    let req = Request {
        dir: Path::new(DIR),
        images: &imgs,
        own: id(9000),
        own_image: None,
        own_session: None,
        spare: &[],
    };
    let bounds = Bounds {
        end_each: Duration::from_secs(5),
        end_total: Duration::ZERO,
        ..Bounds::default()
    };
    let r = stop_all(&mut f, &req, &bounds);
    assert_eq!(r.exit_code(), 0, "{r:?}");
    assert_eq!(Bounds::default().end_total, Duration::from_secs(20));
}

#[test]
fn errors_are_described_for_the_log() {
    for e in [
        StopError::RestartManager {
            call: "RmStartSession",
            code: 5,
        },
        StopError::Table(5),
        StopError::Privilege(1300),
        StopError::Terminate(5),
    ] {
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn a_system_component_that_only_maps_the_files_does_not_block() {
    // On Arm Windows the x86 emulation cache service is listed for every x64 program file.
    let mut cache = proc(50, 0, r"C:\Windows\System32\XtaCache.exe");
    cache.rm_lists_anyway = true;
    let mut f = Fake::new(vec![dyno(10, 1), cache]);
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 0, "{r:?}");
    assert!(f.alive(50), "not ours to end");
    assert_eq!(f.count("terminate:50"), 0);
}

#[test]
fn the_check_runs_with_the_privilege_and_drops_it() {
    let mut f = Fake::new(vec![]);
    run(&mut f);
    let ev: Vec<&str> = f.events.iter().map(String::as_str).collect();
    assert_eq!(ev, vec!["processes", "enable", "rm_list", "drop"]);
}

#[test]
fn a_process_gone_or_refused_at_the_second_open_is_handled() {
    let mut a = dyno(10, 1);
    a.gone_on_end = true;
    let mut b = dyno(11, 1);
    b.denied_on_end = true;
    let mut f = Fake::new(vec![a, b]);
    let r = run(&mut f);
    assert!(
        matches!(&r.actions[0].outcome, Outcome::Skipped(w) if w.contains("exited")),
        "{r:?}"
    );
    assert!(
        matches!(&r.actions[1].outcome, Outcome::Failed(w) if w.contains("to end")),
        "{r:?}"
    );
    assert_eq!(f.count("terminate"), 0);
    assert!(!f.privilege_on);
}

fn run_in_session(f: &mut Fake, own_session: u32) -> StopReport {
    init_log();
    let imgs = images();
    let req = Request {
        dir: Path::new(DIR),
        images: &imgs,
        own: id(9000),
        own_image: None,
        own_session: Some(own_session),
        spare: &[],
    };
    stop_all(f, &req, &Bounds::default())
}

#[test]
fn a_program_in_the_callers_session_that_closes_when_asked_is_not_terminated() {
    let mut p = dyno(10, 1);
    p.has_window = true;
    p.closes_on_request = true;
    let mut f = Fake::new(vec![p]);
    let r = run_in_session(&mut f, 1);
    assert_eq!(r.exit_code(), 0, "{r:?}");
    assert!(
        r.actions[0].graceful && r.actions[0].outcome == Outcome::Ended,
        "{r:?}"
    );
    assert_eq!(f.count("terminate"), 0, "{:?}", f.events);
}

#[test]
fn a_program_that_ignores_the_request_is_terminated_after_the_wait() {
    let mut p = dyno(10, 1);
    p.has_window = true;
    let mut f = Fake::new(vec![p]);
    let r = run_in_session(&mut f, 1);
    assert_eq!(r.exit_code(), 0, "{r:?}");
    assert!(!r.actions[0].graceful, "{r:?}");
    let close = f
        .events
        .iter()
        .position(|e| e == "close:10")
        .expect("asked first");
    let wait = f
        .events
        .iter()
        .position(|e| e == "wait:10")
        .expect("waited");
    let term = f
        .events
        .iter()
        .position(|e| e == "terminate:10")
        .expect("then forced");
    assert!(close < wait && wait < term, "{:?}", f.events);
}

#[test]
fn another_sessions_program_is_never_asked() {
    let mut p = dyno(10, 2);
    p.has_window = true;
    let mut f = Fake::new(vec![p]);
    let r = run_in_session(&mut f, 1);
    assert_eq!(r.exit_code(), 0, "{r:?}");
    assert_eq!(f.count("close"), 0, "{:?}", f.events);
    assert_eq!(f.count("terminate:10"), 1);
}

#[test]
fn a_program_without_a_window_is_not_waited_for_before_the_forced_end() {
    let mut f = Fake::new(vec![dyno(10, 1)]);
    let r = run_in_session(&mut f, 1);
    assert_eq!(r.exit_code(), 0, "{r:?}");
    let term = f.events.iter().position(|e| e == "terminate:10").unwrap();
    assert!(
        f.events[..term].iter().all(|e| !e.starts_with("wait")),
        "{:?}",
        f.events
    );
}

#[test]
fn a_verbatim_folder_is_registered_with_ordinary_paths() {
    let mut f = Fake::new(vec![]);
    let imgs = images();
    let req = Request {
        dir: Path::new(r"\\?\C:\Program Files\Dyno\"),
        images: &imgs,
        own: id(9000),
        own_image: None,
        own_session: None,
        spare: &[],
    };
    stop_all(&mut f, &req, &Bounds::default());
    assert!(
        f.registered[0]
            .iter()
            .any(|p| p == Path::new(r"C:\Program Files\Dyno\Dyno.exe")),
        "{:?}",
        f.registered
    );
}

#[test]
fn the_process_that_started_the_installer_is_never_ended() {
    // apply-update (elevated, in the install folder) started the installer and may still be exiting: it is an ancestor.
    let parent = proc(500, 1, r"C:\Program Files\Dyno\paddock-setup.exe");
    let mut f = Fake::new(vec![parent, dyno(10, 1)]);
    init_log();
    let imgs = images();
    let req = Request {
        dir: Path::new(DIR),
        images: &imgs,
        own: id(9000),
        own_image: None,
        own_session: None,
        spare: &[500],
    };
    let r = stop_all(&mut f, &req, &Bounds::default());
    assert!(
        f.alive(500),
        "the hand-off keeps running and exits with its own code"
    );
    assert_eq!(f.count("terminate:500"), 0);
    assert_eq!(f.count("open:500"), 0, "not even opened: {:?}", f.events);
    assert!(!f.alive(10), "everything else is still ended");
    assert_eq!(
        r.exit_code(),
        0,
        "the spared parent does not count as running: {r:?}"
    );
}

#[test]
fn the_graceful_wait_counts_toward_the_total_and_runs_without_the_privilege() {
    let mut p = dyno(10, 1);
    p.has_window = true;
    let mut q = dyno(11, 1);
    q.has_window = true;
    let mut f = Fake::new(vec![p, q]);
    init_log();
    let imgs = images();
    let req = Request {
        dir: Path::new(DIR),
        images: &imgs,
        own: id(9000),
        own_image: None,
        own_session: Some(1),
        spare: &[],
    };
    let bounds = Bounds {
        graceful: Duration::from_secs(3),
        end_each: Duration::from_secs(5),
        end_total: Duration::from_secs(4),
    };
    f.wait_elapses = Some(Duration::from_secs(3));
    stop_all(&mut f, &req, &bounds);
    // Order of waits: graceful 10, forced 10, graceful 11, forced 11. The first graceful wait took 3 s of the 4 and the
    // forced wait that follows is cut to the 1 s left; by then the total is used up, so the second graceful wait gets nothing
    // and each forced end still gets one second to show.
    let secs = |n| Duration::from_secs(n);
    assert_eq!(
        f.waits,
        vec![secs(3), secs(1), Duration::ZERO, secs(1)],
        "{:?}",
        f.events
    );
    let w = f.events.iter().position(|e| e == "wait:10").unwrap();
    let before: Vec<&str> = f.events[..w].iter().map(String::as_str).collect();
    assert_eq!(
        before.iter().filter(|e| **e == "enable").count(),
        before.iter().filter(|e| **e == "drop").count(),
        "no privilege held while waiting: {before:?}"
    );
}
