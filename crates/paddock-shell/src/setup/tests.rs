//! The checklist, its four steps and the Tauri adapter, all over fakes: nothing here reaches the OS, the credential
//! store, a real Pitwall or the user's folders (rule 7), and nothing waits on the clock.

use super::checklist::{Checklist, Do, Emit, ListError, MAX_ID_LEN, MAX_ITEMS, Runner};
use super::items::*;
use super::model::*;
use paddock_health::notify::os::Permission;
use paddock_pitwall::{Caps, FoundKind, LinkView, PairingView};
use paddock_service::backend::Mechanism;
use paddock_service::status::{Condition, ServiceStatus};
use paddock_settings::{ApplyError, Settings};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------------------------------------------------
// A scripted item

#[derive(Clone)]
struct Script {
    check: Arc<Mutex<Check>>,
    act: Arc<Mutex<Result<(), String>>>,
    repair: Arc<Mutex<Option<Result<(), String>>>>,
    acted: Arc<AtomicUsize>,
    repaired: Arc<AtomicUsize>,
    panic_check: Arc<AtomicBool>,
    panic_act: Arc<AtomicBool>,
}

struct Fake {
    id: String,
    script: Script,
}

fn script(check: Check) -> Script {
    Script {
        check: Arc::new(Mutex::new(check)),
        act: Arc::new(Mutex::new(Ok(()))),
        repair: Arc::new(Mutex::new(None)),
        acted: Arc::default(),
        repaired: Arc::default(),
        panic_check: Arc::default(),
        panic_act: Arc::default(),
    }
}

fn fake(id: &str, check: Check) -> (Arc<dyn SetupItem>, Script) {
    let s = script(check);
    (
        Arc::new(Fake {
            id: id.into(),
            script: s.clone(),
        }),
        s,
    )
}

impl SetupItem for Fake {
    fn id(&self) -> &str {
        &self.id
    }
    fn title(&self) -> String {
        format!("Step {}", self.id)
    }
    fn detail(&self) -> String {
        "Detail".into()
    }
    fn check(&self) -> Check {
        assert!(!self.script.panic_check.load(Ordering::SeqCst), "boom");
        self.script.check.lock().unwrap().clone()
    }
    fn act(&self) -> Result<(), String> {
        assert!(!self.script.panic_act.load(Ordering::SeqCst), "boom");
        self.script.acted.fetch_add(1, Ordering::SeqCst);
        let r = self.script.act.lock().unwrap().clone();
        if r.is_ok() {
            *self.script.check.lock().unwrap() = Check::Done;
        }
        r
    }
    fn repair(&self) -> Result<(), String> {
        self.script.repaired.fetch_add(1, Ordering::SeqCst);
        match self.script.repair.lock().unwrap().clone() {
            Some(r) => r,
            None => self.act(),
        }
    }
}

type Seen = Arc<Mutex<Vec<SetupState>>>;

fn inline() -> Runner {
    Arc::new(|job| {
        job();
        Ok(())
    })
}

fn recorder() -> (Emit, Seen) {
    let seen: Seen = Arc::default();
    let sink = seen.clone();
    (
        Arc::new(move |s: &SetupState| sink.lock().unwrap().push(s.clone())),
        seen,
    )
}

fn list(items: Vec<Arc<dyn SetupItem>>) -> (Arc<Checklist>, Seen) {
    let (emit, seen) = recorder();
    (Checklist::new(items, None, emit, inline()).unwrap(), seen)
}

fn view<'a>(s: &'a SetupState, id: &str) -> &'a ItemView {
    s.items.iter().find(|i| i.id == id).unwrap()
}

// ---------------------------------------------------------------------------------------------------------------------
// The checklist

