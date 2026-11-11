//! Tests of the install plan and the installer over a scripted backend and an in-memory disk, then over the real
//! backends of each OS on the recording fake (`launchctl`, `sc`, `systemctl` are never run).

use super::*;
use crate::backend::{
    Backend, Capabilities, Inspection, Job, Mechanism, Registration, UnregisterError,
};
use crate::port::{Output, RegisterError, RunError};
use crate::privileged::layout::tests::helper_spec;
use crate::testing::{FakeFiles, FakeSystem};
use paddock_env::Os;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[test]
fn a_service_removing_itself_confirms_the_registration_is_gone_and_says_so_when_it_is_not() {
    // ADR 0054 RC-2: the unregister call reported success but the OS still holds the registration (the Linux packaged unit
    // stayed enabled). The removal must not end as done.
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.registration_override = Some(Registration::Registered);
    });
    let layout = layout_for(Os::Linux);
    let (mut f, mut b, mut s) = (FakeFiles::new(), backend.clone(), FakeSystem::new());
    let err = Installer::new(&layout, &mut f, &mut b, &mut s)
        .uninstall_self()
        .unwrap_err();
    assert!(
        matches!(
            err,
            InstallError::Failed {
                phase: Step::ConfirmGone,
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_service_removing_itself_may_still_be_running_because_this_process_is_the_job() {
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.running = true;
        s.job_override = Some(Job::Running);
    });
    let layout = layout_for(Os::Linux);
    let (mut f, mut b, mut s) = (FakeFiles::new(), backend.clone(), FakeSystem::new());
    Installer::new(&layout, &mut f, &mut b, &mut s)
        .with_waits(Waits {
            gone: Duration::from_secs(1),
            ..Waits::default()
        })
        .uninstall_self()
        .unwrap();
}

const MAC_PROGRAM: &str = "/Library/PrivilegedHelperTools/net.decryptology.dyno.helper";
const SOURCE: &str = "/Applications/Dyno.app/Contents/MacOS/dyno-helper";
const WIN_FOLDER: &str = "C:\\Program Files\\Dyno Helper";
const WIN_PROGRAM: &str = "C:\\Program Files\\Dyno Helper\\dyno-helper.exe";
/// The bundled helper of a per-machine Dyno (Program Files: only administrators can change it).
const WIN_SOURCE: &str = "C:\\Program Files\\Dyno\\dyno-helper.exe";
/// The bundled helper of a per-user Dyno (a folder its user can change).
const WIN_USER_SOURCE: &str = "C:\\Users\\Ana\\AppData\\Local\\Dyno\\dyno-helper.exe";

fn layout_for(os: Os) -> Layout {
    let roots = crate::privileged::Roots {
        program_files: PathBuf::from("C:\\Program Files"),
    };
    crate::privileged::layout(os, &helper_spec(), "Dyno Helper", &roots).unwrap()
}

/// What the scripted backend does, shared with the test.
#[derive(Default)]
struct State {
    registered: bool,
    running: bool,
    job_held_polls: u32,
    starts_after_polls: Option<u32>,
    polls_since_register: u32,
    log: Vec<String>,
    register_error: Option<RegisterError>,
    evict_error: Option<String>,
    unregister_error: Option<String>,
    registration_override: Option<Registration>,
    job_override: Option<Job>,
    points_here: Option<bool>,
    /// The running job's process, and how many asks it stays alive for.
    process: Option<paddock_proc::ProcessId>,
    alive_polls: u32,
    /// The process table cannot be read: every look at the process is `Unknown`.
    process_unreadable: bool,
}

/// One look at the scripted OS: it counts as a poll for a job that starts after some.
fn look(s: &mut State) -> Inspection {
    if s.registered {
        s.polls_since_register += 1;
        if s.starts_after_polls
            .is_some_and(|n| s.polls_since_register > n)
        {
            s.running = true;
        }
    }
    view(s)
}

/// What the scripted OS holds now, without logging or advancing anything.
fn view(s: &State) -> Inspection {
    Inspection {
        registration: s.registration_override.clone().unwrap_or(if s.registered {
            Registration::Registered
        } else {
            Registration::Absent
        }),
        job: s.job_override.unwrap_or(if s.running {
            Job::Running
        } else if s.registered {
            Job::NotRunning
        } else {
            Job::Absent
        }),
        points_here: s.points_here,
        ..Inspection::absent()
    }
}

#[derive(Clone, Default)]
struct Scripted(Arc<Mutex<State>>);

impl Scripted {
    fn with(f: impl FnOnce(&mut State)) -> Self {
        let s = Self::default();
        f(&mut s.0.lock().unwrap());
        s
    }
    fn log(&self) -> Vec<String> {
        self.0.lock().unwrap().log.clone()
    }
}

impl Backend for Scripted {
    fn mechanism(&self) -> Mechanism {
        Mechanism::LaunchdFile
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            automatic: false,
            pinned_identity: false,
            login_items: false,
        }
    }
    fn inspect(&mut self) -> Inspection {
        let mut s = self.0.lock().unwrap();
        s.log.push("inspect".into());
        look(&mut s)
    }
    fn register(&mut self) -> Result<Inspection, RegisterError> {
        let mut s = self.0.lock().unwrap();
        s.log.push("register".into());
        if let Some(e) = s.register_error.clone() {
            return Err(e);
        }
        s.registered = true;
        s.polls_since_register = 0;
        Ok(look(&mut s))
    }
    fn unregister(&mut self) -> Result<Inspection, UnregisterError> {
        let mut s = self.0.lock().unwrap();
        s.log.push("unregister".into());
        if let Some(e) = s.unregister_error.clone() {
            return Err(UnregisterError::Failed(e));
        }
        s.registered = false;
        Ok(Inspection::absent())
    }
    fn evict(&mut self) -> Result<(), String> {
        let mut s = self.0.lock().unwrap();
        s.log.push("evict".into());
        if let Some(e) = s.evict_error.clone() {
            return Err(e);
        }
        s.running = false;
        Ok(())
    }
    fn job_gone(&mut self) -> bool {
        let mut s = self.0.lock().unwrap();
        s.log.push("job_gone".into());
        if s.job_held_polls > 0 {
            s.job_held_polls -= 1;
            return false;
        }
        true
    }
    fn pause(&mut self, _how_long: Duration) {
        self.0.lock().unwrap().log.push("pause".into());
    }
    fn job_process(&mut self) -> Option<paddock_proc::ProcessId> {
        self.0.lock().unwrap().process
    }
    fn process_presence(&mut self, process: paddock_proc::ProcessId) -> paddock_proc::Presence {
        use paddock_proc::Presence;
        let mut s = self.0.lock().unwrap();
        s.log.push(format!("alive {}", process.pid()));
        if s.process_unreadable {
            return Presence::Unknown;
        }
        if s.alive_polls > 0 {
            s.alive_polls -= 1;
            return Presence::Running;
        }
        Presence::Gone
    }
}

fn mac_disk() -> FakeFiles {
    let files = FakeFiles::new();
    files
        .put_file(SOURCE, b"helper build 1", 501, 0o755)
        .put_dir("/Library/PrivilegedHelperTools", 0, 0o755);
    files
}

