//! The guard over fake security descriptors built from the default ACLs of `C:\`, `C:\ProgramData` and `C:\Program Files`
//! (described in `paddock_service::privileged::trust_fake`; no user data).

use super::*;
use paddock_service::privileged::trust::{right, Ace, Security, Style, SID_USERS};
use paddock_service::privileged::trust_fake::*;
use std::sync::{Arc, Mutex};

const DATA: &str = "C:\\ProgramData\\Dyno\\Helper";
const STAGING: &str = "C:\\ProgramData\\Dyno\\Helper\\staging";

/// The fake machine shared between the guard and the test.
#[derive(Clone)]
struct Shared(Arc<Mutex<FakePort>>);

impl TrustPort for Shared {
    fn style(&self) -> Style {
        self.0.lock().unwrap().style()
    }
    fn node(&mut self, p: &str) -> io::Result<Option<paddock_service::privileged::trust::Node>> {
        self.0.lock().unwrap().node(p)
    }
    fn canonical(&mut self, p: &str) -> io::Result<String> {
        self.0.lock().unwrap().canonical(p)
    }
    fn walk(&mut self, p: &str, limit: usize) -> io::Result<Vec<(String, paddock_service::privileged::trust::Node)>> {
        self.0.lock().unwrap().walk(p, limit)
    }
    fn create_dir(&mut self, p: &str) -> io::Result<()> {
        self.0.lock().unwrap().create_dir(p)
    }
    fn lock_dir(&mut self, p: &str) -> io::Result<()> {
        self.0.lock().unwrap().lock_dir(p)
    }
    fn rename_aside(&mut self, p: &str) -> io::Result<String> {
        self.0.lock().unwrap().rename_aside(p)
    }
    fn sweep_aside(&mut self, p: &str, keep: usize) -> io::Result<Vec<String>> {
        self.0.lock().unwrap().sweep_aside(p, keep)
    }
    fn remove_tree(&mut self, p: &str) -> io::Result<()> {
        self.0.lock().unwrap().remove_tree(p)
    }
}

/// Opens anything as itself, or as `redirect` (a junction in the requested path that leads elsewhere).
struct Opener {
    redirect: Option<String>,
}

impl ImageOpener for Opener {
    fn open(&self, requested: &str) -> io::Result<Held> {
        let final_path = self.redirect.clone().unwrap_or_else(|| requested.to_string());
        Ok(Held {
            spawn_path: final_path.clone(),
            final_path,
            _keep: Box::new(()),
        })
    }
}

#[derive(Clone, Default)]
struct Runs(Arc<Mutex<Vec<String>>>);

impl ToolRunner for Runs {
    fn run_tool(&self, tool: &Tool, _t: Duration) -> Result<ToolOutput, PlatformError> {
        self.0.lock().unwrap().push(tool.program.clone());
        Ok(ToolOutput {
            ok: true,
            stdout: String::new(),
            error: String::new(),
        })
    }
}

fn guard(machine: FakePort, host: HelperHost, redirect: Option<&str>) -> (Arc<TrustGuard>, Shared) {
    let shared = Shared(Arc::new(Mutex::new(machine)));
    let base = if host == HelperHost::Windows { "C:\\ProgramData" } else { "/var" };
    let g = TrustGuard::new(
        host,
        Box::new(shared.clone()),
        Box::new(Opener {
            redirect: redirect.map(str::to_string),
        }),
        Path::new(base),
    );
    (Arc::new(g), shared)
}

fn tool(program: &str) -> Tool {
    Tool {
        program: program.into(),
        args: vec!["--scan-open".into()],
        env: vec![],
    }
}

fn calls(s: &Shared) -> Vec<String> {
    s.0.lock().unwrap().calls.clone()
}

#[test]
fn a_tools_folder_that_others_can_write_is_refused_before_smartctl_runs() {
    // What a standard user found on the VM: Helper\tools inherits Users (CI)(WD,AD,WEA) from C:\ProgramData.
    let (g, _m) = guard(windows_machine(|_| inherited_from_program_data()), HelperHost::Windows, None);
    let runs = Runs::default();
    let runner = GuardedRunner::new(g, Some(TOOLS.into()), Box::new(runs.clone()));
    let e = runner.run_tool(&tool(SMARTCTL), Duration::from_secs(1)).unwrap_err().to_string();
    assert!(e.contains("not a protected place") && e.contains("Repair the helper"), "{e}");
    assert!(runs.0.lock().unwrap().is_empty(), "nothing is run from an unprotected folder");
}