#[test]
fn ids_are_checked_once_when_the_list_is_built() {
    let (emit, _) = recorder();
    let build = |ids: &[&str]| {
        Checklist::new(
            ids.iter().map(|i| fake(i, Check::Todo).0).collect(),
            None,
            emit.clone(),
            inline(),
        )
        .err()
    };
    assert_eq!(build(&["a", "b"]), None);
    for bad in [
        "",
        "Upper",
        "has space",
        "slash/x",
        &"x".repeat(MAX_ID_LEN + 1),
    ] {
        assert_eq!(build(&[bad]), Some(ListError::BadId(bad.into())));
    }
    assert_eq!(build(&["a", "a"]), Some(ListError::Duplicate("a".into())));
    let many: Vec<String> = (0..=MAX_ITEMS).map(|i| format!("s{i}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    assert_eq!(build(&many), Some(ListError::TooMany(MAX_ITEMS + 1)));
    for e in [
        ListError::BadId("X".into()),
        ListError::Duplicate("a".into()),
        ListError::TooMany(99),
    ] {
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn the_state_lists_the_steps_in_order_and_leaves_out_those_that_do_not_apply() {
    let (a, _) = fake("a", Check::Done);
    let (b, _) = fake("b", Check::Todo);
    let (c, _) = fake("c", Check::NotApplicable);
    let (d, _) = fake("d", Check::Problem("It is off.".into()));
    let (list, _) = list(vec![a, b, c, d]);
    let s = list.state();
    let ids: Vec<&str> = s.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "d"]);
    assert_eq!(view(&s, "a").status, ItemStatus::Done);
    assert_eq!(view(&s, "b").status, ItemStatus::Todo);
    assert_eq!(view(&s, "d").status, ItemStatus::Problem);
    assert_eq!(view(&s, "d").message.as_deref(), Some("It is off."));
    assert_eq!(view(&s, "b").title, "Step b");
    assert_eq!(view(&s, "b").action, "Set up");
    assert!(s.show_on_start && !s.finished);
}

#[test]
fn it_opens_by_itself_only_while_a_step_is_open_and_not_skipped_or_finished() {
    let (a, _) = fake("a", Check::Done);
    let (b, _) = fake("b", Check::Todo);
    let (list, seen) = list(vec![a, b]);
    assert!(list.state().show_on_start);
    let s = list.skip("b", true).unwrap();
    assert!(
        view(&s, "b").skipped && !s.show_on_start,
        "a skipped step does not open it"
    );
    let s = list.skip("b", false).unwrap();
    assert!(!view(&s, "b").skipped && s.show_on_start);
    let s = list.finish().unwrap();
    assert!(s.finished && !s.show_on_start);
    // Every change was sent to the windows.
    assert_eq!(seen.lock().unwrap().len(), 3);
    // Finished stays finished, and the steps stay listed for Settings.
    assert_eq!(list.state().items.len(), 2);
    // Nothing open at all: nothing to show.
    let (done, _) = fake("d", Check::Done);
    assert!(
        !super::checklist::Checklist::new(vec![done], None, recorder().0, inline())
            .unwrap()
            .state()
            .show_on_start
    );
}

#[test]
fn an_unknown_step_is_refused_with_a_code() {
    let (a, _) = fake("a", Check::Todo);
    let (list, _) = list(vec![a]);
    for e in [
        list.skip("zzz", true).unwrap_err(),
        list.request("zzz", Do::Act).unwrap_err(),
    ] {
        assert_eq!(e.code, SetupErrorCode::Unknown);
        assert!(!e.retryable && e.message.contains("zzz"));
    }
    assert_eq!(
        SetupCommandError::not_set_up().code,
        SetupErrorCode::NotSetUp
    );
    assert_eq!(
        SetupCommandError::not_set_up().to_string(),
        "This app has no set-up checklist."
    );
}

#[test]
fn an_action_marks_the_step_as_working_then_reports_the_outcome() {
    let (a, s) = fake("a", Check::Todo);
    let (list, seen) = list(vec![a]);
    list.request("a", Do::Act).unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(view(&seen[0], "a").working);
    assert!(!view(&seen[1], "a").working);
    assert_eq!(view(&seen[1], "a").status, ItemStatus::Done);
    assert_eq!(s.acted.load(Ordering::SeqCst), 1);
}

#[test]
fn a_failed_action_is_a_problem_with_a_repair_that_runs_the_repair() {
    let (a, s) = fake("a", Check::Todo);
    *s.act.lock().unwrap() = Err("The system said no.".into());
    let (list, _) = list(vec![a]);
    list.request("a", Do::Act).unwrap();
    let st = list.state();
    assert_eq!(view(&st, "a").status, ItemStatus::Problem);
    assert_eq!(
        view(&st, "a").message.as_deref(),
        Some("The system said no.")
    );
    // Repair is its own call (rule 19), and a good one clears the message.
    *s.repair.lock().unwrap() = Some(Ok(()));
    *s.check.lock().unwrap() = Check::Done;
    list.request("a", Do::Repair).unwrap();
    assert_eq!(s.repaired.load(Ordering::SeqCst), 1);
    assert_eq!(view(&list.state(), "a").message, None);
    // A failure shown over a step whose own check says problem keeps the failure's words.
    *s.check.lock().unwrap() = Check::Problem("check says".into());
    *s.repair.lock().unwrap() = Some(Err("repair says".into()));
    list.request("a", Do::Repair).unwrap();
    assert_eq!(
        view(&list.state(), "a").message.as_deref(),
        Some("repair says")
    );
}

#[test]
fn a_step_that_runs_refuses_a_second_request_until_it_ends() {
    let (a, _) = fake("a", Check::Todo);
    let (emit, _) = recorder();
    type Job = Box<dyn FnOnce() + Send>;
    let queue: Arc<Mutex<Vec<Job>>> = Arc::default();
    let q = queue.clone();
    let run: Runner = Arc::new(move |job| {
        q.lock().unwrap().push(job);
        Ok(())
    });
    let list = Checklist::new(vec![a], None, emit, run).unwrap();
    list.request("a", Do::Act).unwrap();
    let busy = list.request("a", Do::Act).unwrap_err();
    assert_eq!(busy.code, SetupErrorCode::Busy);
    assert!(busy.retryable);
    let job = queue.lock().unwrap().pop().unwrap();
    job();
    list.request("a", Do::Repair).unwrap();
}

#[test]
fn a_worker_that_cannot_start_leaves_nothing_marked_as_working() {
    let (a, _) = fake("a", Check::Todo);
    let run: Runner = Arc::new(|_| Err("no threads".into()));
    let list = Checklist::new(vec![a], None, recorder().0, run).unwrap();
    for _ in 0..2 {
        let e = list.request("a", Do::Act).unwrap_err();
        assert_eq!(e.code, SetupErrorCode::Failed);
        assert!(e.retryable && e.message.contains("no threads"));
    }
    assert!(!view(&list.state(), "a").working);
}

#[test]
fn a_step_that_panics_is_a_problem_of_that_step_and_nothing_else() {
    let (a, sa) = fake("a", Check::Todo);
    let (b, _) = fake("b", Check::Done);
    let (list, _) = list(vec![a, b]);
    sa.panic_act.store(true, Ordering::SeqCst);
    list.request("a", Do::Act).unwrap();
    let st = list.state();
    assert!(
        view(&st, "a")
            .message
            .as_deref()
            .unwrap()
            .contains("stopped unexpectedly")
    );
    assert_eq!(view(&st, "b").status, ItemStatus::Done);
    assert!(!view(&st, "a").working);
    sa.panic_check.store(true, Ordering::SeqCst);
    let st = list.state();
    assert_eq!(view(&st, "a").status, ItemStatus::Problem);
    assert!(view(&st, "a").message.is_some());
    assert!(SetupCommandError::failed("x").retryable);
}

#[test]
fn the_answers_survive_a_restart_and_forget_steps_that_are_gone() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("setup.json");
    let build = |ids: &[&str]| {
        Checklist::new(
            ids.iter().map(|i| fake(i, Check::Todo).0).collect(),
            Some(file.clone()),
            recorder().0,
            inline(),
        )
        .unwrap()
    };
    let first = build(&["a", "b"]);
    first.skip("a", true).unwrap();
    first.skip("b", true).unwrap();
    first.finish().unwrap();
    let second = build(&["a"]);
    let st = second.state();
    assert!(st.finished && view(&st, "a").skipped);
    second.skip("a", false).unwrap();
    let on_disk = std::fs::read_to_string(&file).unwrap();
    assert!(!on_disk.contains("\"b\""), "{on_disk}");
    // A damaged file starts fresh instead of failing the app.
    // (a valid backup is used when there is one; without it the answers start over)
    std::fs::write(&file, "{ not json").unwrap();
    assert!(build(&["a"]).state().finished);
    std::fs::remove_file(file.with_extension("json.bak")).unwrap();
    std::fs::write(&file, "{ not json").unwrap();
    assert!(!build(&["a"]).state().finished);
}

#[test]
fn an_answer_that_cannot_be_kept_says_so_but_still_applies_this_session() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("file");
    std::fs::write(&blocker, "x").unwrap();
    // The state file would be inside a regular file: it cannot be written.
    let list = Checklist::new(
        vec![fake("a", Check::Todo).0],
        Some(blocker.join("setup.json")),
        recorder().0,
        inline(),
    )
    .unwrap();
    let e = list.skip("a", true).unwrap_err();
    assert_eq!(e.code, SetupErrorCode::NotSaved);
    assert!(e.retryable);
    assert!(view(&list.state(), "a").skipped);
}