/// A Windows machine as the install sees it: stock `C:\`, stock Program Files with the per-machine Dyno folder in it, the
/// helper's folder the way the ACL step leaves it (an `icacls` run is faked, so the folder is put here) and a per-user
/// profile whose folders the user can change.
fn win_trust() -> crate::privileged::trust_fake::FakePort {
    use crate::privileged::trust::{Ace, SID_ADMINISTRATORS, SID_SYSTEM};
    use crate::privileged::trust_fake::*;
    let mut p = windows_machine(|_| locked());
    p.put(
        "C:\\Program Files",
        from_sddl(FileKind::Dir, SDDL_PROGRAM_FILES),
    )
    .put(
        "C:\\Program Files\\Dyno",
        from_sddl(FileKind::Dir, SDDL_PROGRAM_FILES_DYNO),
    )
    .put(
        WIN_SOURCE,
        file(from_sddl(FileKind::Dir, SDDL_PROGRAM_FILES_DYNO)),
    )
    .put(
        WIN_FOLDER,
        from_sddl(FileKind::Dir, SDDL_PROGRAM_FILES_DYNO),
    )
    .put(
        WIN_PROGRAM,
        file(from_sddl(FileKind::Dir, SDDL_PROGRAM_FILES_DYNO)),
    );
    let user = "S-1-5-21-1-2-3-1001";
    let mine = || {
        win(
            user,
            vec![
                Ace::allow(SID_SYSTEM, FULL),
                Ace::allow(SID_ADMINISTRATORS, FULL),
                Ace::allow(user, FULL),
            ],
        )
    };
    for dir in [
        "C:\\Users",
        "C:\\Users\\Ana",
        "C:\\Users\\Ana\\AppData",
        "C:\\Users\\Ana\\AppData\\Local",
        "C:\\Users\\Ana\\AppData\\Local\\Dyno",
    ] {
        p.put(dir, mine());
    }
    p.put(WIN_USER_SOURCE, file(mine()));
    p
}

fn run_install(
    layout: &Layout,
    files: &FakeFiles,
    backend: &Scripted,
    sys: &FakeSystem,
    source: &str,
) -> Result<Report, InstallError> {
    let (mut f, mut b, mut s) = (files.clone(), backend.clone(), sys.clone());
    let mut trust = win_trust();
    let installer = Installer::new(layout, &mut f, &mut b, &mut s);
    if layout.os == Os::Windows {
        installer.with_trust(&mut trust).install(Path::new(source))
    } else {
        installer.install(Path::new(source))
    }
}

fn run_uninstall(
    layout: &Layout,
    files: &FakeFiles,
    backend: &Scripted,
) -> Result<Report, InstallError> {
    let (mut f, mut b, mut s) = (files.clone(), backend.clone(), FakeSystem::new());
    Installer::new(layout, &mut f, &mut b, &mut s).uninstall()
}

fn phases(r: &Report) -> Vec<Step> {
    r.phases.iter().map(|p| p.phase).collect()
}

#[test]
fn the_plans_are_pure_and_differ_per_os() {
    let (mac, win, lin) = (
        layout_for(Os::MacOs),
        layout_for(Os::Windows),
        layout_for(Os::Linux),
    );
    use Step::*;
    assert_eq!(
        plan_install(&mac, false),
        [EnsureFolder, CopyProgram, VerifyProgram, Register, Confirm]
    );
    assert_eq!(
        plan_install(&mac, true),
        [
            StopExisting,
            EnsureFolder,
            CopyProgram,
            VerifyProgram,
            Register,
            Confirm
        ]
    );
    assert_eq!(
        plan_install(&win, false),
        [
            EnsureFolder,
            ApplyAcl,
            CopyProgram,
            VerifyProgram,
            Register,
            Confirm
        ]
    );
    assert_eq!(
        plan_install(&lin, false),
        [VerifyProgram, Register, Confirm]
    );
    assert_eq!(
        plan_uninstall(&mac),
        [StopExisting, RemoveProgram, RemoveFolder, ConfirmGone]
    );
    assert_eq!(plan_uninstall(&lin), [StopExisting, ConfirmGone]);
}

#[test]
fn a_fresh_macos_install_copies_checks_registers_and_confirms() {
    let (files, backend) = (
        mac_disk(),
        Scripted::with(|s| s.starts_after_polls = Some(0)),
    );
    let report = run_install(
        &layout_for(Os::MacOs),
        &files,
        &backend,
        &FakeSystem::new(),
        SOURCE,
    )
    .unwrap();
    assert_eq!(
        phases(&report),
        [
            Step::EnsureFolder,
            Step::CopyProgram,
            Step::VerifyProgram,
            Step::Register,
            Step::Confirm
        ]
    );
    assert!(!report.replaced_existing);
    assert_eq!(report.sha256.as_deref().map(str::len), Some(64));
    let copy = files.facts_of(Path::new(MAC_PROGRAM)).unwrap();
    assert_eq!((copy.owner, copy.mode), (Some(0), Some(0o755)));
    assert!(report.phases.iter().all(|p| !p.detail.is_empty()));
    // The copy is made before anything is registered.
    let calls = files.calls();
    let copy_at = calls.iter().position(|c| c.starts_with("write ")).unwrap();
    assert!(calls[..copy_at].iter().all(|c| !c.starts_with("remove")));
    assert_eq!(backend.log().iter().filter(|l| *l == "register").count(), 1);
}

#[test]
fn an_update_replaces_the_registration_and_waits_for_the_old_job() {
    let files = mac_disk();
    files.put_file(MAC_PROGRAM, b"helper build 0", 0, 0o755);
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.running = true;
        s.job_held_polls = 2;
        s.starts_after_polls = Some(1);
    });
    let report = run_install(
        &layout_for(Os::MacOs),
        &files,
        &backend,
        &FakeSystem::new(),
        SOURCE,
    )
    .unwrap();
    assert!(report.replaced_existing);
    assert_eq!(report.phases[0].phase, Step::StopExisting);
    let log = backend.log();
    let evict = log.iter().position(|l| l == "evict").unwrap();
    let unregister = log.iter().position(|l| l == "unregister").unwrap();
    let register = log.iter().position(|l| l == "register").unwrap();
    assert!(evict < unregister && unregister < register);
    assert_eq!(
        log.iter().filter(|l| *l == "pause").count(),
        3,
        "two waits for the old job, one for the new one"
    );
    assert_eq!(
        files
            .calls()
            .iter()
            .filter(|c| c.starts_with("write "))
            .count(),
        1
    );
}

#[test]
fn a_missing_system_folder_is_created_with_its_mode_and_checked() {
    let files = FakeFiles::new();
    files.put_file(SOURCE, b"x", 501, 0o755);
    let backend = Scripted::with(|s| s.starts_after_polls = Some(0));
    let report = run_install(
        &layout_for(Os::MacOs),
        &files,
        &backend,
        &FakeSystem::new(),
        SOURCE,
    )
    .unwrap();
    assert_eq!(report.phases[0].detail, "created");
    assert!(
        files
            .calls()
            .contains(&"create_dir /Library/PrivilegedHelperTools 1755".to_string())
    );
}

#[test]
fn nothing_is_copied_into_a_folder_anyone_can_change_or_from_an_unsafe_source() {
    let layout = layout_for(Os::MacOs);
    let go = |files: &FakeFiles, source: &str| {
        run_install(
            &layout,
            files,
            &Scripted::default(),
            &FakeSystem::new(),
            source,
        )
    };
    let refused = |r: Result<Report, InstallError>, needle: &str| match r {
        Err(InstallError::Refused(why)) => assert!(why.contains(needle), "{why}"),
        other => panic!("{other:?}"),
    };
    let files = mac_disk();
    files.put_dir("/Library/PrivilegedHelperTools", 0, 0o777);
    refused(go(&files, SOURCE), "lets the group or others change it");
    assert!(
        !files.calls().iter().any(|c| c.starts_with("write ")),
        "refused before copying"
    );
    let files = mac_disk();
    files.put_dir("/Library/PrivilegedHelperTools", 501, 0o755);
    refused(go(&files, SOURCE), "not root");
    let files = mac_disk();
    files.put_symlink("/Library/PrivilegedHelperTools");
    refused(go(&files, SOURCE), "not what the install expects");
    let files = mac_disk();
    refused(go(&files, "/nowhere/dyno-helper"), "is not there");
    files.put_symlink("/Applications/link");
    refused(go(&files, "/Applications/link"), "not a file");
    files.put_dir("/Applications/dir", 501, 0o755);
    refused(go(&files, "/Applications/dir"), "not a file");
    files.put_file(MAC_PROGRAM, b"x", 0, 0o755);
    refused(go(&files, MAC_PROGRAM), "already inside");
}