#[test]
fn a_protected_tools_folder_runs_the_held_program_by_its_checked_path() {
    let (g, _m) = guard(windows_machine(|_| locked()), HelperHost::Windows, None);
    let runs = Runs::default();
    let runner = GuardedRunner::new(g, Some(TOOLS.into()), Box::new(runs.clone()));
    runner.run_tool(&tool(SMARTCTL), Duration::from_secs(1)).unwrap();
    assert_eq!(*runs.0.lock().unwrap(), vec![SMARTCTL.to_string()]);
}

#[test]
fn a_junction_in_the_requested_path_is_refused_and_nothing_is_run() {
    // The handle's final path is somewhere else: a junction was re-pointed between the check and the start.
    let elsewhere = "\\\\?\\C:\\Users\\Eve\\tools\\smartmontools-7.5\\bin\\smartctl.exe";
    let (g, _m) = guard(windows_machine(|_| locked()), HelperHost::Windows, Some(elsewhere));
    let runs = Runs::default();
    let runner = GuardedRunner::new(g, Some(TOOLS.into()), Box::new(runs.clone()));
    let e = runner.run_tool(&tool(SMARTCTL), Duration::from_secs(1)).unwrap_err().to_string();
    assert!(e.contains("leads somewhere else") && e.contains("Repair the helper"), "{e}");
    assert!(runs.0.lock().unwrap().is_empty());
}

#[test]
fn a_planted_program_owned_by_a_user_is_refused_even_in_a_locked_folder() {
    let mut m = windows_machine(|_| locked());
    let mut planted = file(locked());
    if let Security::Windows(d) = &mut planted.security {
        d.owner = "S-1-5-21-111-222-333-1001".into();
    }
    m.put(SMARTCTL, planted);
    let (g, _m) = guard(m, HelperHost::Windows, None);
    let runner = GuardedRunner::new(g, Some(TOOLS.into()), Box::new(Runs::default()));
    let e = runner.run_tool(&tool(SMARTCTL), Duration::from_secs(1)).unwrap_err().to_string();
    assert!(e.contains("owner"), "{e}");
}

#[test]
fn install_locks_the_data_root_tools_and_staging() {
    let mut m = windows_machine(|_| locked());
    for d in [DATA, TOOLS, STAGING] {
        m.nodes.remove(&m.key(d));
    }
    let (g, machine) = guard(m, HelperHost::Windows, None);
    g.prepare_folders(Path::new(DATA), OnUntrusted::Purge).unwrap();
    let c = calls(&machine);
    let tools = Path::new(DATA).join("tools").to_string_lossy().into_owned();
    let staging = Path::new(DATA).join("staging").to_string_lossy().into_owned();
    for dir in [DATA, tools.as_str(), staging.as_str()] {
        let made = c
            .iter()
            .position(|x| x == &format!("create_dir {dir}"))
            .unwrap_or_else(|| panic!("{dir} made: {c:?}"));
        let locked = c
            .iter()
            .position(|x| x == &format!("lock_dir {dir}"))
            .unwrap_or_else(|| panic!("{dir} locked: {c:?}"));
        assert!(made < locked, "created, then locked by handle: {c:?}");
    }
    assert!(c.iter().all(|x| !x.contains("icacls")), "no ACL is set by path: {c:?}");
    assert!(
        c.iter().all(|x| !x.starts_with("remove_tree")),
        "a folder that was not there is not removed: {c:?}"
    );
}

#[test]
fn the_folder_between_the_base_and_the_data_root_is_locked_too() {
    let mut m = windows_machine(|_| locked());
    for d in [DATA, TOOLS] {
        m.nodes.remove(&m.key(d));
    }
    let (g, machine) = guard(m, HelperHost::Windows, None);
    g.ensure_root(Path::new(DATA), OnUntrusted::Purge).unwrap();
    assert!(
        calls(&machine).contains(&"lock_dir C:\\ProgramData\\Dyno".to_string()),
        "{:?}",
        calls(&machine)
    );
}

