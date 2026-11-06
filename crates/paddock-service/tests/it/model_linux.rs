//! The Linux scenarios of the service model (ADR 0056 sections 6 and 8): the real [`Manager`] and the real [`Systemd`]
//! backend over a fake systemd user manager ([`FakeSystemd`]) with `Caps::Linux` (no pinned identity, `automatic`, a
//! definition that is compared). What the user and the OS do to the unit is the alphabet: `systemctl --user disable`,
//! `stop`, a crash, a restart loop, a logout with and without lingering. Every scenario is named, and the explorer takes
//! every sequence of them to a bounded depth and checks the invariants that apply to a systemd unit. The OS is a struct
//! with a virtual clock, so there is no randomness, no sleeping and no real systemd (ER-14).

use paddock_service::backend::{Systemd, Target};
use paddock_service::fake_systemd::{Active, FakeSystemd};
use paddock_service::record::RecordFile;
use paddock_service::stop::{StopTimings, Stopped};
use paddock_service::testing::FakeSystem;
use paddock_service::{Hold, Manager, Plan, ServiceError, ServiceSpec, Started};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const LABEL: &str = "net.model.agent";
const UNIT: &str = "model-agent.service";
const V1: &str = "1.1.0";
const V2: &str = "1.2.0";

fn spec() -> ServiceSpec {
    ServiceSpec::builder(LABEL, "Model", "model-agent")
        .unit("model-agent")
        .args(["run"])
        .build()
        .unwrap()
}

/// One machine: a fake systemd user manager, a unit folder, the in-memory record and the program.
struct Rig {
    sys: FakeSystem,
    ctl: FakeSystemd,
    unit_path: PathBuf,
    exe: PathBuf,
    dir: tempfile::TempDir,
    record: RecordFile,
}

impl Rig {
    fn new() -> Rig {
        let sys = FakeSystem::new();
        let unit_path = PathBuf::from("/home/ana/.config/systemd/user").join(UNIT);
        let exe = PathBuf::from("/opt/model/model-agent");
        sys.put_program(&exe);
        let ctl = FakeSystemd::new(&sys, UNIT, &unit_path);
        Rig {
            sys,
            ctl,
            unit_path,
            exe,
            dir: tempfile::tempdir().unwrap(),
            record: RecordFile::memory(),
        }
    }

    /// The manager of version `version`; the service answers when systemd says the unit is active.
    fn manager(&self, version: &str) -> Manager {
        let backend = Systemd::new(
            spec(),
            self.unit_path.clone(),
            Target::exe(self.exe.clone()),
            Box::new(self.sys.clone()),
        );
        let ctl = self.ctl.clone();
        Manager::new(
            "Model",
            Box::new(backend),
            self.dir.path(),
            LABEL,
            version,
            move || ctl.active() == Active::Active,
        )
        .with_record(self.record.reopen())
    }

    fn registered(&self) -> bool {
        self.sys.file(&self.unit_path).is_some()
    }

    /// The commands that changed something since `mark`.
    fn changes_since(&self, mark: usize) -> Vec<String> {
        self.sys.commands()[mark..]
            .iter()
            .filter(|c| {
                ["enable", "disable", "start", "stop"]
                    .iter()
                    .any(|v| c.starts_with(&format!("systemctl --user {v}")))
            })
            .cloned()
            .collect()
    }

    fn mark(&self) -> usize {
        self.sys.commands().len()
    }

    /// Installs and starts version `V1` the way a first run does.
    fn installed(self) -> Rig {
        self.manager(V1).install(&never()).unwrap();
        self
    }
}

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

// ------------------------------------------------------------------------------------------------ named scenarios

#[test]
fn install_writes_the_unit_enables_and_starts_it_and_the_record_remembers_the_version() {
    let rig = Rig::new();
    rig.manager(V1).install(&never()).unwrap();
    assert!(rig.registered());
    assert!(rig.ctl.enabled());
    assert_eq!(rig.ctl.active(), Active::Active);
    assert_eq!(rig.record.load().registered_version.as_deref(), Some(V1));
    let text = rig.sys.file(&rig.unit_path).unwrap();
    assert!(
        text.contains("ExecStart=\"/opt/model/model-agent\" \"run\""),
        "{text}"
    );
}