#[test]
fn a_copy_that_is_not_roots_or_changed_underneath_is_refused_and_unregistered() {
    let layout = layout_for(Os::MacOs);
    // Run without administrator rights: the copy belongs to the user, not to root.
    let files = mac_disk();
    files.new_files_belong_to(501);
    let backend = Scripted::default();
    let err = run_install(&layout, &files, &backend, &FakeSystem::new(), SOURCE).unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("user id 501")),
        "{err}"
    );
    assert!(!backend.log().contains(&"register".to_string()));
    let files = mac_disk();
    files.tamper(true);
    let err = run_install(
        &layout,
        &files,
        &Scripted::default(),
        &FakeSystem::new(),
        SOURCE,
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            InstallError::Failed {
                phase: Step::VerifyProgram,
                ..
            }
        ),
        "{err}"
    );
    assert!(err.to_string().contains("differs"));
    let files = mac_disk();
    files.fail_with(Some(std::io::ErrorKind::PermissionDenied));
    let err = run_install(
        &layout,
        &files,
        &Scripted::default(),
        &FakeSystem::new(),
        SOURCE,
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            InstallError::Failed {
                phase: Step::CopyProgram,
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn a_refused_registration_leaves_nothing_registered() {
    let layout = layout_for(Os::MacOs);
    for (error, expect) in [
        (RegisterError::NeedsElevation, InstallError::NeedsElevation),
        (RegisterError::Declined, InstallError::Declined),
        (
            RegisterError::Other("busy".into()),
            InstallError::Failed {
                phase: Step::Register,
                why: "busy".into(),
            },
        ),
    ] {
        let backend = Scripted::with(|s| s.register_error = Some(error));
        let err =
            run_install(&layout, &mac_disk(), &backend, &FakeSystem::new(), SOURCE).unwrap_err();
        assert_eq!(err, expect);
        assert!(!err.to_string().is_empty());
        let log = backend.log();
        assert_eq!(&log[log.len() - 2..], ["evict", "unregister"]);
    }
}

#[test]
fn a_clean_up_that_fails_after_a_refused_registration_is_logged_and_the_install_error_stays() {
    let layout = layout_for(Os::MacOs);
    let backend = Scripted::with(|s| {
        s.register_error = Some(RegisterError::Other("busy".into()));
        s.evict_error = Some("no such job".into());
        s.unregister_error = Some("plist locked".into());
    });
    let err = run_install(&layout, &mac_disk(), &backend, &FakeSystem::new(), SOURCE).unwrap_err();
    assert!(
        matches!(
            err,
            InstallError::Failed {
                phase: Step::Register,
                ..
            }
        ),
        "{err:?}"
    );
    let log = backend.log();
    assert_eq!(
        &log[log.len() - 2..],
        ["evict", "unregister"],
        "both clean-up steps were tried"
    );
}

#[test]
fn what_the_os_says_after_registering_decides_the_outcome() {
    let layout = layout_for(Os::MacOs);
    let outcome = |f: fn(&mut State)| {
        let backend = Scripted::with(f);
        let r = run_install(&layout, &mac_disk(), &backend, &FakeSystem::new(), SOURCE);
        (r, backend)
    };
    let why = |r: Result<Report, InstallError>| match r {
        Err(InstallError::Failed {
            phase: Step::Confirm,
            why,
        }) => why,
        other => panic!("{other:?}"),
    };
    let (r, b) = outcome(|_| {});
    assert!(why(r).contains("did not start in time"));
    assert!(
        b.log().iter().filter(|l| *l == "pause").count() >= 30,
        "bounded, virtual waits"
    );
    assert!(
        why(outcome(|s| s.registration_override = Some(Registration::NeedsApproval)).0)
            .contains("Login Items")
    );
    assert!(
        why(outcome(|s| s.registration_override = Some(Registration::SwitchedOff)).0)
            .contains("switched off")
    );
    assert!(
        why(outcome(|s| s.registration_override = Some(Registration::Absent)).0)
            .contains("no registration")
    );
    assert!(
        why(outcome(
            |s| s.registration_override = Some(Registration::Unavailable("no systemd".into()))
        )
        .0)
        .contains("no systemd")
    );
    assert!(why(outcome(|s| s.points_here = Some(false)).0).contains("another program"));
    assert!(why(outcome(|s| s.job_override = Some(Job::Refused)).0).contains("refuses"));
    let (r, b) = outcome(|s| s.starts_after_polls = Some(5));
    assert!(r.is_ok());
    assert!(b.log().contains(&"pause".to_string()));
}

#[test]
fn a_service_that_stops_with_an_error_says_how_it_ended() {
    let layout = layout_for(Os::MacOs);
    let backend = Scripted::with(|s| {
        s.job_override = Some(Job::Failed(crate::backend::LastExit::ServiceSpecific(3)));
    });
    let r = run_install(&layout, &mac_disk(), &backend, &FakeSystem::new(), SOURCE);
    match r {
        Err(InstallError::Failed {
            phase: Step::Confirm,
            why,
        }) => assert!(
            why.contains("service-specific exit code 3"),
            "the cause reaches the user: {why}"
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn confirm_judges_the_inspection_register_returned_and_asks_the_os_again_only_to_wait() {
    let layout = layout_for(Os::MacOs);
    let backend = Scripted::with(|s| s.job_override = Some(Job::Running));
    run_install(&layout, &mac_disk(), &backend, &FakeSystem::new(), SOURCE).unwrap();
    assert_eq!(
        backend.log().iter().filter(|l| *l == "inspect").count(),
        1,
        "one look before (is anything registered?); the read-back after registering is not repeated"
    );
}

#[test]
fn stopping_an_older_registration_reports_what_it_could_not_do() {
    let layout = layout_for(Os::MacOs);
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.evict_error = Some("bootout: no such process".into());
        s.job_held_polls = 100;
        s.starts_after_polls = Some(0);
    });
    let report = run_install(&layout, &mac_disk(), &backend, &FakeSystem::new(), SOURCE).unwrap();
    let detail = &report.phases[0].detail;
    assert!(
        detail.contains("bootout") && detail.contains("still holds the job"),
        "{detail}"
    );
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.unregister_error = Some("plist locked".into());
    });
    let err = run_install(&layout, &mac_disk(), &backend, &FakeSystem::new(), SOURCE).unwrap_err();
    assert_eq!(
        err,
        InstallError::Failed {
            phase: Step::StopExisting,
            why: "plist locked".into()
        }
    );
}

const ICACLS_GOOD: &str = "C:\\Program Files\\Dyno Helper NT AUTHORITY\\SYSTEM:(OI)(CI)(F)\n BUILTIN\\Administrators:(OI)(CI)(F)\n BUILTIN\\Users:(OI)(CI)(RX)\n";

fn windows_disk() -> FakeFiles {
    let files = FakeFiles::windows();
    files.put_file(WIN_SOURCE, b"helper.exe", 0, 0);
    files.put_dir("C:\\Program Files", 0, 0);
    files
}

#[test]
fn windows_sets_and_checks_the_acl_before_the_copy() {
    let (files, sys) = (windows_disk(), FakeSystem::new());
    sys.answer(
        "icacls C:\\Program Files\\Dyno Helper /inheritance:r",
        Output::ok("Successfully processed 1 files"),
    );
    sys.answer(
        "icacls C:\\Program Files\\Dyno Helper",
        Output::ok(ICACLS_GOOD),
    );
    let backend = Scripted::with(|s| s.starts_after_polls = Some(0));
    let report = run_install(&layout_for(Os::Windows), &files, &backend, &sys, WIN_SOURCE).unwrap();
    assert_eq!(
        phases(&report),
        [
            Step::EnsureFolder,
            Step::ApplyAcl,
            Step::CopyProgram,
            Step::VerifyProgram,
            Step::Register,
            Step::Confirm
        ]
    );
    assert!(sys.commands()[0].contains("/grant:r *S-1-5-32-545:(OI)(CI)RX"));
    assert_eq!(files.facts_of(Path::new(WIN_PROGRAM)).unwrap().owner, None);
    assert!(files.facts_of(Path::new(WIN_FOLDER)).is_some());
}

#[test]
fn a_windows_acl_that_cannot_be_set_or_does_not_read_back_stops_before_the_copy() {
    let layout = layout_for(Os::Windows);
    let try_with = |setup: fn(&FakeSystem)| {
        let (files, sys) = (windows_disk(), FakeSystem::new());
        setup(&sys);
        let err = run_install(&layout, &files, &Scripted::default(), &sys, WIN_SOURCE).unwrap_err();
        assert!(!files.calls().iter().any(|c| c.starts_with("write ")));
        match err {
            InstallError::Failed {
                phase: Step::ApplyAcl,
                why,
            } => why,
            other => panic!("{other:?}"),
        }
    };
    assert!(
        try_with(|s| {
            s.answer(
                "icacls",
                Output {
                    code: Some(5),
                    stderr: "Access is denied.".into(),
                    ..Output::default()
                },
            );
        })
        .contains("Access is denied")
    );
    assert!(
        try_with(|s| {
            s.on("icacls", |_, _| {
                Err(RunError::NotStarted("icacls: not found".into()))
            });
        })
        .contains("not found")
    );
    assert!(
        try_with(|s| {
            s.answer(
                "icacls C:\\Program Files\\Dyno Helper /inheritance:r",
                Output::code(0),
            );
            s.answer(
                "icacls C:\\Program Files\\Dyno Helper",
                Output::ok("Everyone:(F)\n"),
            );
        })
        .contains("access entries")
    );
    assert!(
        try_with(|s| {
            s.on("icacls", |w, _| {
                if w.len() > 2 {
                    Ok(Output::code(0))
                } else {
                    Err(RunError::TimedOut("icacls".into()))
                }
            });
        })
        .contains("in time")
    );
}

#[test]
fn linux_enables_the_packaged_unit_and_never_copies() {
    let layout = layout_for(Os::Linux);
    let files = FakeFiles::new();
    let backend = Scripted::with(|s| s.starts_after_polls = Some(0));
    let missing = run_install(
        &layout,
        &files,
        &backend,
        &FakeSystem::new(),
        "/usr/bin/dyno-helper",
    )
    .unwrap_err();
    assert!(
        matches!(
            &missing,
            InstallError::Failed {
                phase: Step::VerifyProgram,
                ..
            }
        ) || matches!(&missing, InstallError::Refused(_))
    );
    files.put_file("/usr/bin/dyno-helper", b"deb", 0, 0o755);
    let report = run_install(
        &layout,
        &files,
        &backend,
        &FakeSystem::new(),
        "/usr/bin/dyno-helper",
    )
    .unwrap();
    assert_eq!(
        phases(&report),
        [Step::VerifyProgram, Step::Register, Step::Confirm]
    );
    assert!(report.sha256.is_none());
    assert!(files.calls().iter().all(|c| !c.starts_with("write ")));
    files.put_file("/usr/bin/dyno-helper", b"deb", 501, 0o755);
    let err = run_install(
        &layout,
        &files,
        &Scripted::default(),
        &FakeSystem::new(),
        "/usr/bin/dyno-helper",
    )
    .unwrap_err();
    assert!(matches!(err, InstallError::Refused(_)));
}

#[test]
fn uninstall_removes_the_registration_the_copy_and_only_its_own_folder() {
    let files = mac_disk();
    files.put_file(MAC_PROGRAM, b"x", 0, 0o755);
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.running = true;
    });
    let report = run_uninstall(&layout_for(Os::MacOs), &files, &backend).unwrap();
    assert_eq!(
        phases(&report),
        [
            Step::StopExisting,
            Step::RemoveProgram,
            Step::RemoveFolder,
            Step::ConfirmGone
        ]
    );
    assert!(files.facts_of(Path::new(MAC_PROGRAM)).is_none());
    assert!(
        files
            .facts_of(Path::new("/Library/PrivilegedHelperTools"))
            .is_some(),
        "the OS's folder stays"
    );
    // Again: everything is already gone, which is the goal.
    let again = run_uninstall(&layout_for(Os::MacOs), &files, &backend).unwrap();
    assert!(again.phases.iter().any(|p| p.detail == "was not there"));

    let win = windows_disk();
    win.put_dir(WIN_FOLDER, 0, 0)
        .put_file(WIN_PROGRAM, b"x", 0, 0);
    let report = run_uninstall(&layout_for(Os::Windows), &win, &Scripted::default()).unwrap();
    assert!(win.facts_of(Path::new(WIN_FOLDER)).is_none() && report.phases[2].detail == "removed");
    let win = windows_disk();
    win.put_dir(WIN_FOLDER, 0, 0)
        .put_file(format!("{WIN_FOLDER}\\other.dll"), b"x", 0, 0);
    let report = run_uninstall(&layout_for(Os::Windows), &win, &Scripted::default()).unwrap();
    assert!(report.phases[2].detail.starts_with("left in place"));
    let win = windows_disk();
    let report = run_uninstall(&layout_for(Os::Windows), &win, &Scripted::default()).unwrap();
    assert_eq!(report.phases[2].detail, "was not there");
}