#[test]
fn an_existing_vendor_folder_that_passes_the_ancestor_rule_is_locked_so_users_cannot_pre_create_helper() {
    // The unfixed helper left C:\ProgramData\Dyno inheriting Users (CI)(WD,AD,...): fine for an ancestor, but Users could
    // create Helper in it before the helper does.
    let mut m = windows_machine(|_| locked());
    m.put("C:\\ProgramData\\Dyno", inherited_from_program_data());
    let (g, machine) = guard(m, HelperHost::Windows, None);
    g.ensure_root(Path::new(TOOLS), OnUntrusted::Refuse).unwrap();
    assert_eq!(machine.0.lock().unwrap().nodes["c:\\programdata\\dyno"], locked());
    assert!(calls(&machine).contains(&"lock_dir C:\\ProgramData\\Dyno".to_string()));
}

fn user_made_dyno() -> FakePort {
    let user = "S-1-5-21-111-222-333-1001";
    let mut m = windows_machine(|_| locked());
    let mut dyno = inherited_from_program_data();
    if let Security::Windows(sd) = &mut dyno.security {
        sd.owner = user.into();
    }
    m.put("C:\\ProgramData\\Dyno", dyno);
    m
}

#[test]
fn at_service_start_an_untrusted_vendor_folder_is_refused_and_only_a_repair_moves_it_aside() {
    let (g, machine) = guard(user_made_dyno(), HelperHost::Windows, None);
    let e = g.ensure_root(Path::new(TOOLS), OnUntrusted::Refuse).unwrap_err().to_string();
    assert!(e.contains("Repair the helper") && e.contains("Dyno"), "{e}");
    assert!(calls(&machine).is_empty(), "start only refuses: {:?}", calls(&machine));
    g.ensure_root(Path::new(TOOLS), OnUntrusted::Purge).unwrap();
    assert!(calls(&machine).contains(&"rename_aside C:\\ProgramData\\Dyno".to_string()));
}

#[test]
fn moved_aside_folders_are_capped_at_two_with_the_oldest_removed() {
    let mut m = user_made_dyno();
    for n in 1..=3 {
        m.put(&format!("C:\\ProgramData\\Dyno.untrusted-{n}"), locked());
        m.put(&format!("C:\\ProgramData\\Dyno.untrusted-{n}\\Helper"), locked());
    }
    let (g, machine) = guard(m, HelperHost::Windows, None);
    g.ensure_root(Path::new(TOOLS), OnUntrusted::Purge).unwrap();
    let nodes = machine.0.lock().unwrap().nodes.clone();
    let mut asides: Vec<&String> = nodes
        .keys()
        .filter(|k| k.contains(".untrusted-") && !k.contains("\\helper"))
        .collect();
    asides.sort();
    assert_eq!(asides.len(), 2, "{asides:?}");
    assert!(
        asides.iter().all(|k| !k.ends_with("-1") && !k.ends_with("-2")),
        "the oldest go, the newest stay: {asides:?}"
    );
    assert!(
        !nodes.keys().any(|k| k.starts_with("c:\\programdata\\dyno.untrusted-1\\")),
        "what was inside goes with it"
    );
    assert!(
        calls(&machine).contains(&"sweep_aside C:\\ProgramData 2".to_string()),
        "{:?}",
        calls(&machine)
    );
}

/// A tree a standard user made before the fix: owned by that user, Users may add things.
fn user_made_tree() -> FakePort {
    let user = "S-1-5-21-111-222-333-1001";
    let mut m = windows_machine(|_| locked());
    for d in [DATA, TOOLS, "C:\\ProgramData\\Dyno\\Helper\\tools\\smartmontools-7.5"] {
        let mut n = inherited_from_program_data();
        if let Security::Windows(sd) = &mut n.security {
            sd.owner = user.into();
            sd.dacl.as_mut().unwrap().push(Ace::allow(user, FULL));
        }
        m.put(d, n);
    }
    m
}

#[test]
fn a_tools_folder_a_user_made_is_removed_and_made_again_as_system() {
    // The data root is the helper's (locked); the user pre-created tools\\smartmontools-7.5\\bin before the first install.
    let mut m = user_made_tree();
    m.put(DATA, locked());
    let (g, machine) = guard(m, HelperHost::Windows, None);
    g.ensure_root(Path::new(TOOLS), OnUntrusted::Purge).unwrap();
    let c = calls(&machine);
    let removed = c.iter().position(|x| x == &format!("remove_tree {TOOLS}")).expect("removed");
    let made = c.iter().position(|x| x == &format!("create_dir {TOOLS}")).expect("made again");
    assert!(removed < made, "{c:?}");
    // What the user had put below is gone with it.
    assert!(!machine
        .0
        .lock()
        .unwrap()
        .nodes
        .contains_key("c:\\programdata\\dyno\\helper\\tools\\smartmontools-7.5"));
}

