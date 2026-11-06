//! The verbs over the shared simulator: `Manager` + `Systemd` against `FakeSystemd`. What the transition decides is tested once
//! in `paddock-service` (its model and its Linux scenarios); these tests check the wiring: Dyno's unit, its paths, the words and
//! the exit codes of the verbs.

use super::*;
use std::sync::atomic::AtomicBool;
use paddock_env::MapVars;
use paddock_service::fake_systemd::{Active, FakeSystemd};
use paddock_service::testing::FakeSystem;

const HOME: &str = "/home/ana";
const EXE: &str = "/opt/dyno/dyno-agent";

struct Rig {
    sys: FakeSystem,
    ctl: FakeSystemd,
    vars: MapVars,
    data: tempfile::TempDir,
    unit: PathBuf,
}

impl Rig {
    fn new() -> Rig {
        let vars = MapVars::new().with("HOME", HOME);
        let unit = unit_path(&vars).unwrap();
        let sys = FakeSystem::new();
        sys.put_program(EXE);
        let ctl = FakeSystemd::new(&sys, "dyno-agent.service", &unit);
        Rig {
            sys,
            ctl,
            vars,
            data: tempfile::tempdir().unwrap(),
            unit,
        }
    }

    fn manager(&self, version: &str, timings: Timings) -> Manager {
        let sys = self.sys.clone();
        let factory: SystemFactory = Box::new(move || Box::new(sys.clone()));
        manager(Path::new(EXE), &self.vars, self.data.path(), version, &factory, timings).unwrap()
    }

    fn installed(self) -> Rig {
        install(&mut self.manager("1.1.0", install_timings()), &AtomicBool::new(false)).unwrap();
        self
    }

    fn unit_text(&self) -> Option<String> {
        self.sys.file(&self.unit)
    }
}

#[test]
fn the_unit_is_where_systemd_reads_user_units_and_says_what_it_starts() {
    let rig = Rig::new().installed();
    assert_eq!(rig.unit, PathBuf::from("/home/ana/.config/systemd/user/dyno-agent.service"));
    let text = rig.unit_text().unwrap();
    for line in [
        "ExecStart=\"/opt/dyno/dyno-agent\" \"run\"",
        "Restart=always",
        "RestartSec=5",
        "Nice=10",
        "CPUWeight=20",
        "MemoryHigh=256M",
        "WantedBy=default.target",
        "Documentation=https://dev.decryptology.net/decryp7/dyno",
        "SyslogIdentifier=dyno-agent",
        "IOSchedulingClass=best-effort",
        "IOSchedulingPriority=7",
    ] {
        assert!(text.lines().any(|l| l == line), "{line} is missing from\n{text}");
    }
    assert!(
        !text.contains("IOSchedulingClass=idle"),
        "rule 52: the agent answers requests somebody waits for\n{text}"
    );
    let absolute = MapVars::new().with("HOME", HOME).with("XDG_CONFIG_HOME", "/home/ana/cfg");
    assert_eq!(
        unit_path(&absolute).unwrap(),
        PathBuf::from("/home/ana/cfg/systemd/user/dyno-agent.service")
    );
    assert!(unit_path(&MapVars::new()).is_err(), "no home folder: nothing to register in");
}

#[test]
fn a_program_path_with_a_space_is_quoted_in_the_unit() {
    let rig = Rig::new();
    let sys = rig.sys.clone();
    let factory: SystemFactory = Box::new(move || Box::new(sys.clone()));
    let exe = Path::new("/opt/Dyno App/dyno-agent");
    rig.sys.put_program(exe);
    let mut m = manager(exe, &rig.vars, rig.data.path(), "1.1.0", &factory, install_timings()).unwrap();
    install(&mut m, &AtomicBool::new(false)).unwrap();
    assert!(
        rig.unit_text().unwrap().contains("ExecStart=\"/opt/Dyno App/dyno-agent\" \"run\""),
        "{:?}",
        rig.unit_text()
    );
}

#[test]
fn install_registers_enables_and_starts_the_unit_and_a_second_install_replaces_it() {
    let rig = Rig::new();
    install(&mut rig.manager("1.1.0", install_timings()), &AtomicBool::new(false)).unwrap();
    assert!(rig.unit_text().is_some());
    assert!(rig.ctl.enabled());
    assert_eq!(rig.ctl.active(), Active::Active);
    assert!(rig.manager("1.1.0", install_timings()).status().running);
    install(&mut rig.manager("1.1.0", install_timings()), &AtomicBool::new(false)).unwrap();
    assert_eq!(rig.ctl.active(), Active::Active);
}

#[test]
fn install_without_a_user_manager_says_how_to_reach_it_and_writes_nothing_that_is_enabled() {
    let rig = Rig::new();
    rig.ctl.logout();
    let e = install(&mut rig.manager("1.1.0", install_timings()), &AtomicBool::new(false)).unwrap_err();
    assert!(e.contains("not reachable") && e.contains("log in"), "{e}");
    assert!(!rig.ctl.enabled());
}

#[test]
fn install_over_a_unit_the_user_disabled_says_what_to_run_instead_of_switching_it_on() {
    let rig = Rig::new().installed();
    rig.ctl.user_disables();
    let e = install(&mut rig.manager("1.1.0", install_timings()), &AtomicBool::new(false)).unwrap_err();
    assert!(e.contains("systemctl --user enable --now dyno-agent.service"), "{e}");
    assert!(!rig.ctl.enabled());
}