#[test]
fn uninstall_fails_loudly_when_the_os_keeps_the_job() {
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.unregister_error = Some("denied".into());
    });
    let err = run_uninstall(&layout_for(Os::MacOs), &mac_disk(), &backend).unwrap_err();
    assert!(matches!(
        err,
        InstallError::Failed {
            phase: Step::StopExisting,
            ..
        }
    ));
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.running = true;
        s.job_override = Some(Job::Running);
        s.registration_override = Some(Registration::Registered);
    });
    let err = run_uninstall(&layout_for(Os::MacOs), &mac_disk(), &backend).unwrap_err();
    assert!(matches!(
        err,
        InstallError::Failed {
            phase: Step::ConfirmGone,
            ..
        }
    ));
    // A packaged unit that is switched off counts as gone.
    let backend = Scripted::with(|s| s.registration_override = Some(Registration::SwitchedOff));
    assert!(run_uninstall(&layout_for(Os::Linux), &FakeFiles::new(), &backend).is_ok());
    // A copy that cannot be removed is an error, not a silent leftover.
    let files = mac_disk();
    files.put_file(MAC_PROGRAM, b"x", 0, 0o755);
    struct Locked(FakeFiles);
    impl ProtectedFiles for Locked {
        fn facts(&mut self, p: &Path) -> std::io::Result<Option<crate::privileged::FileFacts>> {
            self.0.clone().facts(p)
        }
        fn create_dir(&mut self, p: &Path, m: u32) -> std::io::Result<()> {
            self.0.create_dir(p, m)
        }
        fn open_source(
            &mut self,
            p: &Path,
            m: u64,
        ) -> std::io::Result<Option<crate::privileged::HeldSource>> {
            self.0.open_source(p, m)
        }
        fn write_program(&mut self, b: &[u8], to: &Path, m: u32) -> std::io::Result<String> {
            self.0.write_program(b, to, m)
        }
        fn sha256(&mut self, p: &Path) -> std::io::Result<String> {
            self.0.sha256(p)
        }
        fn read_checked(
            &mut self,
            p: &Path,
            m: u64,
        ) -> std::io::Result<Option<(crate::privileged::FileFacts, String)>> {
            self.0.read_checked(p, m)
        }
        fn remove_file(&mut self, _: &Path) -> std::io::Result<()> {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        }
        fn remove_dir(&mut self, p: &Path) -> std::io::Result<()> {
            self.0.remove_dir(p)
        }
    }
    let layout = layout_for(Os::MacOs);
    let (mut locked, mut b, mut s) = (Locked(files), Scripted::default(), FakeSystem::new());
    let err = Installer::new(&layout, &mut locked, &mut b, &mut s)
        .uninstall()
        .unwrap_err();
    assert!(matches!(
        err,
        InstallError::Failed {
            phase: Step::RemoveProgram,
            ..
        }
    ));
}