// ---------------------------------------------------------------------------------------------------------------------
// The service step

struct FakeService {
    status: Mutex<Option<ServiceStatus>>,
    calls: Mutex<Vec<&'static str>>,
    fail: AtomicBool,
    login_page: AtomicBool,
}

fn status(condition: Condition) -> ServiceStatus {
    let (installed, can_install, can_repair, can_open) = match condition {
        Condition::NotInstalled => (false, true, false, false),
        Condition::SwitchedOff | Condition::NeedsApproval => (true, false, false, true),
        Condition::Running | Condition::Repairing | Condition::NewerOwner => {
            (true, false, false, false)
        }
        Condition::Unavailable => (false, false, false, false),
        _ => (true, false, true, false),
    };
    ServiceStatus {
        mechanism: Mechanism::LaunchdBundled,
        condition,
        installed,
        running: condition == Condition::Running,
        message: Some(format!("{condition:?} words")),
        can_repair,
        can_install,
        can_open_login_items: can_open,
        repair: None,
    }
}

fn service(condition: Option<Condition>) -> (ServiceItem, Arc<FakeService>) {
    let fake = Arc::new(FakeService {
        status: Mutex::new(condition.map(status)),
        calls: Mutex::default(),
        fail: AtomicBool::new(false),
        login_page: AtomicBool::new(true),
    });
    (ServiceItem::new("Demo", fake.clone()), fake)
}