#[test]
fn repair_removes_a_whole_user_made_tree_data_root_first() {
    let (g, machine) = guard(user_made_tree(), HelperHost::Windows, None);
    g.prepare_folders(Path::new(DATA), OnUntrusted::Purge).unwrap();
    let c = calls(&machine);
    assert_eq!(
        c.iter().filter(|x| x.starts_with("remove_tree")).count(),
        1,
        "the data root's removal takes the rest: {c:?}"
    );
    let first_removal = c.iter().position(|x| x.starts_with("remove_tree")).unwrap();
    assert_eq!(c[first_removal], format!("remove_tree {DATA}"), "{c:?}");
    assert!(
        c[..first_removal].iter().all(|x| x.starts_with("lock_dir")),
        "only the way is locked before: {c:?}"
    );
}

#[test]
fn the_data_root_a_user_made_is_refused_at_start_and_removed_by_repair() {
    let (g, machine) = guard(user_made_tree(), HelperHost::Windows, None);
    let e = g.ensure_root(Path::new(DATA), OnUntrusted::Refuse).unwrap_err().to_string();
    assert!(e.contains("Repair the helper"), "{e}");
    assert!(
        calls(&machine)
            .iter()
            .all(|x| !x.starts_with("remove_tree") && !x.starts_with("icacls")),
        "nothing is touched at start"
    );
    // Repair removes the whole tree (config.json in it cannot be trusted either) and locks a fresh one.
    g.ensure_root(Path::new(DATA), OnUntrusted::Purge).unwrap();
    assert!(calls(&machine).contains(&format!("remove_tree {DATA}")));
}

#[test]
fn a_protected_folder_is_locked_again_but_never_emptied() {
    let (g, machine) = guard(windows_machine(|_| locked()), HelperHost::Windows, None);
    g.ensure_root(Path::new(TOOLS), OnUntrusted::Purge).unwrap();
    assert!(calls(&machine).iter().all(|x| !x.starts_with("remove_tree")));
}

#[test]
fn no_acl_change_happens_when_a_system_ancestor_fails() {
    // ProgramData (the base) lets Users delete its children: a user could swap Dyno for a junction. Nothing is made, locked,
    // moved or removed, whichever policy asks, and Repair is not named when Repair itself is what failed.
    for (policy, repair_named) in [(OnUntrusted::Purge, false), (OnUntrusted::Refuse, true)] {
        let mut m = windows_machine(|_| locked());
        let mut pd = stock_program_data();
        if let Security::Windows(d) = &mut pd.security {
            d.dacl.as_mut().unwrap().push(Ace::allow(SID_USERS, right::DELETE_CHILD));
        }
        m.put("C:\\ProgramData", pd);
        let (g, machine) = guard(m, HelperHost::Windows, None);
        let e = g.ensure_root(Path::new(TOOLS), policy).unwrap_err().to_string();
        assert!(e.contains("DELETE_CHILD"), "{e}");
        assert_eq!(e.contains("Repair the helper"), repair_named, "{e}");
        assert!(calls(&machine).is_empty(), "nothing is touched: {:?}", calls(&machine));
    }
    // Also a fault above the base.
    let mut m = windows_machine(|_| locked());
    let mut root = stock_drive_root();
    if let Security::Windows(d) = &mut root.security {
        d.dacl.as_mut().unwrap().push(Ace::allow(SID_USERS, right::WRITE_DAC));
    }
    m.put("C:\\", root);
    let (g, machine) = guard(m, HelperHost::Windows, None);
    assert!(g.prepare_folders(Path::new(DATA), OnUntrusted::Purge).is_err());
    assert!(calls(&machine).is_empty());
}