#[test]
fn errors_read_as_sentences_and_a_layout_without_a_copy_refuses_copy_steps() {
    for e in [
        InstallError::Refused("x".into()),
        InstallError::NeedsElevation,
        InstallError::Declined,
        InstallError::Failed {
            phase: Step::Register,
            why: "x".into(),
        },
    ] {
        assert!(
            e.to_string().ends_with('.') || e.to_string().contains("log"),
            "{e}"
        );
    }
    let layout = layout_for(Os::Linux);
    let (mut f, mut b, mut s) = (FakeFiles::new(), Scripted::default(), FakeSystem::new());
    let mut installer =
        Installer::new(&layout, &mut f, &mut b, &mut s).with_waits(Waits::default());
    assert!(matches!(
        installer.run_step(Step::CopyProgram, None),
        Err(InstallError::Refused(_))
    ));
    assert!(matches!(
        installer.run_step(Step::RemoveProgram, None),
        Err(InstallError::Refused(_))
    ));
    let layout = layout_for(Os::MacOs);
    let mut installer = Installer::new(&layout, &mut f, &mut b, &mut s);
    assert!(matches!(
        installer.run_step(Step::CopyProgram, None),
        Err(InstallError::Refused(_))
    ));
}

// The real backends of each OS over the recording fake: the commands the install would run.

#[test]
fn macos_registers_a_daemon_plist_that_runs_the_protected_copy() {
    let layout = layout_for(Os::MacOs);
    let sys = FakeSystem::new();
    sys.answer(
        "/bin/launchctl print system/net.decryptology.dyno.helper",
        Output::code(113),
    );
    let mut backend =
        crate::privileged::backend_for(&layout, &helper_spec(), Box::new(sys.clone())).unwrap();
    let (mut files, mut os) = (mac_disk(), FakeSystem::new());
    let err = Installer::new(&layout, &mut files, backend.as_mut(), &mut os)
        .with_waits(Waits {
            gone: Duration::ZERO,
            start: Duration::from_secs(2),
            poll: Duration::from_secs(1),
        })
        .install(Path::new(SOURCE))
        .unwrap_err();
    // The fake launchd reports no job, so the confirm step fails after the registration was made and undone.
    assert!(
        matches!(
            err,
            InstallError::Failed {
                phase: Step::Confirm,
                ..
            }
        ),
        "{err}"
    );
    let plist = PathBuf::from("/Library/LaunchDaemons").join("net.decryptology.dyno.helper.plist");
    let commands = sys.commands();
    assert!(
        commands.contains(&format!(
            "/bin/launchctl bootstrap system {}",
            plist.display()
        )),
        "{commands:?}"
    );
    assert!(
        commands
            .iter()
            .any(|c| c.starts_with("/bin/launchctl bootout system/"))
    );
}

#[test]
fn windows_creates_the_service_with_one_privilege_and_a_start_dacl() {
    let layout = layout_for(Os::Windows);
    let sys = FakeSystem::new();
    sys.answer(
        "sc qprivs DynoHelper",
        Output::ok("SERVICE_NAME: DynoHelper\n PRIVILEGES : SeChangeNotifyPrivilege\n"),
    );
    sys.answer("sc query", Output::ok("STATE : 4  RUNNING"));
    sys.answer(
        "sc qc",
        Output::ok(&format!(
            "START_TYPE : 2 AUTO_START\n BINARY_PATH_NAME : \"{WIN_PROGRAM}\" --service\n"
        )),
    );
    let mut backend =
        crate::privileged::backend_for(&layout, &helper_spec(), Box::new(sys.clone())).unwrap();
    let files = windows_disk();
    let os = FakeSystem::new();
    os.answer(
        "icacls C:\\Program Files\\Dyno Helper",
        Output::ok(ICACLS_GOOD),
    );
    let (mut f, mut o, mut trust) = (files.clone(), os.clone(), win_trust());
    let report = Installer::new(&layout, &mut f, backend.as_mut(), &mut o)
        .with_trust(&mut trust)
        .install(Path::new(WIN_SOURCE));
    assert!(report.is_ok(), "{report:?}");
    let commands = sys.commands();
    let at = |prefix: &str| {
        commands
            .iter()
            .position(|c| c.starts_with(prefix))
            .unwrap_or_else(|| panic!("{prefix}: {commands:?}"))
    };
    assert!(at("sc create DynoHelper") < at("sc privs DynoHelper SeChangeNotifyPrivilege"));
    assert!(at("sc privs") < at("sc sdset DynoHelper D:("));
    assert!(at("sc sdset") < at("sc failure") && at("sc qprivs") < at("sc start DynoHelper"));
    assert!(commands.iter().any(|c| c.contains("(A;;LCRPLORC;;;IU)")));
}

#[test]
fn windows_refuses_to_start_a_service_that_holds_more_privileges() {
    let layout = layout_for(Os::Windows);
    let sys = FakeSystem::new();
    sys.answer(
        "sc qprivs DynoHelper",
        Output::ok("PRIVILEGES : SeChangeNotifyPrivilege\n SeDebugPrivilege\n"),
    );
    let mut backend =
        crate::privileged::backend_for(&layout, &helper_spec(), Box::new(sys.clone())).unwrap();
    let err = backend.register().unwrap_err();
    assert!(
        matches!(&err, RegisterError::Other(w) if w.contains("SeDebugPrivilege") && w.contains("exactly")),
        "{err}"
    );
    assert!(!sys.commands().iter().any(|c| c.starts_with("sc start")));
    sys.answer("sc qprivs DynoHelper", Output::code(5));
    assert_eq!(backend.register(), Err(RegisterError::NeedsElevation));
    sys.on("sc qprivs", |_, _| Err(RunError::TimedOut("sc".into())));
    assert!(backend.register().is_err());
}