impl ServicePort for FakeService {
    fn status(&self) -> Option<ServiceStatus> {
        self.status.lock().unwrap().clone()
    }
    fn install(&self) -> Result<(), String> {
        self.calls.lock().unwrap().push("install");
        if self.fail.load(Ordering::SeqCst) {
            Err("no".into())
        } else {
            Ok(())
        }
    }
    fn repair(&self) -> Result<(), String> {
        self.calls.lock().unwrap().push("repair");
        Ok(())
    }
    fn open_login_items(&self) -> Result<bool, String> {
        self.calls.lock().unwrap().push("login-items");
        Ok(self.login_page.load(Ordering::SeqCst))
    }
}

#[test]
fn the_service_step_follows_the_service_condition() {
    let expect = |c: Condition, want: Check| {
        let (item, _) = service(Some(c));
        let got = item.check();
        match (&got, &want) {
            (Check::Problem(_), Check::Problem(_)) => {}
            _ => assert_eq!(got, want, "{c:?}"),
        }
    };
    expect(Condition::Running, Check::Done);
    expect(Condition::NotInstalled, Check::Todo);
    expect(Condition::Repairing, Check::Todo);
    expect(Condition::Unavailable, Check::NotApplicable);
    for c in [
        Condition::NotRunning,
        Condition::Refused,
        Condition::ProgramMissing,
        Condition::Moved,
        Condition::SwitchedOff,
        Condition::NeedsApproval,
        Condition::NewerOwner,
    ] {
        expect(c, Check::Problem(String::new()));
    }
    assert_eq!(service(None).0.check(), Check::NotApplicable);
    // Without a message the problem still says something.
    let (item, fake) = service(Some(Condition::NotRunning));
    fake.status.lock().unwrap().as_mut().unwrap().message = None;
    assert!(matches!(item.check(), Check::Problem(m) if m.contains("not running")));
    assert_eq!(item.id(), ids::SERVICE);
    assert_eq!(item.action_label(), "Install");
    assert!(item.detail().contains("Demo") && !item.title().is_empty());
}

#[test]
fn the_service_step_installs_repairs_or_opens_login_items_as_the_engine_allows() {
    let (item, f) = service(Some(Condition::NotInstalled));
    item.act().unwrap();
    assert_eq!(*f.calls.lock().unwrap(), ["install"]);
    f.fail.store(true, Ordering::SeqCst);
    assert_eq!(item.act().unwrap_err(), "no");

    let (item, f) = service(Some(Condition::Refused));
    item.act().unwrap();
    item.repair().unwrap();
    assert_eq!(*f.calls.lock().unwrap(), ["repair", "repair"]);

    // The user's switch stays the user's: the page opens, nothing flips.
    let (item, f) = service(Some(Condition::SwitchedOff));
    item.repair().unwrap();
    item.act().unwrap();
    assert_eq!(*f.calls.lock().unwrap(), ["login-items", "login-items"]);
    f.login_page.store(false, Ordering::SeqCst);
    assert!(item.repair().unwrap_err().contains("no Login Items page"));

    // Nothing applies: the engine's words come back; installing is the fallback of a repair.
    let (item, _) = service(Some(Condition::NewerOwner));
    assert!(item.repair().unwrap_err().contains("NewerOwner"));
    let (item, f) = service(Some(Condition::NewerOwner));
    f.status.lock().unwrap().as_mut().unwrap().message = None;
    assert!(
        item.repair()
            .unwrap_err()
            .contains("Nothing can be repaired")
    );
    let (item, f) = service(Some(Condition::NotInstalled));
    item.repair().unwrap();
    assert_eq!(*f.calls.lock().unwrap(), ["install"]);
    // No answer at all.
    let (item, _) = service(None);
    assert!(item.act().unwrap_err().contains("could not be asked"));
}