#[test]
fn a_user_who_disabled_the_unit_is_never_contradicted_by_a_start_a_resume_or_an_update() {
    let rig = Rig::new().installed();
    rig.ctl.user_disables();
    let mark = rig.mark();
    for version in [V1, V2] {
        let mut m = rig.manager(version);
        assert_eq!(m.on_start(&never()), Started::Left(Hold::SwitchedOff));
        assert!(!m.resume(&never(), &mut |_| {}).unwrap());
    }
    assert_eq!(rig.changes_since(mark), Vec::<String>::new());
    assert!(!rig.ctl.enabled());
    // Pressing Install is consent but the switch is the user's: it says what to do and changes nothing.
    assert!(matches!(
        rig.manager(V1).install(&never()),
        Err(ServiceError::SwitchedOff)
    ));
    assert_eq!(rig.changes_since(mark), Vec::<String>::new());
}

#[test]
fn a_unit_the_user_stopped_is_left_stopped_by_a_start_and_brought_back_by_a_resume() {
    let rig = Rig::new().installed();
    rig.ctl.user_stops();
    let mark = rig.mark();
    let mut m = rig.manager(V1);
    // A start does not undo a stop by the user: the registration is current, the OS runs nothing, the plan leaves it.
    assert_eq!(m.on_start(&never()), Started::Left(Hold::Healthy));
    assert_eq!(rig.changes_since(mark), Vec::<String>::new());
    // A resume is the app bringing back what it stopped itself: it starts the unit again, and never disables anything.
    assert!(m.resume(&never(), &mut |_| {}).unwrap());
    assert_eq!(rig.ctl.active(), Active::Active);
    assert!(
        rig.changes_since(mark)
            .iter()
            .all(|c| !c.contains("disable"))
    );
}

#[test]
fn quit_and_stop_names_the_user_unit_and_a_resume_starts_it_again_without_touching_the_switch() {
    let rig = Rig::new().installed();
    let mut m = rig.manager(V1);
    let how = m
        .stop_mechanism()
        .expect("an active user unit can be stopped by name");
    let mut sys = rig.sys.clone();
    assert_eq!(
        paddock_service::stop::stop(&mut sys, &how, StopTimings::default()).unwrap(),
        Stopped::Stopped
    );
    assert_eq!(rig.ctl.active(), Active::Inactive);
    assert!(rig.ctl.enabled(), "a stop keeps the registration");
    assert!(m.resume(&never(), &mut |_| {}).unwrap());
    assert_eq!(rig.ctl.active(), Active::Active);
    assert!(rig.ctl.enabled());
}

#[test]
fn a_stop_names_nothing_while_the_unit_runs_nothing() {
    let rig = Rig::new().installed();
    rig.ctl.user_stops();
    assert_eq!(rig.manager(V1).stop_mechanism(), None);
}

#[test]
fn a_crash_is_systemds_to_restart_and_a_start_during_the_wait_replaces_nothing() {
    let rig = Rig::new().installed();
    rig.ctl.crashes();
    let mark = rig.mark();
    let mut m = rig.manager(V1);
    assert_eq!(m.on_start(&never()), Started::Left(Hold::Healthy));
    assert!(
        m.supervises(),
        "an activating unit is about to be restarted by systemd"
    );
    assert_eq!(rig.changes_since(mark), Vec::<String>::new());
    rig.ctl.restarted();
    assert_eq!(rig.ctl.active(), Active::Active);
}

#[test]
fn a_restart_loop_is_systemds_and_is_neither_registered_over_nor_hidden() {
    let rig = Rig::new().installed();
    rig.ctl.restart_loop(40, false);
    let mark = rig.mark();
    let mut m = rig.manager(V1);
    for _ in 0..3 {
        assert_eq!(m.on_start(&never()), Started::Left(Hold::Healthy));
    }
    assert_eq!(rig.changes_since(mark), Vec::<String>::new());
    let status = m.status();
    assert!(status.installed && !status.running, "{status:?}");
    // A start limit that gave up leaves the unit `failed`: still registered, still not replaced by a start.
    rig.ctl.restart_loop(5, true);
    assert_eq!(m.on_start(&never()), Started::Left(Hold::Healthy));
    assert_eq!(rig.changes_since(mark), Vec::<String>::new());
    assert!(
        m.installed(),
        "a failed unit is still the user's registration"
    );
}

