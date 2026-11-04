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
}

/// The file name of a Windows path (the host may not split on a backslash).
fn base_name(p: &Path) -> String {
    p.to_string_lossy().rsplit(['\\', '/']).next().unwrap_or_default().to_owned()
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
}

impl Fake {
    fn new(procs: Vec<Proc>) -> Fake {
        Fake { procs, wait_ok: true, ..Fake::default() }
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
            return Err(StopError::RestartManager { call: "RmStartSession", code: 5 });
        }
        Ok(self
            .procs
            .iter()
            .filter(|p| p.alive && (p.rm_lists_anyway || files.iter().any(|f| f.to_string_lossy().eq_ignore_ascii_case(&p.image.to_string_lossy()))))
            .map(|p| RmProcess { id: p.id, session: Some(p.session), name: base_name(&p.image), image: Some(base_name(&p.image)) })
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
            .map(|p| Candidate { pid: p.id.pid(), image: base_name(&p.image) })
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
        self.events.push(format!("open:{pid}:{}", if for_end { "end" } else { "query" }));
        let p = self.procs.iter().find(|p| p.id.pid() == pid && p.alive).ok_or(OpenFailure::Gone)?;
        if (p.needs_privilege && !self.privilege_on) || p.denied {
            return Err(OpenFailure::Denied(5));
        }
        let mut id = p.id;
        if for_end && p.swapped_on_end {
            id = ProcessId::new(pid, StartTime::from_micros(1));
        }
        Ok(FakeHeld { id, image: (!p.no_image).then(|| p.image.clone()), session: p.session })
    }

    fn terminate(&mut self, held: &FakeHeld) -> Result<(), StopError> {
        self.events.push(format!("terminate:{}", held.id.pid()));
        assert!(self.privilege_on, "terminate runs with the privilege enabled");
        if self.procs.iter().any(|p| p.id.pid() == held.id.pid() && p.terminate_fails) {
            return Err(StopError::Terminate(5));
        }
        for p in &mut self.procs {
            if p.id.pid() == held.id.pid() && !p.immune {
                p.alive = false;
            }
        }
        Ok(())
    }

    fn wait_exit(&mut self, held: &FakeHeld, _bound: Duration) -> bool {
        self.events.push(format!("wait:{}", held.id.pid()));
        self.wait_ok && !self.alive(held.id.pid())
    }
}

fn images() -> Vec<String> {
    vec!["Dyno.exe".into(), "dyno-agent.exe".into(), "paddock-setup.exe".into()]
}

fn run(f: &mut Fake) -> StopReport {
    let imgs = images();
    let req = Request { dir: Path::new(DIR), images: &imgs, own: id(9000), own_image: None };
    stop_all(f, &req, &Bounds::default())
}

fn dyno(pid: u32, session: u32) -> Proc {
    proc(pid, session, r"C:\Program Files\Dyno\dyno.exe")
}

#[test]
fn every_sessions_programs_are_ended_by_handle_whatever_restart_manager_does() {
    // Restart Manager's shutdown ends nothing while another session holds a file (measured), so it is not used: a failing
    // Restart Manager changes nothing about the result (PM-6).
    let mut f = Fake::new(vec![dyno(10, 1), dyno(11, 2), proc(12, 3, r"C:\Program Files\Dyno\dyno-agent.exe")]);
    f.rm_fails = true;
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 0, "{r:?}");
    assert_eq!(r.actions.len(), 3);
    assert!(r.actions.iter().all(|a| a.outcome == Outcome::Ended && a.id.is_some()), "{r:?}");
    assert_eq!(r.actions.iter().filter_map(|a| a.session).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert!(matches!(r.restart_manager_error, Some(StopError::RestartManager { code: 5, .. })));
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
    assert!(files.iter().any(|p| p == Path::new(r"C:\Program Files\Dyno\Dyno.exe")));
    assert_eq!(f.count("enable"), 1, "only the check needs the privilege when there are no candidates");
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
            e if e.starts_with("open") || e.starts_with("terminate") => assert_eq!(depth, 1, "{e} outside the privilege: {ev:?}"),
            _ => {}
        }
        assert!((0..=1).contains(&depth), "unbalanced at {e}: {ev:?}");
    }
    assert_eq!(depth, 0);
    assert_eq!((f.count("enable"), f.count("drop")), (3, 3), "two entries and the check");
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
    assert!(matches!(&r.actions[0].outcome, Outcome::Skipped(why) if why.contains("outside")), "{r:?}");
    assert_eq!(f.count("enable"), f.count("drop"));
}

#[test]
fn the_folder_is_compared_by_components_and_without_case() {
    assert!(is_directly_in(Path::new(r"c:\PROGRAM FILES\dyno\Dyno.EXE"), Path::new(DIR)));
    assert!(is_directly_in(Path::new(r"\\?\C:\Program Files\Dyno\a.exe"), Path::new(r"C:\Program Files\Dyno\")));
    assert!(!is_directly_in(Path::new(r"C:\Program Files\Dyno Helper\a.exe"), Path::new(DIR)));
    assert!(!is_directly_in(Path::new(r"C:\Program Files\Dyno\sub\a.exe"), Path::new(DIR)));
    assert!(!is_directly_in(Path::new(r"C:\Program Files\Dyno"), Path::new(DIR)));
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
    let req = Request { dir: Path::new(DIR), images: &imgs, own: id(9000), own_image: Some(&own_image) };
    stop_all(&mut f, &req, &Bounds::default());
    assert_eq!(f.registered[0].len(), 2);
    assert!(f.registered[0].iter().all(|p| !p.to_string_lossy().to_lowercase().ends_with("paddock-setup.exe")));
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
    assert_eq!(f.count("enable"), 1, "no retry per entry once it is known to fail");
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
    assert!(matches!(r.actions.last().unwrap().outcome, Outcome::Failed(_)));
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
    assert!(matches!(&r.actions[0].outcome, Outcome::Skipped(why) if why.contains("identity")), "{r:?}");
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
    assert!(matches!(&r.actions[0].outcome, Outcome::Skipped(w) if w.contains("exited")), "{r:?}");
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
    assert!(matches!(&r.actions[0].outcome, Outcome::Failed(w) if w.contains("opened")), "{r:?}");
    assert!(!f.privilege_on);
}

#[test]
fn a_handle_without_an_image_path_is_a_failure_not_a_guess() {
    let mut p = dyno(10, 1);
    p.no_image = true;
    let mut f = Fake::new(vec![p]);
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 11);
    assert!(matches!(&r.actions[0].outcome, Outcome::Failed(w) if w.contains("image path")), "{r:?}");
    assert_eq!(f.count("terminate"), 0);
}

#[test]
fn a_terminate_the_system_refuses_is_a_failure() {
    let mut p = dyno(10, 1);
    p.terminate_fails = true;
    let mut f = Fake::new(vec![p]);
    let r = run(&mut f);
    assert_eq!(r.exit_code(), 11);
    assert!(matches!(&r.actions[0].outcome, Outcome::Failed(w) if w.contains("TerminateProcess")), "{r:?}");
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
    let req = Request { dir: Path::new(DIR), images: &imgs, own: id(9000), own_image: None };
    let bounds = Bounds { end_each: Duration::from_secs(5), end_total: Duration::ZERO };
    let r = stop_all(&mut f, &req, &bounds);
    assert_eq!(r.exit_code(), 0, "{r:?}");
    assert_eq!(Bounds::default().end_total, Duration::from_secs(20));
}

#[test]
fn errors_are_described_for_the_log() {
    for e in [
        StopError::RestartManager { call: "RmStartSession", code: 5 },
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