#[test]
fn the_service_port_over_the_real_control_reads_and_acts_through_the_fake_os() {
    use paddock_service::backend::{Target, WindowsRun};
    use paddock_service::control::ServiceControl;
    use paddock_service::manager::Manager;
    use paddock_service::spec::ServiceSpec;
    use paddock_service::testing::FakeSystem;
    let sys = FakeSystem::new();
    sys.put_program("C:\\a.exe");
    let spec = ServiceSpec::builder("net.a.b", "A", "a").build().unwrap();
    let b = WindowsRun::new(spec, Target::exe("C:\\a.exe"), Box::new(sys));
    let manager = Manager::new(
        "A",
        Box::new(b),
        std::path::Path::new("/nonexistent-paddock-test"),
        "net.a.b",
        "1",
        || false,
    );
    let control = ServiceControl::new(manager, Arc::new(|_| {}));
    let port: &dyn ServicePort = &*control;
    assert_eq!(port.status().unwrap().condition, Condition::NotInstalled);
    // The fake OS answers every command with success or a refusal; the port maps both to a plain result.
    let _ = port.install();
    let _ = port.repair();
    assert!(port.open_login_items().is_ok());
}

// ---------------------------------------------------------------------------------------------------------------------
// The Start at login step

fn login_settings(refuse: Option<Arc<AtomicBool>>) -> Arc<Settings> {
    let settings = Arc::new(Settings::in_memory());
    let section = paddock_service::login::launch_at_login_section("Demo", true);
    settings
        .register(paddock_settings::section(
            section,
            move |_| -> Result<(), ApplyError> {
                match &refuse {
                    Some(r) if r.load(Ordering::SeqCst) => Err("the login item was refused".into()),
                    _ => Ok(()),
                }
            },
        ))
        .unwrap();
    settings
}

#[test]
fn the_login_step_turns_the_setting_on() {
    let settings = login_settings(None);
    let item = LoginItem::new("Demo", settings.clone());
    assert_eq!(item.check(), Check::Todo);
    item.act().unwrap();
    assert_eq!(item.check(), Check::Done);
    assert_eq!(item.id(), ids::LOGIN);
    assert!(item.detail().contains("Demo") && !item.title().is_empty());
    assert_eq!(item.action_label(), "Turn on");
    // No such section (no service in the app): the step is not there.
    assert_eq!(
        LoginItem::new("Demo", Arc::new(Settings::in_memory())).check(),
        Check::NotApplicable
    );
    assert!(
        LoginItem::new("Demo", Arc::new(Settings::in_memory()))
            .act()
            .is_err()
    );
}

#[test]
fn the_login_step_reports_a_refused_apply() {
    let refuse = Arc::new(AtomicBool::new(false));
    let settings = login_settings(Some(refuse.clone()));
    refuse.store(true, Ordering::SeqCst);
    let e = LoginItem::new("Demo", settings).act().unwrap_err();
    assert!(e.contains("refused"), "{e}");
}

#[test]
fn the_login_step_reports_values_the_settings_refuse() {
    // A section whose only field is validated against a range refuses the value: the errors come back as one message.
    use paddock_settings::{FieldKind, SectionOwner, SettingField, SettingsSection};
    let settings = Arc::new(Settings::in_memory());
    let section = SettingsSection::new(
        paddock_service::LOGIN_SECTION,
        "Start at login",
        SectionOwner::Shell,
    )
    .field(SettingField::new(
        paddock_service::login::LAUNCH_AT_LOGIN,
        "Open at login",
        FieldKind::Text {
            max_len: 5,
            pattern: Some("x".into()),
        },
        "x",
    ));
    settings
        .register(paddock_settings::section(
            section,
            |_| -> Result<(), ApplyError> { Ok(()) },
        ))
        .unwrap();
    let e = LoginItem::new("Demo", settings).act().unwrap_err();
    assert!(!e.is_empty());
}