#[test]
fn a_registration_whose_service_exits_at_once_is_registered_a_bounded_number_of_times() {
    let rig = Rig::new();
    rig.ctl.starts_into_a_loop(true);
    let mut registers = 0;
    for _ in 0..8 {
        let mut m = rig.manager(V1);
        let _ = m.install_unattended(&never());
        let _ = m.on_start(&never());
        registers = rig
            .sys
            .commands()
            .iter()
            .filter(|c| c.starts_with("systemctl --user enable"))
            .count();
    }
    assert!(registers <= 3 + 8, "{registers} enables for one version");
}

#[test]
fn a_logout_without_lingering_stops_the_unit_and_the_next_login_starts_it_without_the_app() {
    let rig = Rig::new().installed();
    rig.ctl.logout();
    assert_eq!(rig.ctl.active(), Active::Inactive);
    assert!(
        rig.registered() && rig.ctl.enabled(),
        "the registration outlives the session"
    );
    rig.ctl.login();
    assert_eq!(
        rig.ctl.active(),
        Active::Active,
        "systemd starts an enabled unit with the user's manager"
    );
    assert_eq!(
        rig.manager(V1).on_start(&never()),
        Started::Left(Hold::Healthy)
    );
}

#[test]
fn nothing_is_changed_while_systemctl_cannot_be_reached_and_the_words_say_what_to_do() {
    let rig = Rig::new().installed();
    rig.ctl.logout();
    let before = rig.sys.file(&rig.unit_path);
    let mark = rig.mark();
    let mut m = rig.manager(V2);
    assert!(matches!(
        m.on_start(&never()),
        Started::Left(Hold::Unavailable(_))
    ));
    assert!(matches!(m.resume(&never(), &mut |_| {}), Ok(false)));
    assert_eq!(rig.changes_since(mark), Vec::<String>::new());
    assert_eq!(
        rig.sys.file(&rig.unit_path),
        before,
        "the unit file is not rewritten"
    );
    let Err(e) = m.install(&never()) else {
        panic!("install cannot work without a bus");
    };
    assert!(matches!(e, ServiceError::Unavailable(_)), "{e}");
    assert!(rig.registered(), "a failed request removes nothing");
}

#[test]
fn a_removal_that_cannot_reach_systemd_says_so_and_leaves_the_unit_and_the_record_as_they_were() {
    let rig = Rig::new().installed();
    rig.ctl.logout();
    let Err(e) = rig.manager(V1).uninstall() else {
        panic!("a removal that changed nothing must not say it worked");
    };
    assert!(
        matches!(e, ServiceError::Unavailable(_) | ServiceError::Failed(_)),
        "{e}"
    );
    assert!(rig.registered());
    assert_eq!(rig.record.load().registered_version.as_deref(), Some(V1));
    // The next start does not finish a removal that never happened.
    rig.ctl.login();
    assert_ne!(rig.manager(V1).check(), Plan::Remove);
}

#[test]
fn uninstall_stops_disables_and_removes_the_unit_and_forgets_the_version() {
    let rig = Rig::new().installed();
    rig.manager(V1)
        .uninstall_patient(6, Duration::from_secs(5))
        .unwrap();
    assert!(!rig.registered());
    assert!(!rig.ctl.enabled());
    assert_eq!(rig.ctl.active(), Active::Inactive);
    assert_eq!(rig.record.load().registered_version, None);
    // Removing what is not there succeeds.
    rig.manager(V1).uninstall().unwrap();
}

#[test]
fn an_update_rewrites_the_unit_once_and_a_unit_in_the_new_text_is_only_marked() {
    let rig = Rig::new().installed();
    let mark = rig.mark();
    assert!(matches!(
        rig.manager(V2).on_start(&never()),
        Started::Marked
    ));
    assert_eq!(
        rig.changes_since(mark),
        Vec::<String>::new(),
        "the text is current: only the version is remembered"
    );
    assert_eq!(rig.record.load().registered_version.as_deref(), Some(V2));
    // An older unit text is registered again, once.
    rig.sys.put_file(
        &rig.unit_path,
        "[Service]\nExecStart=/opt/model/model-agent run\nRestart=always\n",
    );
    let mark = rig.mark();
    let started = rig.manager("1.3.0").on_start(&never());
    assert!(matches!(started, Started::Repaired(..)), "{started:?}");
    assert_eq!(
        rig.changes_since(mark)
            .iter()
            .filter(|c| c.contains("enable --now"))
            .count(),
        1
    );
    let cmds: Vec<String> = rig.sys.commands()[mark..].to_vec();
    assert!(
        !cmds.iter().any(|c| c.contains(" disable") || c.contains(" stop")),
        "the working service is registered over, never removed first: {cmds:?}"
    );
    assert_eq!(
        cmds.iter().filter(|c| c.contains(" restart")).count(),
        1,
        "the unit it replaced is restarted so the new text runs: {cmds:?}"
    );
    assert!(
        rig.sys
            .file(&rig.unit_path)
            .unwrap()
            .contains("SyslogIdentifier=model-agent")
    );
}