#[test]
fn an_untrusted_vendor_folder_on_the_way_is_moved_aside_and_never_removed_or_followed() {
    // A user made C:\ProgramData\Dyno (they own it, so they can re-permission it) with a Helper tree inside.
    let user = "S-1-5-21-111-222-333-1001";
    let mut m = windows_machine(|_| locked());
    let mut dyno = inherited_from_program_data();
    if let Security::Windows(sd) = &mut dyno.security {
        sd.owner = user.into();
    }
    m.put("C:\\ProgramData\\Dyno", dyno);
    let (g, machine) = guard(m, HelperHost::Windows, None);
    g.ensure_root(Path::new(TOOLS), OnUntrusted::Purge).unwrap();
    let c = calls(&machine);
    let aside = c
        .iter()
        .position(|x| x == "rename_aside C:\\ProgramData\\Dyno")
        .expect("moved aside");
    let made = c.iter().position(|x| x == "create_dir C:\\ProgramData\\Dyno").expect("made again");
    assert!(aside < made, "{c:?}");
    assert!(
        c.iter().all(|x| !x.starts_with("remove_tree")),
        "what a user made is moved aside, never deleted as SYSTEM: {c:?}"
    );
    // The new folders are the helper's: tools is made fresh under the new Helper.
    assert!(c.contains(&format!("create_dir {TOOLS}")), "{c:?}");
}

#[test]
fn a_user_owned_entry_deep_in_a_locked_tools_folder_makes_the_folder_untrusted() {
    // The attack: the user pre-created tools\\smartmontools-7.5\\bin before the folder was locked; the lock does not change
    // an owner, so the entry is still the user's.
    let user = "S-1-5-21-111-222-333-1001";
    let mut m = windows_machine(|_| locked());
    let mut bin = locked();
    if let Security::Windows(d) = &mut bin.security {
        d.owner = user.into();
    }
    m.put("C:\\ProgramData\\Dyno\\Helper\\tools\\smartmontools-7.5\\bin", bin);
    let (g, machine) = guard(m, HelperHost::Windows, None);
    let e = g.ensure_root(Path::new(TOOLS), OnUntrusted::Refuse).unwrap_err().to_string();
    assert!(e.contains("bin") && e.contains("owner"), "{e}");
    g.ensure_root(Path::new(TOOLS), OnUntrusted::Purge).unwrap();
    assert!(calls(&machine).contains(&format!("remove_tree {TOOLS}")));
}

fn unix_machine(owner: u32, mode: u32) -> FakePort {
    use paddock_service::privileged::files::FileKind;
    use paddock_service::privileged::trust::Node;
    let mut p = FakePort {
        style: Style::Unix,
        nodes: Default::default(),
        links: Default::default(),
        calls: vec![],
    };
    let dir = |o, m| Node {
        kind: FileKind::Dir,
        security: Security::Unix {
            owner: o,
            mode: m,
            write_acl: false,
        },
    };
    for d in ["/", "/var", "/var/lib", "/var/lib/dyno-helper"] {
        p.put(d, dir(0, 0o755));
    }
    p.put("/var/lib/dyno-helper/tools", dir(owner, mode));
    p
}

#[test]
fn unix_folders_are_root_owned_0755_and_one_loose_or_somebody_elses_is_emptied() {
    let tools = "/var/lib/dyno-helper/tools";
    let (g, machine) = guard(unix_machine(0, 0o755), HelperHost::Linux, None);
    g.ensure_root(Path::new(tools), OnUntrusted::Purge).unwrap();
    assert!(calls(&machine).contains(&format!("lock_dir {tools}")));
    assert!(
        calls(&machine).iter().all(|x| !x.starts_with("remove_tree")),
        "a protected folder is kept"
    );
    for (owner, mode) in [(501, 0o755), (0, 0o777)] {
        let (g, machine) = guard(unix_machine(owner, mode), HelperHost::Macos, None);
        g.ensure_root(Path::new(tools), OnUntrusted::Purge).unwrap();
        assert!(calls(&machine).contains(&format!("remove_tree {tools}")), "{owner} {mode:o}");
    }
}

#[test]
fn on_unix_a_program_behind_a_link_is_refused_except_the_os_s_own() {
    // macOS and a helper-owned root: the resolved path must be the requested one.
    let (g, _m) = guard(
        unix_machine(0, 0o755),
        HelperHost::Macos,
        Some("/private/var/lib/dyno-helper/tools/smartctl"),
    );
    let e = g
        .open_checked("/var/lib/dyno-helper/tools/smartctl", Some(Path::new("/var/lib/dyno-helper/tools")))
        .err()
        .unwrap();
    assert!(e.to_string().contains("leads somewhere else"), "{e}");
    // Linux, the distribution's smartctl (merged /usr): resolved first, then checked from the resolved path.
    let mut m = unix_machine(0, 0o755);
    use paddock_service::privileged::files::FileKind;
    use paddock_service::privileged::trust::Node;
    for d in ["/usr", "/usr/bin"] {
        m.put(
            d,
            Node {
                kind: FileKind::Dir,
                security: Security::Unix {
                    owner: 0,
                    mode: 0o755,
                    write_acl: false,
                },
            },
        );
    }
    m.put(
        "/usr/bin/smartctl",
        Node {
            kind: FileKind::File,
            security: Security::Unix {
                owner: 0,
                mode: 0o755,
                write_acl: false,
            },
        },
    );
    let (g, _m) = guard(m, HelperHost::Linux, Some("/usr/bin/smartctl"));
    g.open_checked("/usr/sbin/smartctl", None).unwrap();
}