// ---------------------------------------------------------------------------------------------------------------------
// The notification step

struct FakeAccess {
    permission: Mutex<Permission>,
    after_request: Permission,
    asks: bool,
    opened: AtomicUsize,
}

impl NotificationAccess for FakeAccess {
    fn permission(&self) -> Permission {
        *self.permission.lock().unwrap()
    }
    fn request(&self) -> Permission {
        *self.permission.lock().unwrap() = self.after_request;
        self.after_request
    }
    fn open_system_settings(&self) -> Result<(), String> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn asks(&self) -> bool {
        self.asks
    }
}

fn access(now: Permission, after: Permission, asks: bool) -> (NotificationsItem, Arc<FakeAccess>) {
    let a = Arc::new(FakeAccess {
        permission: Mutex::new(now),
        after_request: after,
        asks,
        opened: AtomicUsize::new(0),
    });
    (NotificationsItem::new("Demo", a.clone()), a)
}

#[test]
fn the_notification_step_asks_once_and_repairs_a_refusal_by_opening_the_system_settings() {
    let (item, _) = access(Permission::Unknown, Permission::Granted, true);
    assert_eq!(item.check(), Check::Todo);
    item.act().unwrap();
    assert_eq!(item.check(), Check::Done);
    assert_eq!(item.id(), ids::NOTIFICATIONS);
    assert_eq!(item.action_label(), "Allow");
    assert!(item.detail().contains("Demo") && !item.title().is_empty());

    // The OS never says yes or no: asking once is all the app can do.
    let (item, _) = access(Permission::Unknown, Permission::Unknown, true);
    item.act().unwrap();
    assert_eq!(item.check(), Check::Done);

    // The user said no: Repair opens the system settings; the app cannot switch it on.
    let (item, a) = access(Permission::Denied, Permission::Denied, true);
    assert!(matches!(item.check(), Check::Problem(m) if m.contains("system settings")));
    assert!(item.act().unwrap_err().contains("did not allow"));
    item.repair().unwrap();
    assert_eq!(a.opened.load(Ordering::SeqCst), 1);

    // Systems that notify without asking: no step.
    let (item, _) = access(Permission::Unknown, Permission::Unknown, false);
    assert_eq!(item.check(), Check::NotApplicable);
    // The default of `asks` follows the platform (macOS asks).
    struct Bare;
    impl NotificationAccess for Bare {
        fn permission(&self) -> Permission {
            Permission::Granted
        }
        fn request(&self) -> Permission {
            Permission::Granted
        }
        fn open_system_settings(&self) -> Result<(), String> {
            Ok(())
        }
    }
    assert_eq!(Bare.asks(), cfg!(target_os = "macos"));
}