#[test]
fn ensure_registers_a_stale_unit_again_and_leaves_a_current_one_a_missing_one_and_a_disabled_one_alone() {
    // Not installed stays not installed.
    let rig = Rig::new();
    assert_eq!(ensure(&mut rig.manager("1.1.0", install_timings()), false, &AtomicBool::new(false)), Ok(false));
    assert!(rig.unit_text().is_none());

    // Installed and current: nothing to do.
    let rig = Rig::new().installed();
    let mark = rig.sys.commands().len();
    assert_eq!(ensure(&mut rig.manager("1.1.0", install_timings()), false, &AtomicBool::new(false)), Ok(false));
    assert!(rig.sys.commands().len() > mark);
    assert!(rig.sys.commands()[mark..]
        .iter()
        .all(|c| !c.starts_with("systemctl --user enable") && !c.starts_with("systemctl --user disable")));

    // Another executable: registered again for this one.
    rig.sys.put_file(&rig.unit, "[Service]\nExecStart=/old/dyno-agent run\n");
    assert_eq!(ensure(&mut rig.manager("1.1.0", install_timings()), false, &AtomicBool::new(false)), Ok(true));
    assert!(rig.unit_text().unwrap().contains("ExecStart=\"/opt/dyno/dyno-agent\" \"run\""));

    // What the user disabled stays disabled.
    rig.ctl.user_disables();
    rig.sys.put_file(&rig.unit, "[Service]\nExecStart=/old/dyno-agent run\n");
    assert_eq!(ensure(&mut rig.manager("1.1.0", install_timings()), false, &AtomicBool::new(false)), Ok(false));
    assert!(rig.unit_text().unwrap().contains("/old/dyno-agent"));
}

#[test]
fn ensure_after_an_update_writes_the_new_text_of_the_unit_once() {
    let rig = Rig::new().installed();
    rig.sys
        .put_file(&rig.unit, "[Service]\nExecStart=/opt/dyno/dyno-agent run\nRestart=always\n");
    assert_eq!(ensure(&mut rig.manager("1.2.0", install_timings()), false, &AtomicBool::new(false)), Ok(true));
    assert!(rig.unit_text().unwrap().contains("MemoryHigh=256M"));
    let mark = rig.sys.commands().len();
    assert_eq!(ensure(&mut rig.manager("1.2.0", install_timings()), false, &AtomicBool::new(false)), Ok(false));
    assert!(rig.sys.commands()[mark..].iter().all(|c| !c.starts_with("systemctl --user enable")));
}

#[test]
fn uninstall_stops_disables_and_removes_the_unit_and_removing_what_is_not_there_succeeds() {
    let rig = Rig::new().installed();
    uninstall(&mut rig.manager("1.1.0", install_timings())).unwrap();
    assert!(rig.unit_text().is_none());
    assert!(!rig.ctl.enabled());
    assert_eq!(rig.ctl.active(), Active::Inactive);
    uninstall(&mut rig.manager("1.1.0", install_timings())).unwrap();
}

#[test]
fn status_says_what_the_verb_always_said() {
    let rig = Rig::new();
    assert_eq!(status_line(&mut rig.manager("1.1.0", install_timings())), "not installed");
    let rig = rig.installed();
    assert_eq!(status_line(&mut rig.manager("1.1.0", install_timings())), "installed, running");
    rig.ctl.user_stops();
    assert_eq!(status_line(&mut rig.manager("1.1.0", install_timings())), "installed, not running");
    rig.sys.put_file(&rig.unit, "[Service]\nExecStart=/old/dyno-agent run\n");
    assert_eq!(
        status_line(&mut rig.manager("1.1.0", install_timings())),
        "installed, not running, registered for another executable"
    );
}

#[test]
fn ensure_result_names_what_an_unanswered_registration_means_with_and_without_the_wait() {
    use paddock_service::Hold;
    let reg = || Started::Repaired(Reason::Moved, RepairOutcome::Registered);
    assert_eq!(ensure_result(reg(), false), Ok(true));
    assert!(ensure_result(reg(), true).unwrap_err().contains("did not start"));
    assert_eq!(
        ensure_result(Started::Repaired(Reason::Moved, RepairOutcome::Recovered), true),
        Ok(true)
    );
    assert_eq!(ensure_result(Started::Left(Hold::Healthy), true), Ok(false));
    assert!(ensure_result(Started::Busy, true).is_err());
    assert!(ensure_result(Started::Repaired(Reason::Moved, RepairOutcome::Failed("x".into())), true).is_err());
}

#[test]
fn the_verbs_print_and_fail_as_they_always_did() {
    use crate::service::run_verb;
    let rig = Rig::new();
    let mut unit = Unit::new(rig.manager("1.1.0", install_timings()));
    assert_eq!(run_verb("status", &mut unit).unwrap(), "not installed");
    assert_eq!(run_verb("ensure", &mut unit).unwrap(), "nothing to do");
    assert_eq!(run_verb("install", &mut unit).unwrap(), "installed and started");
    assert_eq!(run_verb("status", &mut unit).unwrap(), "installed, running");
    rig.sys.put_file(&rig.unit, "[Service]\nExecStart=/old/dyno-agent run\n");
    assert_eq!(run_verb("ensure", &mut unit).unwrap(), "registered again");
    assert_eq!(run_verb("uninstall", &mut unit).unwrap(), "removed");
    assert_eq!(run_verb("uninstall", &mut unit).unwrap(), "removed");
    assert_eq!(
        run_verb("restart", &mut unit).unwrap_err(),
        "service takes install, uninstall, status or ensure"
    );
    rig.ctl.logout();
    assert!(
        run_verb("install", &mut unit).is_err(),
        "a failing verb is an error, which the command line turns into an exit code"
    );
}