#[test]
fn linux_enables_the_unit_the_package_shipped_and_leaves_the_file_alone() {
    let layout = layout_for(Os::Linux);
    let sys = FakeSystem::new();
    // The package put the unit there, disabled; `enable --now` is judged by reading it back.
    sys.put_file(
        "/usr/lib/systemd/system/dyno-helper.service",
        &format!(
            "[Service]\nExecStart={} --service\n",
            layout.program.display()
        ),
    );
    sys.put_program(layout.program.clone());
    let mut backend =
        crate::privileged::backend_for(&layout, &helper_spec(), Box::new(sys.clone())).unwrap();
    backend.register().unwrap();
    assert_eq!(
        sys.commands()
            .into_iter()
            .filter(|c| !c.contains(" show ") && !c.contains("is-"))
            .collect::<Vec<_>>(),
        ["systemctl enable --now dyno-helper.service"]
    );
    sys.clear_calls();
    // Turned off, the packaged unit stays on disk but reads back disabled and stopped.
    sys.answer("systemctl is-active", Output::ok("inactive\n"));
    sys.answer("systemctl is-enabled", Output::ok("disabled\n"));
    backend.unregister().unwrap();
    assert_eq!(
        sys.commands()
            .into_iter()
            .filter(|c| !c.contains(" show ") && !c.contains("is-"))
            .collect::<Vec<_>>(),
        ["systemctl disable --now dyno-helper.service"]
    );
    assert!(sys.calls().iter().all(|c| !matches!(
        c,
        crate::testing::Call::Write(..) | crate::testing::Call::Remove(..)
    )));
    // The packaged unit is read, not compared with what this copy would write.
    let unit = PathBuf::from("/usr/lib/systemd/system/dyno-helper.service");
    sys.put_file(
        &unit,
        "[Service]\nExecStart=\"/usr/bin/dyno-helper\" \"serve\"\n",
    );
    sys.answer("systemctl is-active", Output::ok("active\n"));
    sys.answer("systemctl is-enabled", Output::ok("enabled\n"));
    let look = backend.inspect();
    assert_eq!(
        (look.registration, look.job, look.definition_current),
        (Registration::Registered, Job::Running, None)
    );
}

#[test]
fn a_service_removing_itself_deletes_its_files_first_and_stops_last() {
    use Step::*;
    assert_eq!(
        plan_uninstall_self(&layout_for(Os::MacOs)),
        [RemoveProgram, RemoveFolder, StopExisting, ConfirmGone]
    );
    assert_eq!(
        plan_uninstall_self(&layout_for(Os::Linux)),
        [StopExisting, ConfirmGone]
    );
    let files = mac_disk();
    files.put_file(MAC_PROGRAM, b"x", 0, 0o755);
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.running = true;
    });
    let layout = layout_for(Os::MacOs);
    let (mut f, mut b, mut s) = (files.clone(), backend.clone(), FakeSystem::new());
    let report = Installer::new(&layout, &mut f, &mut b, &mut s)
        .uninstall_self()
        .unwrap();
    assert_eq!(
        phases(&report),
        [RemoveProgram, RemoveFolder, StopExisting, ConfirmGone]
    );
    assert!(files.facts_of(Path::new(MAC_PROGRAM)).is_none());
    assert_eq!(
        backend
            .log()
            .iter()
            .find(|l| *l == "evict" || *l == "unregister"),
        Some(&"unregister".to_string()),
        "the registration goes before the stop that ends this process"
    );
}

#[test]
fn a_program_the_os_will_not_delete_while_it_runs_is_left_and_the_registration_still_goes() {
    struct Busy(FakeFiles);
    impl ProtectedFiles for Busy {
        fn facts(&mut self, p: &Path) -> std::io::Result<Option<crate::privileged::FileFacts>> {
            self.0.clone().facts(p)
        }
        fn create_dir(&mut self, p: &Path, m: u32) -> std::io::Result<()> {
            self.0.create_dir(p, m)
        }
        fn open_source(
            &mut self,
            p: &Path,
            m: u64,
        ) -> std::io::Result<Option<crate::privileged::HeldSource>> {
            self.0.open_source(p, m)
        }
        fn write_program(&mut self, b: &[u8], to: &Path, m: u32) -> std::io::Result<String> {
            self.0.write_program(b, to, m)
        }
        fn sha256(&mut self, p: &Path) -> std::io::Result<String> {
            self.0.sha256(p)
        }
        fn read_checked(
            &mut self,
            p: &Path,
            m: u64,
        ) -> std::io::Result<Option<(crate::privileged::FileFacts, String)>> {
            self.0.read_checked(p, m)
        }
        fn remove_file(&mut self, _: &Path) -> std::io::Result<()> {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        }
        fn remove_dir(&mut self, p: &Path) -> std::io::Result<()> {
            self.0.remove_dir(p)
        }
    }
    let layout = layout_for(Os::MacOs);
    let backend = Scripted::with(|s| s.registered = true);
    let (mut f, mut b, mut s) = (Busy(mac_disk()), backend.clone(), FakeSystem::new());
    let report = Installer::new(&layout, &mut f, &mut b, &mut s)
        .uninstall_self()
        .unwrap();
    assert!(
        report.phases[0]
            .detail
            .starts_with("left in place while it runs")
    );
    assert!(backend.log().contains(&"unregister".to_string()));
}

fn helper_process() -> paddock_proc::ProcessId {
    paddock_proc::ProcessId::new(
        4242,
        paddock_proc::StartTime::from_micros(1_700_000_000_000_000),
    )
}

#[test]
fn a_stop_waits_until_the_running_process_has_ended_before_anything_is_removed() {
    // Measured in the Windows 11 VM: the service entry was gone while the process still held its program, so the removal
    // failed with "Access is denied". The wait is on the process's identity (pid and start time), not on the service entry.
    let files = mac_disk();
    files.put_file(MAC_PROGRAM, b"x", 0, 0o755);
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.running = true;
        s.process = Some(helper_process());
        s.alive_polls = 3;
    });
    let report = run_uninstall(&layout_for(Os::MacOs), &files, &backend).unwrap();
    assert_eq!(phases(&report)[0], Step::StopExisting);
    let log = backend.log();
    assert_eq!(
        log.iter().filter(|l| l.starts_with("alive 4242")).count(),
        4,
        "asked until it ended: {log:?}"
    );
    assert!(log.iter().filter(|l| *l == "pause").count() >= 3, "{log:?}");
    assert!(files.facts_of(Path::new(MAC_PROGRAM)).is_none());
}

#[test]
fn a_process_that_does_not_end_is_an_error_that_names_it_and_removes_nothing() {
    let files = mac_disk();
    files.put_file(MAC_PROGRAM, b"x", 0, 0o755);
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.running = true;
        s.process = Some(helper_process());
        s.alive_polls = u32::MAX;
    });
    let err = run_uninstall(&layout_for(Os::MacOs), &files, &backend).unwrap_err();
    assert!(
        matches!(&err, InstallError::Failed { phase: Step::StopExisting, why } if why.contains("4242") && why.contains("still running")),
        "{err:?}"
    );
    assert!(
        files.facts_of(Path::new(MAC_PROGRAM)).is_some(),
        "the program stays"
    );
}

#[test]
fn an_unreadable_process_table_is_not_a_stopped_process_and_removes_nothing() {
    let files = mac_disk();
    files.put_file(MAC_PROGRAM, b"x", 0, 0o755);
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.running = true;
        s.process = Some(helper_process());
        s.process_unreadable = true;
    });
    let err = run_uninstall(&layout_for(Os::MacOs), &files, &backend).unwrap_err();
    assert!(
        matches!(&err, InstallError::Failed { phase: Step::StopExisting, why } if why.contains("4242") && why.contains("could not be checked")),
        "{err:?}"
    );
    assert!(
        files.facts_of(Path::new(MAC_PROGRAM)).is_some(),
        "the program stays"
    );
}