/// Trusts every path: for tests of code that only needs a guard that says yes (the refusals are tested above).
struct Anything;

impl TrustPort for Anything {
    fn style(&self) -> Style {
        Style::Unix
    }
    fn node(&mut self, p: &str) -> io::Result<Option<paddock_service::privileged::trust::Node>> {
        use paddock_service::privileged::files::FileKind;
        // "/h/..." are made-up folders of the tests that never touch the disk; anything else must really be there.
        if !p.starts_with("/h") && !Path::new(p).exists() {
            return Ok(None);
        }
        let kind = if p.ends_with(".exe") || p.ends_with("smartctl") {
            FileKind::File
        } else {
            FileKind::Dir
        };
        Ok(Some(paddock_service::privileged::trust::Node {
            kind,
            security: Security::Unix {
                owner: 0,
                mode: 0o755,
                write_acl: false,
            },
        }))
    }
    fn canonical(&mut self, p: &str) -> io::Result<String> {
        Ok(p.to_string())
    }
    fn walk(&mut self, _p: &str, _limit: usize) -> io::Result<Vec<(String, paddock_service::privileged::trust::Node)>> {
        Ok(Vec::new())
    }
    fn create_dir(&mut self, p: &str) -> io::Result<()> {
        if p.starts_with("/h") {
            return Ok(());
        }
        std::fs::create_dir(p)
    }
    fn lock_dir(&mut self, _p: &str) -> io::Result<()> {
        Ok(())
    }
    fn rename_aside(&mut self, p: &str) -> io::Result<String> {
        Ok(p.to_string())
    }
    fn sweep_aside(&mut self, _p: &str, _keep: usize) -> io::Result<Vec<String>> {
        Ok(Vec::new())
    }
    fn remove_tree(&mut self, p: &str) -> io::Result<()> {
        match std::fs::remove_dir_all(p) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}

/// A guard that trusts every path, over the real temporary folders of a test.
pub(crate) fn trusting() -> Arc<TrustGuard> {
    Arc::new(TrustGuard::new(
        HelperHost::Linux,
        Box::new(Anything),
        Box::new(Opener { redirect: None }),
        Path::new("/"),
    ))
}

#[test]
fn a_clean_install_folder_that_still_inherits_users_write_is_locked_again() {
    // After Dyno's own install on the VM: owner Administrators, but Users:(I)(CI)(WD,AD,WEA,WA) inherited. A user can add a DLL
    // next to smartctl.exe or a file next to the staged installer.
    let mut m = windows_machine(|_| locked());
    for d in [TOOLS, STAGING] {
        m.put(d, inherited_from_program_data());
    }
    let (g, machine) = guard(m, HelperHost::Windows, None);
    for dir in [TOOLS, STAGING] {
        assert!(g.verify_dir(Path::new(dir), Path::new(dir)).is_err(), "{dir}");
        g.ensure_root(Path::new(dir), OnUntrusted::Purge).unwrap();
        g.verify_dir(Path::new(dir), Path::new(dir)).unwrap();
    }
    assert!(calls(&machine).iter().filter(|c| c.starts_with("lock_dir")).count() >= 2);
}

#[test]
fn resetting_the_staging_folder_empties_it_and_locks_it_again() {
    let (g, machine) = guard(windows_machine(|_| locked()), HelperHost::Windows, None);
    g.reset_root(Path::new(STAGING)).unwrap();
    let c = calls(&machine);
    let removed = c.iter().position(|x| x == &format!("remove_tree {STAGING}")).expect("emptied");
    let made = c.iter().rposition(|x| x == &format!("create_dir {STAGING}")).expect("made again");
    assert!(removed < made, "{c:?}");
}