#[test]
fn a_unit_that_points_at_a_removed_program_is_registered_again_for_the_program_that_is_there() {
    let rig = Rig::new().installed();
    let text = rig
        .sys
        .file(&rig.unit_path)
        .unwrap()
        .replace("/opt/model/model-agent", "/gone/model-agent");
    rig.sys.put_file(&rig.unit_path, &text);
    let started = rig.manager(V1).on_start(&never());
    assert!(matches!(started, Started::Repaired(..)), "{started:?}");
    assert!(
        rig.sys
            .file(&rig.unit_path)
            .unwrap()
            .contains("ExecStart=\"/opt/model/model-agent\" \"run\"")
    );
}

// ------------------------------------------------------------------------------------------------------- explorer

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Do {
    Start,
    Update,
    Install,
    Uninstall,
    Resume,
    Stop,
    UserDisables,
    UserStops,
    Crashes,
    RestartLoop,
    Logout,
    Login,
    Linger,
}

const ALPHABET: [Do; 13] = [
    Do::Start,
    Do::Update,
    Do::Install,
    Do::Uninstall,
    Do::Resume,
    Do::Stop,
    Do::UserDisables,
    Do::UserStops,
    Do::Crashes,
    Do::RestartLoop,
    Do::Logout,
    Do::Login,
    Do::Linger,
];

struct Walk {
    rig: Rig,
    version: &'static str,
    /// What the invariants found, with the sequence that replays it.
    notes: Vec<String>,
    /// `enable` commands an unattended entry caused, per version.
    unattended_enables: BTreeMap<&'static str, u32>,
    /// The user asked for the service and has not removed it since.
    wanted: bool,
}

fn step(w: &mut Walk, e: Do) {
    let unattended = matches!(e, Do::Start | Do::Update | Do::Resume);
    let (up, file_before, enabled_before, active_before, registered_before) = (
        w.rig.ctl.bus_up(),
        w.rig.sys.file(&w.rig.unit_path),
        w.rig.ctl.enabled(),
        w.rig.ctl.active(),
        w.rig.registered(),
    );
    let mark = w.rig.mark();
    match e {
        Do::Update => {
            w.version = V2;
            w.rig.manager(w.version).on_start(&never());
        }
        Do::Start => {
            w.rig.manager(w.version).on_start(&never());
        }
        Do::Resume => {
            let _ = w.rig.manager(w.version).resume(&never(), &mut |_| {});
        }
        Do::Install => {
            w.wanted = true;
            let _ = w.rig.manager(w.version).install(&never());
        }
        Do::Uninstall => {
            w.wanted = false;
            let result = w.rig.manager(w.version).uninstall();
            if result.is_ok()
                && (w.rig.registered()
                    || w.rig.ctl.enabled()
                    || w.rig.ctl.active() == Active::Active)
            {
                w.notes
                    .push("an uninstall that said it worked left the unit behind".into());
            }
            if result.is_err() {
                // The service was not removed, so the request stands.
                w.wanted = registered_before || w.wanted;
            }
        }
        Do::Stop => {
            let mut sys = w.rig.sys.clone();
            if let Some(how) = w.rig.manager(w.version).stop_mechanism() {
                let _ = paddock_service::stop::stop(&mut sys, &how, StopTimings::default());
            }
        }
        Do::UserDisables => {
            if w.rig.ctl.enabled() {
                w.rig.ctl.user_disables();
            }
        }
        Do::UserStops => w.rig.ctl.user_stops(),
        Do::Crashes => {
            if w.rig.ctl.active() == Active::Active {
                w.rig.ctl.crashes();
            }
        }
        Do::RestartLoop => {
            if w.rig.ctl.active() != Active::Inactive {
                w.rig.ctl.restart_loop(10, false);
            }
        }
        Do::Logout => w.rig.ctl.logout(),
        Do::Login => w.rig.ctl.login(),
        Do::Linger => w.rig.ctl.set_linger(true),
    }
    let changed = w.rig.changes_since(mark);
    let enables = changed.iter().filter(|c| c.contains("enable")).count() as u32;
    if unattended {
        *w.unattended_enables.entry(w.version).or_default() += enables;
        if !up
            && (!changed.is_empty()
                || w.rig.sys.file(&w.rig.unit_path) != file_before && file_before.is_some())
        {
            w.notes.push(format!(
                "{e:?} changed the unit while systemctl could not be reached"
            ));
        }
        if up && registered_before && !enabled_before && !changed.is_empty() {
            w.notes.push(format!(
                "{e:?} contradicted the user's disable: {changed:?}"
            ));
        }
        if up && registered_before && !enabled_before && w.rig.ctl.enabled() {
            w.notes
                .push(format!("{e:?} enabled a unit the user disabled"));
        }
        if active_before != Active::Inactive
            && up
            && w.version == V1
            && changed.iter().any(|c| c.contains("disable"))
            && e != Do::Update
        {
            w.notes
                .push(format!("{e:?} removed a unit that runs or is about to"));
        }
    }
    if w.rig.registered() != registered_before && unattended && !w.wanted && registered_before {
        w.notes.push(format!(
            "{e:?} lost the user's registration without a request"
        ));
    }
}