#[test]
fn a_service_removing_itself_unregisters_before_it_stops_because_the_stop_ends_the_process_running_the_steps()
 {
    // Measured in a macOS VM: `launchctl bootout` of its own job ended the helper before the plist was removed, so the
    // definition stayed behind. Taking the registration away first leaves nothing to restart and nothing behind.
    let files = mac_disk();
    files.put_file(MAC_PROGRAM, b"x", 0, 0o755);
    let backend = Scripted::with(|s| {
        s.registered = true;
        s.running = true;
        s.process = Some(helper_process());
        s.alive_polls = 5;
    });
    let (mut f, mut b, mut sys) = (files.clone(), backend.clone(), FakeSystem::new());
    Installer::new(&layout_for(Os::MacOs), &mut f, &mut b, &mut sys)
        .uninstall_self()
        .unwrap();
    let log = backend.log();
    let at = |what: &str| {
        log.iter()
            .position(|l| l == what)
            .unwrap_or_else(|| panic!("{what} not in {log:?}"))
    };
    assert!(at("unregister") < at("evict"), "{log:?}");
    assert!(
        log.iter().all(|l| !l.starts_with("alive")),
        "it does not wait for its own process: {log:?}"
    );
}

// ---- the trust of the copy (security review findings F1 and F2) ----

fn win_install_with(
    trust: &mut crate::privileged::trust_fake::FakePort,
    files: &FakeFiles,
    source: &str,
) -> Result<Report, InstallError> {
    let sys = FakeSystem::new();
    sys.answer(
        "icacls C:\\Program Files\\Dyno Helper /inheritance:r",
        Output::ok("Successfully processed 1 files"),
    );
    sys.answer(
        "icacls C:\\Program Files\\Dyno Helper",
        Output::ok(ICACLS_GOOD),
    );
    let backend = Scripted::with(|s| s.starts_after_polls = Some(0));
    let (mut f, mut b, mut s) = (files.clone(), backend, sys);
    Installer::new(&layout_for(Os::Windows), &mut f, &mut b, &mut s)
        .with_trust(trust)
        .install(Path::new(source))
}

#[test]
fn a_windows_install_without_a_trust_check_is_refused() {
    let (files, sys) = (windows_disk(), FakeSystem::new());
    let (mut f, mut b, mut s) = (files.clone(), Scripted::default(), sys);
    let err = Installer::new(&layout_for(Os::Windows), &mut f, &mut b, &mut s)
        .install(Path::new(WIN_SOURCE))
        .unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("no trust check")),
        "{err:?}"
    );
    assert!(files.facts_of(Path::new(WIN_PROGRAM)).is_none());
}

#[test]
fn a_per_user_source_is_refused_on_windows_with_the_next_step_and_nothing_is_copied() {
    // The user can replace a per-user copy between the check and the copy: the chain to it must be administrators' only.
    let files = windows_disk();
    files.put_file(WIN_USER_SOURCE, b"helper.exe", 0, 0);
    let err = win_install_with(&mut win_trust(), &files, WIN_USER_SOURCE).unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("reinstall Dyno for all users")),
        "{err:?}"
    );
    assert!(files.facts_of(Path::new(WIN_PROGRAM)).is_none());
    assert!(!files.calls().iter().any(|c| c.starts_with("write ")));
}

#[test]
fn a_program_files_folder_users_may_delete_from_is_refused_before_anything_is_made() {
    use crate::privileged::trust::{Ace, SID_ADMINISTRATORS, SID_SYSTEM, SID_USERS, right};
    use crate::privileged::trust_fake::*;
    let files = windows_disk();
    let mut trust = win_trust();
    trust.put(
        "C:\\Program Files",
        win(
            SID_ADMINISTRATORS,
            vec![
                Ace::allow(SID_SYSTEM, FULL),
                Ace::allow(SID_ADMINISTRATORS, FULL),
                Ace::allow(SID_USERS, RX | right::DELETE_CHILD),
            ],
        ),
    );
    // The source is somewhere administrators-only that is not below Program Files, so only the install's own check of Program
    // Files can refuse this.
    files.put_file("C:\\Tools\\dyno-helper.exe", b"helper.exe", 0, 0);
    trust
        .put("C:\\Tools", locked())
        .put("C:\\Tools\\dyno-helper.exe", file(locked()));
    let err = win_install_with(&mut trust, &files, "C:\\Tools\\dyno-helper.exe").unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("Program Files") && w.contains("FILE_DELETE_CHILD")),
        "{err:?}"
    );
    assert!(!files.calls().iter().any(|c| c.starts_with("create_dir")));
}

#[test]
fn a_folder_the_acl_step_left_changeable_by_users_is_refused_even_when_icacls_listed_it_right() {
    // `icacls` printed the expected three entries, but the real descriptor (read from the open folder) lets Users write: the
    // trust rule, not the listing, decides. Windows reports no owner or mode, so `verify_protected` alone passes it.
    use crate::privileged::trust::{Ace, SID_ADMINISTRATORS, SID_SYSTEM, SID_USERS};
    use crate::privileged::trust_fake::*;
    let files = windows_disk();
    let mut trust = win_trust();
    trust.put(
        WIN_FOLDER,
        win(
            SID_ADMINISTRATORS,
            vec![
                Ace::allow(SID_SYSTEM, FULL),
                Ace::allow(SID_ADMINISTRATORS, FULL),
                Ace::allow(SID_USERS, MODIFY),
            ],
        ),
    );
    let err = win_install_with(&mut trust, &files, WIN_SOURCE).unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("protected folder") && w.contains(SID_USERS)),
        "{err:?}"
    );
    assert!(!files.calls().iter().any(|c| c.starts_with("write ")));
}

#[test]
fn a_copy_that_users_can_write_is_refused_after_it_was_written() {
    use crate::privileged::trust::{Ace, SID_ADMINISTRATORS, SID_USERS};
    use crate::privileged::trust_fake::*;
    let files = windows_disk();
    let mut trust = win_trust();
    trust.put(
        WIN_PROGRAM,
        file(win(
            SID_ADMINISTRATORS,
            vec![
                Ace::allow(SID_ADMINISTRATORS, FULL),
                Ace::allow(SID_USERS, MODIFY),
            ],
        )),
    );
    let err = win_install_with(&mut trust, &files, WIN_SOURCE).unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("installed program")),
        "{err:?}"
    );
}

/// Accepts exactly one program with exactly one signature.
struct ExpectSignature {
    program: Vec<u8>,
    signature: Vec<u8>,
}

impl SourceVerifier for ExpectSignature {
    fn verify(&self, program: &[u8], signature: &[u8]) -> Result<String, String> {
        if signature != self.signature {
            Err("the signature is not the one this program was signed with; reinstall Dyno".into())
        } else if program != self.program {
            Err("the program does not match its signature; reinstall Dyno".into())
        } else {
            Ok("valid".into())
        }
    }
}

const SIG: &str = "/Applications/Dyno.app/Contents/Resources/dyno-helper.minisig";

fn mac_install_signed(
    files: &FakeFiles,
    verifier: &ExpectSignature,
) -> Result<Report, InstallError> {
    let (mut f, mut b, mut s) = (
        files.clone(),
        Scripted::with(|s| s.starts_after_polls = Some(0)),
        FakeSystem::new(),
    );
    Installer::new(&layout_for(Os::MacOs), &mut f, &mut b, &mut s)
        .with_signature(SignatureCheck {
            verifier,
            locations: vec![
                PathBuf::from("/Applications/Dyno.app/Contents/MacOS/dyno-helper.minisig"),
                PathBuf::from(SIG),
            ],
        })
        .install(Path::new(SOURCE))
}

#[test]
fn a_helper_with_the_right_signature_is_copied_and_the_verdict_is_reported() {
    let files = mac_disk();
    files.put_file(SIG, b"sig-1", 501, 0o644);
    let v = ExpectSignature {
        program: b"helper build 1".to_vec(),
        signature: b"sig-1".to_vec(),
    };
    let report = mac_install_signed(&files, &v).unwrap();
    assert_eq!(report.signature.as_deref(), Some("valid"));
    assert_eq!(
        report.sha256.as_deref(),
        Some(crate::privileged::files::hash_bytes(b"helper build 1").as_str())
    );
}