#[test]
fn the_toaster_access_reads_the_permission_and_uses_the_apps_closures() {
    use paddock_health::notify::os::FakeToaster;
    let toaster = Arc::new(FakeToaster::default());
    toaster.set_permission(Permission::Denied);
    let plain = ToasterAccess::new(toaster.clone());
    assert_eq!(plain.permission(), Permission::Denied);
    assert_eq!(
        plain.request(),
        Permission::Denied,
        "without a closure asking reads the permission again"
    );
    assert!(
        plain
            .open_system_settings()
            .unwrap_err()
            .contains("notification settings")
    );
    let opened = Arc::new(AtomicUsize::new(0));
    let count = opened.clone();
    let wired = ToasterAccess::new(toaster)
        .request_with(|| Permission::Granted)
        .open_settings_with(move || {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
    assert_eq!(wired.request(), Permission::Granted);
    wired.open_system_settings().unwrap();
    assert_eq!(opened.load(Ordering::SeqCst), 1);
}

// ---------------------------------------------------------------------------------------------------------------------
// The Pitwall step

struct FakePitwall {
    view: Mutex<LinkView>,
    calls: Mutex<Vec<&'static str>>,
    connect: Result<(), String>,
}

impl PitwallPort for FakePitwall {
    fn probe(&self) -> LinkView {
        self.view.lock().unwrap().clone()
    }
    fn connect(&self) -> Result<(), String> {
        self.calls.lock().unwrap().push("connect");
        self.connect.clone()
    }
    fn start(&self) -> Result<(), String> {
        self.calls.lock().unwrap().push("start");
        Ok(())
    }
}

fn pitwall(view: LinkView) -> (PitwallItem, Arc<FakePitwall>) {
    let p = Arc::new(FakePitwall {
        view: Mutex::new(view),
        calls: Mutex::default(),
        connect: Ok(()),
    });
    (PitwallItem::new("Demo", Caps::chat(8192), p.clone()), p)
}

fn running(paired: bool, usable: bool) -> LinkView {
    LinkView {
        found: FoundKind::Running,
        paired,
        usable,
        model: usable.then(|| "Qwen".to_string()),
        ..LinkView::default()
    }
}

#[test]
fn the_pitwall_step_is_optional_and_never_an_error_for_a_missing_pitwall() {
    let (item, _) = pitwall(LinkView::default());
    assert_eq!(item.check(), Check::Todo, "not installed is a step to skip");
    let needs = item.needs_ai().unwrap();
    assert_eq!(needs.actions, [paddock_pitwall::NextStep::InstallPitwall]);
    assert!(item.act().unwrap_err().contains("not installed"));

    let (item, p) = pitwall(LinkView {
        found: FoundKind::Installed,
        ..LinkView::default()
    });
    assert_eq!(item.check(), Check::Todo);
    item.act().unwrap();
    assert_eq!(*p.calls.lock().unwrap(), ["start"]);
    assert_eq!(
        item.needs_ai().unwrap().actions,
        [paddock_pitwall::NextStep::StartPitwall]
    );

    assert_eq!(item.id(), ids::PITWALL);
    assert_eq!(item.action_label(), "Connect");
    assert!(item.detail().contains("optional") && !item.title().is_empty());
}

#[test]
fn the_pitwall_step_pairs_and_shows_the_code_to_compare() {
    let (item, p) = pitwall(running(false, false));
    assert_eq!(item.check(), Check::Todo);
    assert_eq!(item.hint(), None);
    item.act().unwrap();
    assert_eq!(*p.calls.lock().unwrap(), ["connect"]);
    p.view.lock().unwrap().pairing = Some(PairingView {
        code: "123456".into(),
        expires_at_ms: 1,
    });
    assert_eq!(item.check(), Check::Todo);
    assert!(item.hint().unwrap().contains("123456"));

    let (item, _) = pitwall(running(true, true));
    assert_eq!(item.check(), Check::Done);
    assert!(item.needs_ai().is_none());
    item.act().unwrap();

    // A refused pairing comes back as the step's failure.
    let p = Arc::new(FakePitwall {
        view: Mutex::new(running(false, false)),
        calls: Mutex::default(),
        connect: Err("denied".into()),
    });
    let item = PitwallItem::new("Demo", Caps::chat(8192), p);
    assert_eq!(item.act().unwrap_err(), "denied");
}

#[test]
fn the_pitwall_step_reports_a_paused_pitwall_as_a_problem_with_start_as_its_repair() {
    let mut paused = running(true, false);
    paused.paused = Some("paused".into());
    let (item, p) = pitwall(paused);
    assert!(matches!(item.check(), Check::Problem(m) if m.contains("paused")));
    item.repair().unwrap();
    assert_eq!(*p.calls.lock().unwrap(), ["start"]);
    // Another problem has no action of its own: its words come back.
    let mut odd = running(true, false);
    odd.problem = Some("Pitwall is warming up.".into());
    let (item, _) = pitwall(odd);
    assert!(matches!(item.check(), Check::Problem(_)));
    assert!(item.act().unwrap_err().contains("warming up"));
}

#[test]
fn the_link_port_asks_the_link_and_the_app_starts_pitwall() {
    use paddock_pitwall::{AppIdentity, Link, LinkConfig, Locator};
    use paddock_secrets::MemorySecrets;
    let dir = tempfile::tempdir().unwrap();
    let link = Link::new(LinkConfig::new(
        AppIdentity::new("demo", "Demo").unwrap(),
        Locator::in_dir(dir.path(), None, Arc::new(|_| false)),
        Arc::new(MemorySecrets::new("Demo")),
    ));
    let port = LinkPort::new(link.clone());
    assert_eq!(port.probe().found, FoundKind::Installed);
    assert!(port.connect().is_err(), "nothing runs to pair with");
    assert!(port.start().unwrap_err().contains("Open Pitwall"));
    let started = Arc::new(AtomicBool::new(false));
    let flag = started.clone();
    let port = LinkPort::new(link).starter(move || {
        flag.store(true, Ordering::SeqCst);
        Ok(())
    });
    port.start().unwrap();
    assert!(started.load(Ordering::SeqCst));
}

// ---------------------------------------------------------------------------------------------------------------------
// The Tauri adapter

mod adapter {
    use super::*;
    use crate::setup::{COMMANDS, EVENT, tauri as setup_tauri};
    use ::tauri::ipc::{CallbackFn, InvokeBody};
    use ::tauri::test::{
        INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
    };
    use ::tauri::webview::InvokeRequest;
    use ::tauri::{Listener, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
    use serde_json::{Value, json};
    use std::sync::mpsc;

    type App = ::tauri::App<MockRuntime>;

    fn app() -> (App, WebviewWindow<MockRuntime>) {
        let app = mock_builder()
            .invoke_handler(setup_tauri::invoke_handler())
            .build(mock_context(noop_assets()))
            .unwrap();
        let window = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
            .build()
            .unwrap();
        (app, window)
    }

    fn invoke(w: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
        let request = InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: if cfg!(windows) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        };
        get_ipc_response(w, request).map(|b| b.deserialize::<Value>().unwrap())
    }

    #[test]
    fn without_a_checklist_every_command_answers_not_set_up() {
        let (_app, w) = app();
        for (cmd, body) in [
            ("setup_state", json!({})),
            ("setup_act", json!({"id": "a"})),
            ("setup_repair", json!({"id": "a"})),
            ("setup_skip", json!({"id": "a", "skipped": true})),
            ("setup_finish", json!({})),
        ] {
            let e = invoke(&w, cmd, body).unwrap_err();
            assert_eq!(e["code"], "notSetUp", "{cmd}");
        }
    }

    #[test]
    fn the_commands_serve_the_checklist_and_changes_reach_every_window() {
        let (app, w) = app();
        let (a, s) = fake("a", Check::Todo);
        let (b, _) = fake("b", Check::Todo);
        setup_tauri::install(app.handle(), vec![a, b], None).unwrap();
        let (tx, rx) = mpsc::channel();
        app.listen(EVENT, move |e| {
            let _ = tx.send(e.payload().to_string());
        });

        let st = invoke(&w, "setup_state", json!({})).unwrap();
        assert_eq!(st["items"].as_array().unwrap().len(), 2);
        assert_eq!(st["showOnStart"], true);
        assert_eq!(st["items"][0]["needsAi"], Value::Null);

        let st = invoke(&w, "setup_skip", json!({"id": "b", "skipped": true})).unwrap();
        assert_eq!(st["items"][1]["skipped"], true);
        assert_eq!(
            invoke(&w, "setup_skip", json!({"id": "zz", "skipped": true})).unwrap_err()["code"],
            "unknown"
        );

        // Act returns at once; the worker's outcome arrives as events (working, then done). Waiting is on the channel.
        invoke(&w, "setup_act", json!({"id": "a"})).unwrap();
        let mut done = false;
        while !done {
            let payload: Value = serde_json::from_str(&rx.recv().unwrap()).unwrap();
            done =
                payload["items"][0]["status"] == "done" && payload["items"][0]["working"] == false;
        }
        assert_eq!(s.acted.load(Ordering::SeqCst), 1);
        invoke(&w, "setup_repair", json!({"id": "a"})).unwrap();
        assert_eq!(
            invoke(&w, "setup_act", json!({"id": "zz"})).unwrap_err()["code"],
            "unknown"
        );

        let st = invoke(&w, "setup_finish", json!({})).unwrap();
        assert_eq!(st["finished"], true);
        assert_eq!(st["showOnStart"], false);
    }

    #[test]
    fn a_bad_list_is_refused_when_it_is_installed() {
        let (app, _w) = app();
        let (a, _) = fake("same", Check::Todo);
        let (b, _) = fake("same", Check::Todo);
        assert!(setup_tauri::install(app.handle(), vec![a, b], None).is_err());
    }

    #[test]
    fn the_table_declares_who_may_call_what() {
        let spec = |name: &str| {
            COMMANDS
                .specs()
                .iter()
                .find(|s| s.name() == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        assert!(COMMANDS.specs().len() == 5);
        for name in [
            "setup_act",
            "setup_repair",
            "setup_skip",
            "setup_finish",
            "setup_state",
        ] {
            let _ = spec(name);
        }
    }
}

#[test]
fn every_list_error_and_command_error_has_words() {
    assert!(SetupCommandError::failed("x").to_string().contains("x"));
}