fn sequences(depth: usize) -> Vec<Vec<Do>> {
    let mut all: Vec<Vec<Do>> = vec![vec![]];
    let mut layer: Vec<Vec<Do>> = vec![vec![]];
    for _ in 0..depth {
        let mut next = Vec::new();
        for s in &layer {
            for e in ALPHABET {
                let mut t = s.clone();
                t.push(e);
                next.push(t);
            }
        }
        all.extend(next.iter().cloned());
        layer = next;
    }
    all
}

fn explore(depth: usize) -> (usize, BTreeMap<String, String>) {
    let mut runs = 0;
    let mut failures: BTreeMap<String, String> = BTreeMap::new();
    for fresh in [true, false] {
        for seq in sequences(depth) {
            runs += 1;
            let rig = if fresh {
                Rig::new()
            } else {
                Rig::new().installed()
            };
            let mut w = Walk {
                rig,
                version: V1,
                notes: Vec::new(),
                unattended_enables: BTreeMap::new(),
                wanted: !fresh,
            };
            for e in &seq {
                step(&mut w, *e);
            }
            for (v, n) in &w.unattended_enables {
                if *n > 3 * (1 + seq.len() as u32) {
                    w.notes.push(format!(
                        "{n} unattended enables of version {v} (not bounded)"
                    ));
                }
            }
            // No lost service: a service the user asked for, not removed or disabled since, is registered after the
            // user is back and a start has run.
            w.rig.ctl.set_linger(true);
            w.rig.ctl.login();
            for _ in 0..2 {
                w.rig.manager(w.version).on_start(&never());
            }
            for note in w.notes {
                failures
                    .entry(
                        note.split(" while ")
                            .next()
                            .unwrap_or(&note)
                            .chars()
                            .take(60)
                            .collect(),
                    )
                    .or_insert_with(|| format!("{seq:?} (fresh {fresh}): {note}"));
            }
        }
    }
    (runs, failures)
}

#[test]
fn every_sequence_of_the_linux_alphabet_keeps_the_invariants() {
    let (runs, failures) = explore(3);
    println!("{runs} runs");
    assert!(
        failures.is_empty(),
        "{runs} runs; first of each kind:\n{}",
        failures.values().cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn a_unit_that_points_at_another_copy_is_removed_first_not_registered_over_while_it_runs() {
    // Register-first is for an update only: a registration that points elsewhere is taken out before the new one is made.
    let rig = Rig::new().installed();
    rig.sys.put_program("/opt/old/model-agent");
    rig.sys.put_file(
        &rig.unit_path,
        "[Service]\nExecStart=\"/opt/old/model-agent\" \"run\"\nRestart=always\n",
    );
    let mark = rig.mark();
    let started = rig.manager(V1).on_start(&never());
    assert!(matches!(started, Started::Repaired(..)), "{started:?}");
    let cmds: Vec<String> = rig.sys.commands()[mark..].to_vec();
    assert!(cmds.iter().any(|c| c.contains(" disable") || c.contains(" stop")), "{cmds:?}");
}