#[test]
fn a_helper_with_a_bad_or_missing_signature_is_refused_and_nothing_is_written() {
    let v = ExpectSignature {
        program: b"helper build 1".to_vec(),
        signature: b"sig-1".to_vec(),
    };
    // A bad signature.
    let files = mac_disk();
    files.put_file(SIG, b"forged", 501, 0o644);
    let err = mac_install_signed(&files, &v).unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("reinstall Dyno")),
        "{err:?}"
    );
    assert!(
        !files.calls().iter().any(|c| c.starts_with("write ")),
        "{:?}",
        files.calls()
    );
    assert!(files.facts_of(Path::new(MAC_PROGRAM)).is_none());
    // A program that is not the signed one.
    let files = mac_disk();
    files.put_file(SOURCE, b"swapped helper", 501, 0o755);
    files.put_file(SIG, b"sig-1", 501, 0o644);
    let err = mac_install_signed(&files, &v).unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("does not match")),
        "{err:?}"
    );
    assert!(files.facts_of(Path::new(MAC_PROGRAM)).is_none());
    // No signature file at all.
    let files = mac_disk();
    let err = mac_install_signed(&files, &v).unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("no signature file") && w.contains("reinstall Dyno")),
        "{err:?}"
    );
    assert!(files.facts_of(Path::new(MAC_PROGRAM)).is_none());
}

#[test]
fn the_bytes_that_are_signed_are_the_bytes_that_are_copied_even_if_the_path_is_swapped_after_the_check()
 {
    // The user replaces the helper after it was checked. A copy that reopens the path would install the replacement; the
    // installer writes the bytes it read through the handle it checked.
    let files = mac_disk();
    files.put_file(SIG, b"sig-1", 501, 0o644);
    files.swap_source_after_open(b"evil helper");
    let v = ExpectSignature {
        program: b"helper build 1".to_vec(),
        signature: b"sig-1".to_vec(),
    };
    let report = mac_install_signed(&files, &v).unwrap();
    assert_eq!(
        report.sha256.as_deref(),
        Some(crate::privileged::files::hash_bytes(b"helper build 1").as_str())
    );
    assert_eq!(
        files.content_of(Path::new(MAC_PROGRAM)).as_deref(),
        Some(&b"helper build 1"[..])
    );
}

#[test]
fn a_copy_without_a_checked_source_is_refused() {
    let layout = layout_for(Os::MacOs);
    let (mut f, mut b, mut s) = (mac_disk(), Scripted::default(), FakeSystem::new());
    let mut installer = Installer::new(&layout, &mut f, &mut b, &mut s);
    let err = installer
        .run_step(Step::CopyProgram, Some(Path::new(SOURCE)))
        .unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("was not checked")),
        "{err:?}"
    );
}

#[test]
fn a_signature_location_that_cannot_be_read_is_skipped_and_the_next_one_is_used() {
    struct DeniesFirst(FakeFiles);
    impl ProtectedFiles for DeniesFirst {
        fn facts(&mut self, p: &Path) -> std::io::Result<Option<crate::privileged::FileFacts>> {
            self.0.clone().facts(p)
        }
        fn create_dir(&mut self, p: &Path, m: u32) -> std::io::Result<()> {
            self.0.create_dir(p, m)
        }
        fn open_source(
            &mut self,
            p: &Path,
            m: u64,
        ) -> std::io::Result<Option<crate::privileged::HeldSource>> {
            if p.ends_with("MacOS/dyno-helper.minisig") {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            self.0.open_source(p, m)
        }
        fn write_program(&mut self, b: &[u8], to: &Path, m: u32) -> std::io::Result<String> {
            self.0.write_program(b, to, m)
        }
        fn sha256(&mut self, p: &Path) -> std::io::Result<String> {
            self.0.sha256(p)
        }
        fn read_checked(
            &mut self,
            p: &Path,
            m: u64,
        ) -> std::io::Result<Option<(crate::privileged::FileFacts, String)>> {
            self.0.read_checked(p, m)
        }
        fn remove_file(&mut self, p: &Path) -> std::io::Result<()> {
            self.0.remove_file(p)
        }
        fn remove_dir(&mut self, p: &Path) -> std::io::Result<()> {
            self.0.remove_dir(p)
        }
    }
    let disk = mac_disk();
    disk.put_file(SIG, b"sig-1", 501, 0o644);
    let v = ExpectSignature {
        program: b"helper build 1".to_vec(),
        signature: b"sig-1".to_vec(),
    };
    let (mut f, mut b, mut s) = (
        DeniesFirst(disk),
        Scripted::with(|s| s.starts_after_polls = Some(0)),
        FakeSystem::new(),
    );
    let report = Installer::new(&layout_for(Os::MacOs), &mut f, &mut b, &mut s)
        .with_signature(SignatureCheck {
            verifier: &v,
            locations: vec![
                PathBuf::from("/Applications/Dyno.app/Contents/MacOS/dyno-helper.minisig"),
                PathBuf::from(SIG),
            ],
        })
        .install(Path::new(SOURCE))
        .unwrap();
    assert_eq!(report.signature.as_deref(), Some("valid"));
}

#[test]
fn a_verbatim_prefixed_source_is_judged_by_the_path_without_the_prefix() {
    // `canonicalize` on Windows returns `\\?\C:\...`: the chain check and the "already inside the protected folder" comparison
    // must see the plain path, or a per-machine source fails with "not there" and the installed copy could be copied onto itself.
    let files = windows_disk();
    let verbatim = format!("\\\\?\\{WIN_SOURCE}");
    files.put_file(verbatim.as_str(), b"helper.exe", 0, 0);
    let report = win_install_with(&mut win_trust(), &files, &verbatim);
    assert!(report.is_ok(), "{report:?}");
    let inside = format!("\\\\?\\{WIN_PROGRAM}");
    files.put_file(inside.as_str(), b"helper.exe", 0, 0);
    let err = win_install_with(&mut win_trust(), &files, &inside).unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("already inside")),
        "{err:?}"
    );
}

#[test]
fn the_source_check_alone_changes_nothing_and_refuses_a_bad_source() {
    let files = mac_disk();
    let v = ExpectSignature {
        program: b"helper build 1".to_vec(),
        signature: b"sig-1".to_vec(),
    };
    let (mut f, mut b, mut s) = (files.clone(), Scripted::default(), FakeSystem::new());
    let err = Installer::new(&layout_for(Os::MacOs), &mut f, &mut b, &mut s)
        .with_signature(SignatureCheck {
            verifier: &v,
            locations: vec![PathBuf::from(SIG)],
        })
        .preflight(Path::new(SOURCE))
        .unwrap_err();
    assert!(
        matches!(&err, InstallError::Refused(w) if w.contains("no signature file")),
        "{err:?}"
    );
    assert!(
        !files
            .calls()
            .iter()
            .any(|c| c.starts_with("write ") || c.starts_with("create_dir"))
    );
    assert!(
        !b.log()
            .iter()
            .any(|l| l == "register" || l == "unregister" || l == "evict")
    );
    files.put_file(SIG, b"sig-1", 501, 0o644);
    let (mut f, mut b, mut s) = (files.clone(), Scripted::default(), FakeSystem::new());
    let report = Installer::new(&layout_for(Os::MacOs), &mut f, &mut b, &mut s)
        .with_signature(SignatureCheck {
            verifier: &v,
            locations: vec![PathBuf::from(SIG)],
        })
        .preflight(Path::new(SOURCE))
        .unwrap();
    assert_eq!(report.signature.as_deref(), Some("valid"));
    assert!(!files.calls().iter().any(|c| c.starts_with("write ")));
}
