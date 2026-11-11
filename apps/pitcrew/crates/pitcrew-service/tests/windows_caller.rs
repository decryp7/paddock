#![allow(missing_docs)]
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The release blocker of 2026-11-11 on a real Windows machine: the real pipe, the real caller token, the real folder ACLs.
//!
//! These tests need two kinds of caller and a machine to change, so they are `#[ignore]`d everywhere and run on the Windows test VM
//! with `--include-ignored` (`scripts/vm-lock-run.sh`; the standard user's half through `vm-win.sh job`, the elevated half through
//! `vm-win.sh ssh --admin`). `PITCREW_TEST_EXPECT=standard|elevated` says who runs the test, and a test refuses to run without it
//! (a probe that cannot say who it is would pass either way, ER-33). See `docs/testing/windows-test-vm.md`.

use paddock_ipc::endpoint::Stream;
use paddock_service::privileged::trust::{is_trusted_sid, Ace, Security, TrustPort};
use paddock_service::privileged::trust_real::RealTrust;
use pitcrew_core::config::RunnerConfig;
use pitcrew_core::names::RunnerName;
use pitcrew_core::protocol::{read_frame, write_frame, ErrorCode, Request, Response, PROTOCOL_VERSION};
use pitcrew_service::host::Host;
use pitcrew_service::protected::{
    check_adopted_runner_folder, check_runner_folder, ensure_protected_root, forget_adopted, prepare_runner_dir, read_adopted,
    settle_runner_dir, RootOutcome, ADOPTED_FILE,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn expectation() -> String {
    let v = std::env::var("PITCREW_TEST_EXPECT").expect("set PITCREW_TEST_EXPECT=standard or elevated: the test must know who runs it");
    assert!(v == "standard" || v == "elevated", "PITCREW_TEST_EXPECT={v}");
    v
}

#[allow(
    clippy::disallowed_methods,
    reason = "a test builds its scratch path from the OS temp folder (or the VM's test folder) and removes it"
)]
fn scratch(name: &str) -> PathBuf {
    let base = std::env::var_os("PITCREW_TEST_SCRATCH").map_or_else(std::env::temp_dir, PathBuf::from);
    let p = base.join(format!("pitcrew-win-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn connect(name: &str) -> Stream {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        match Stream::connect(name, Duration::from_secs(1)) {
            Ok(s) => return s,
            Err(e) => {
                assert!(std::time::Instant::now() < deadline, "the service did not listen: {e}");
                std::thread::yield_now();
            }
        }
    }
}

fn ask(s: &mut Stream, r: &Request) -> Response {
    write_frame(s, r).unwrap();
    read_frame::<Response>(s).unwrap()
}

fn permission_denied(r: &Response) -> bool {
    matches!(r, Response::Error(e) if e.code == ErrorCode::PermissionDenied)
}

/// The real pipe, the real token: a standard user is refused a change and still reads and operates; an elevated administrator
/// changes.
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=standard|elevated"]
fn the_pipe_refuses_changes_from_a_caller_that_is_not_an_elevated_administrator() {
    let who = expectation();
    let data = scratch("pipe");
    let host = Host::start_at(data.clone(), data.clone(), |e| panic!("the service stopped: {e}")).unwrap();
    let mut s = connect(&pitcrew_service::ipc::pipe_name(&data));
    assert!(matches!(
        ask(
            &mut s,
            &Request::Hello {
                protocol: PROTOCOL_VERSION
            }
        ),
        Response::Hello { .. }
    ));
    assert!(
        matches!(ask(&mut s, &Request::Status), Response::Status { .. }),
        "reading is open to the pipe's users"
    );
    let change = Request::SaveSettings {
        config: pitcrew_core::config::Config::default(),
    };
    let answer = ask(&mut s, &change);
    match who.as_str() {
        "standard" => assert!(permission_denied(&answer), "{answer:?}"),
        _ => assert_eq!(answer, Response::Ok, "an elevated administrator may change settings"),
    }
    // Operating a runner is open to the pipe's users: the answer is "no such runner", never "needs administrator rights".
    let stop = ask(
        &mut s,
        &Request::Stop {
            runner: RunnerName::parse("nobody").unwrap(),
        },
    );
    assert!(!permission_denied(&stop), "{stop:?}");
    // The refusal does not end the connection.
    assert!(matches!(ask(&mut s, &Request::Status), Response::Status { .. }));
    drop(s);
    host.stop();
    let _ = std::fs::remove_dir_all(&data);
}

/// The installed service (SYSTEM, `\\.\pipe\pitcrew`, `PITCREW_TEST_INSTALLED=1`): an interactive user can talk to it with the mask the
/// app uses, and cannot host a pipe of that name (no `FILE_CREATE_PIPE_INSTANCE`), so cannot receive an elevated caller.
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=standard|elevated, and the service installed (PITCREW_TEST_INSTALLED=1)"]
fn an_interactive_user_can_use_the_installed_pipe_but_not_host_one() {
    let who = expectation();
    // The squat attempt is what an interactive standard user could do; an administrator may create an instance (the ACL gives
    // Administrators full control), so the row proves nothing when run as one.
    assert_eq!(
        who, "standard",
        "this row must run as the standard user (PITCREW_TEST_EXPECT=standard)"
    );
    assert_eq!(
        std::env::var("PITCREW_TEST_INSTALLED").as_deref(),
        Ok("1"),
        "install the service first and set PITCREW_TEST_INSTALLED=1"
    );
    let mut file = pitcrew_core::service_paths::open_pipe_options()
        .open(pitcrew_service::ipc::PIPE_NAME)
        .unwrap_or_else(|e| panic!("opening the installed pipe as {who} with the app's mask: {e}"));
    write_frame(
        &mut file,
        &Request::Hello {
            protocol: PROTOCOL_VERSION,
        },
    )
    .unwrap();
    assert!(matches!(read_frame::<Response>(&mut file).unwrap(), Response::Hello { .. }));
    write_frame(&mut file, &Request::Status).unwrap();
    assert!(matches!(read_frame::<Response>(&mut file).unwrap(), Response::Status { .. }));
    drop(file);
    // Creating another instance of the installed service's pipe: refused to an interactive user.
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let squat = runtime.block_on(async {
        tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(false)
            .create(pitcrew_service::ipc::PIPE_NAME)
    });
    match squat {
        Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}"),
        Ok(_) => panic!("a standard user created an instance of the service's pipe: it would receive an elevated caller"),
    }
}

/// The folder the service makes: owned by nobody but the trusted accounts, nothing but read and execute for the others.
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=standard|elevated"]
fn a_new_data_folder_is_protected_from_the_moment_it_exists() {
    let _ = expectation();
    let root = scratch("created");
    let dir = root.join("Pitcrew");
    let mut machine = RealTrust;
    let outcome = ensure_protected_root(&mut machine, &list_real, &dir, &["pitcrew.toml"], &["logs"]).unwrap();
    assert_eq!(outcome, RootOutcome::Created);
    let node = machine.node(&dir.display().to_string()).unwrap().unwrap();
    let Security::Windows(d) = node.security else {
        panic!("not a Windows descriptor")
    };
    let dacl = d.dacl.expect("a DACL");
    assert!(!dacl.is_empty());
    const WRITE_LIKE: u32 = 0x0000_0002 | 0x0000_0004 | 0x0000_0040 | 0x0001_0000 | 0x0004_0000 | 0x0008_0000 | 0x4000_0000 | 0x1000_0000;
    for Ace { sid, mask, allow, .. } in &dacl {
        if *allow && !is_trusted_sid(sid) {
            assert_eq!(mask & WRITE_LIKE, 0, "{sid} may write: mask {mask:#x}");
        }
    }
    assert!(
        dacl.iter().any(|a| a.sid == "S-1-5-18" && a.allow) && dacl.iter().any(|a| a.sid == "S-1-5-32-544" && a.allow),
        "SYSTEM and Administrators are in it: {dacl:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A folder (or a configuration in it) made by a user before the service existed is never taken as it is. The folder is the one
/// `PITCREW_TEST_SQUATTED` names (directly below `C:\ProgramData`, as the real one is, so its ancestors are trusted and only what
/// the user made in it can fail): the standard user's run makes it (and cannot take it over), the elevated run, after it, finds the
/// user's configuration in it and moves it aside (an administrator's own files would be trusted, so it cannot make the folder).
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=standard|elevated"]
fn a_data_folder_a_user_made_first_is_not_trusted() {
    let who = expectation();
    let dir = PathBuf::from(std::env::var_os("PITCREW_TEST_SQUATTED").expect("PITCREW_TEST_SQUATTED names the folder"));
    if who == "standard" {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pitcrew.toml"), "schema_version = 1\n").unwrap();
    }
    let result = ensure_protected_root(&mut RealTrust, &list_real, &dir, &["pitcrew.toml"], &["logs"]);
    match (who.as_str(), &result) {
        ("elevated", Ok(RootOutcome::MovedAside(aside, why))) => {
            assert!(Path::new(aside).exists(), "{aside}");
            assert!(why.contains("pitcrew.toml"), "{why}");
        }
        // A standard user cannot change the owner: the folder stays untrusted and the service would not start.
        ("standard", Err(e)) => assert!(e.contains("Locking"), "{e}"),
        other => panic!("{other:?}"),
    }
}

fn runner(dir: &Path, session: &str) -> RunnerConfig {
    let mut cfg: RunnerConfig = toml::from_str(&format!(
        "name = \"r\"\nkind = \"gitea-runner\"\ndir = '{}'\nsession = \"{session}\"\n",
        dir.display()
    ))
    .unwrap();
    cfg.name = RunnerName::parse("r").unwrap();
    cfg
}

/// The data folder of the settle test: directly below `C:\ProgramData`, as the real one is.
fn settle_root() -> PathBuf {
    PathBuf::from(
        std::env::var_os("PITCREW_TEST_SETTLE_ROOT").expect("PITCREW_TEST_SETTLE_ROOT names the data folder (below C:\\ProgramData)"),
    )
}

/// The children of a folder as the guard lists them on the real machine.
fn list_real(dir: &str) -> std::io::Result<Vec<String>> {
    std::fs::read_dir(dir)?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect()
}

fn guard(dir: &Path) -> Result<(), String> {
    check_runner_folder(&mut RealTrust, &list_real, dir)
}

/// ADR 0004, second amendment, measured on a real machine. The elevated run lays a service runner's folder out as an older
/// Pitcrew did (made in `C:\ProgramData`, so it inherited what that gives users), shows what the guard says before and after the
/// service settles it, and the standard run, after it, shows what a user can and cannot do there and that a folder the user owns is
/// refused.
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=standard|elevated; the elevated run first"]
fn settled_service_runner_folders_pass_the_guard_and_a_user_folder_never_does() {
    let who = expectation();
    let root = settle_root();
    let svc = root.join("runners").join("svc");
    if who == "elevated" {
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(svc.join("work")).unwrap();
        let before = guard(&svc);
        println!("MEASURED before (an older Pitcrew's folder): guard = {before:?}");
        assert!(
            before.is_err(),
            "the old layout should not pass the guard: that is the upgrade problem"
        );
        let out = ensure_protected_root(&mut RealTrust, &list_real, &root, &["pitcrew.toml"], &["logs"]).unwrap();
        println!("MEASURED data folder: {out:?}");
        let line = settle_runner_dir(&mut RealTrust, &list_real, &root, &runner(&svc, "service"));
        println!("MEASURED settle service runner: {line:?}");
        let after = guard(&svc);
        println!("MEASURED after the service settled the runner folder: guard = {after:?}");
        assert!(
            after.is_ok(),
            "after settling, the guard accepts the service runner's folder: {after:?}"
        );
    } else {
        // A folder the user owns is refused whole, whatever is in it.
        let mine = std::path::PathBuf::from(std::env::var_os("PITCREW_TEST_SCRATCH").expect("PITCREW_TEST_SCRATCH")).join("user-runner");
        std::fs::create_dir_all(mine.join("work")).unwrap();
        let refused = guard(&mine).unwrap_err();
        println!("MEASURED standard user's own folder: {refused}");
        assert!(refused.contains("not a folder only administrators can change"), "{refused}");
        // What a standard user cannot do in the service runner's folder or beside it.
        assert!(
            std::fs::create_dir(svc.join("evil")).is_err(),
            "a standard user made a folder in a service runner's folder"
        );
        assert!(
            std::fs::write(svc.join("evil.txt"), b"x").is_err(),
            "a standard user wrote in a service runner's folder"
        );
        assert!(
            std::fs::create_dir(root.join("runners").join("evil")).is_err(),
            "a standard user made a runner folder"
        );
        println!("MEASURED standard user: refused in the service runner's folder and in runners\\");
    }
}

/// P1-1, written first and ignored until the shared adopt-a-tree mechanism lands (ADR `adopt-tree`); until then the service blocks
/// such a runner whatever its structure (it did not make the folder: `check_adopted_runner_folder`). The elevated run lays a service
/// runner out as an older Pitcrew did, with a folder two levels down (`svc\work\job`) that inherited the right `C:\ProgramData`
/// gives users to add to it; after the service has settled the runner's folder and adopted its tree, the standard user must not be
/// able to create anything in it.
#[test]
#[ignore = "VM: waits for the shared adopt-a-tree mechanism in paddock-service (docs/adr-adopt-tree); the runner stays blocked until then"]
fn a_grandchild_of_a_settled_service_runner_folder_is_not_writable_by_a_user() {
    let who = expectation();
    let root = PathBuf::from(std::env::var_os("PITCREW_TEST_GRANDCHILD_ROOT").expect("PITCREW_TEST_GRANDCHILD_ROOT names the data folder"));
    let svc = root.join("runners").join("svc");
    if who == "elevated" {
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(svc.join("work").join("job")).unwrap();
        ensure_protected_root(&mut RealTrust, &list_real, &root, &["pitcrew.toml"], &["logs"]).unwrap();
        settle_runner_dir(&mut RealTrust, &list_real, &root, &runner(&svc, "service"));
        // The shared mechanism's call goes here when it lands: it adopts the runner's whole tree, then the guard may accept it.
    } else {
        assert!(
            std::fs::create_dir(svc.join("work").join("job").join("evil")).is_err(),
            "a standard user made a folder two levels inside a settled service runner's folder"
        );
    }
}

/// What `os_prepare` records and `os_folder_guard` reads, on a real NTFS volume (round 5 P2-1; `os_prepare` and `os_folder_guard`
/// themselves act only as LocalSystem, so this row runs their steps as the elevated administrator): the service makes a runner's
/// folder (recorded first, then bound to the file it made), the guard accepts it; the folder renamed away and another put at the same
/// name is refused; removing the runner forgets the entry.
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=elevated"]
fn the_folder_the_service_made_is_accepted_and_a_replacement_under_its_name_is_not() {
    assert_eq!(expectation(), "elevated", "this row makes protected folders");
    let root = PathBuf::from(format!("{}-adopt", settle_root().display()));
    let _ = std::fs::remove_dir_all(&root);
    let out = ensure_protected_root(&mut RealTrust, &list_real, &root, &["pitcrew.toml"], &["logs", "runners"]).unwrap();
    println!("MEASURED data folder: {out:?}");
    let file = root.join(ADOPTED_FILE);
    let dir = root.join("runners").join("made");
    let mut record = |made: &Path, id: Option<paddock_service::privileged::trust::FileIdentity>| -> Result<(), String> {
        let mut list = read_adopted(&file)?;
        match id {
            Some(id) => list.bind(made, id),
            None => list.insert(made),
        }
        pitcrew_service::store::write_atomic(&file, &list.to_json()).map_err(|e| e.to_string())
    };
    let line = prepare_runner_dir(&mut RealTrust, &dir, &mut record).unwrap();
    println!("MEASURED prepare: {line:?}");
    let accepts = |dir: &Path| check_adopted_runner_folder(&mut RealTrust, &list_real, &read_adopted(&file).unwrap(), dir);
    let accepted = accepts(&dir);
    println!("MEASURED guard on the folder the service made: {accepted:?}");
    assert!(accepted.is_ok(), "{accepted:?}");
    // Swapped: the made folder moves away, another (same protections, made by the same account) takes its name.
    std::fs::rename(&dir, root.join("runners").join("moved")).unwrap();
    std::fs::create_dir(&dir).unwrap();
    let swapped = accepts(&dir).unwrap_err();
    println!("MEASURED guard on a replacement: {swapped}");
    assert!(swapped.contains("replaced") || swapped.contains("never bound"), "{swapped}");
    assert_eq!(forget_adopted(&file, &dir), Ok(true));
    assert!(!read_adopted(&file).unwrap().contains(&dir));
    let _ = std::fs::remove_dir_all(&root);
}

/// Round 5 P1-2 and P2-9 on a real folder: a file in `logs` with a second hard link is moved aside, one with a single name stays, and a
/// second start does not move the aside again.
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=elevated"]
fn the_sweep_of_logs_moves_a_hard_linked_file_aside_once() {
    assert_eq!(expectation(), "elevated", "this row makes protected folders");
    let root = PathBuf::from(format!("{}-sweep", settle_root().display()));
    let _ = std::fs::remove_dir_all(&root);
    ensure_protected_root(&mut RealTrust, &list_real, &root, &["pitcrew.toml"], &["logs"]).unwrap();
    let logs = root.join("logs");
    std::fs::write(logs.join("service.log"), b"x").unwrap();
    std::fs::write(logs.join("planted.log"), b"x").unwrap();
    let outside = root.join("outside-name.log");
    std::fs::hard_link(logs.join("planted.log"), &outside).unwrap();
    let first = ensure_protected_root(&mut RealTrust, &list_real, &root, &["pitcrew.toml"], &["logs"]).unwrap();
    println!("MEASURED first start: {first:?}");
    let RootOutcome::Locked(swept) = first else { panic!("{first:?}") };
    assert_eq!(swept.moved.len(), 1, "{swept:?}");
    assert!(
        swept.moved[0].0.ends_with("planted.log") && swept.moved[0].2.contains("hard link"),
        "{swept:?}"
    );
    assert!(logs.join("service.log").exists(), "a file with one name stays");
    let second = ensure_protected_root(&mut RealTrust, &list_real, &root, &["pitcrew.toml"], &["logs"]).unwrap();
    println!("MEASURED second start: {second:?}");
    assert!(matches!(second, RootOutcome::Locked(s) if s.moved.is_empty() && s.failed.is_empty()));
    let _ = std::fs::remove_dir_all(&root);
}

/// Round 5 P1-1 on Windows: a second per-user engine on one folder is refused (the lock is `File::try_lock`, LockFileEx here), and the
/// first keeps working.
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=standard"]
fn a_second_per_user_engine_on_one_folder_is_refused() {
    use pitcrew_service::ipc::ServeMode;
    assert_eq!(expectation(), "standard", "the per-user engine runs as an ordinary user");
    let data = scratch("two-engines");
    let first = Host::start_with(data.clone(), data.clone(), ServeMode::User, |e| panic!("the engine stopped: {e}")).unwrap();
    let before = std::fs::read_to_string(data.join(pitcrew_core::service_paths::USER_ENGINE_PIPE_FILE)).unwrap();
    let second = Host::start_with(data.clone(), data.clone(), ServeMode::User, |_| {})
        .err()
        .expect("a second engine must be refused");
    println!("MEASURED second engine: {second}");
    assert!(second.to_string().contains("already running"), "{second}");
    let after = std::fs::read_to_string(data.join(pitcrew_core::service_paths::USER_ENGINE_PIPE_FILE)).unwrap();
    assert_eq!(before, after, "the refused engine did not touch pipe.name");
    assert!(
        pitcrew_core::service_paths::open_pipe_options().open(before.trim()).is_ok(),
        "the first engine's pipe still answers"
    );
    first.stop();
    let _ = std::fs::remove_dir_all(&data);
}

/// The per-user engine (ADR 0004, second amendment) as the standard user runs it: its pipe is owned by that user and the app's
/// owner check accepts it only when told it is the user's own engine; it serves its own user every request (no administrator
/// needed for their own runners), and the SYSTEM service's rule for the same request is the opposite.
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=standard"]
fn a_per_user_engine_is_owned_by_its_user_and_serves_them_every_request() {
    use pitcrew_core::service_paths::pipe_owner_ok;
    use pitcrew_service::ipc::ServeMode;
    use pitcrew_service::platform::win_session::{current_user_sid, pipe_owner_sid};
    let who = expectation();
    assert_eq!(who, "standard", "this row is the standard user's engine");
    let data = scratch("user-engine");
    let host = Host::start_with(data.clone(), data.clone(), ServeMode::User, |e| panic!("the engine stopped: {e}")).unwrap();
    let name = pitcrew_service::ipc::read_user_pipe_name(&data).expect("the engine recorded its pipe name");
    let mut s = connect(&name);
    assert!(matches!(
        ask(
            &mut s,
            &Request::Hello {
                protocol: PROTOCOL_VERSION
            }
        ),
        Response::Hello { .. }
    ));
    let change = Request::SaveSettings {
        config: pitcrew_core::config::Config::default(),
    };
    assert_eq!(
        ask(&mut s, &change),
        Response::Ok,
        "a user changes their own runners without an administrator"
    );
    drop(s);
    let file = pitcrew_core::service_paths::open_pipe_options().open(&name).unwrap();
    let owner = pipe_owner_sid(&file).unwrap();
    let me = current_user_sid().unwrap();
    println!("MEASURED per-user engine pipe owner = {owner}; this account = {me}");
    assert!(pipe_owner_ok(&owner, Some(&me)), "the client accepts its own engine's pipe");
    assert!(
        !pipe_owner_ok(&owner, None),
        "and does not accept a user-owned pipe as the service's"
    );
    drop(file);
    host.stop();
    let _ = std::fs::remove_dir_all(&data);
}

fn engine_file() -> PathBuf {
    PathBuf::from(
        std::env::var_os("PITCREW_TEST_ENGINE_FILE").expect("PITCREW_TEST_ENGINE_FILE names the file the engine's pipe name goes in"),
    )
}

/// The standard user's engine, held open for the other account's row: it writes its pipe name, signals ready, and waits (at most
/// three minutes) for the other row to finish.
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=standard, run beside another_account_cannot_reach_a_per_user_engine"]
fn a_per_user_engine_is_held_open_for_another_account() {
    use pitcrew_service::ipc::ServeMode;
    assert_eq!(expectation(), "standard", "this row is the standard user's engine");
    let file = engine_file();
    let data = scratch("held-engine");
    let host = Host::start_with(data.clone(), data.clone(), ServeMode::User, |e| panic!("the engine stopped: {e}")).unwrap();
    let name = pitcrew_service::ipc::read_user_pipe_name(&data).expect("the engine recorded its pipe name");
    std::fs::write(&file, &name).unwrap();
    std::fs::write(file.with_extension("ready"), b"1").unwrap();
    let done = file.with_extension("done");
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    while !done.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500)); // conformance: hang-guard — polls for the other account's done marker, bounded by the 180 s deadline above
    }
    assert!(done.exists(), "the other account's row did not finish in time");
    host.stop();
    let _ = std::fs::remove_dir_all(&data);
}

/// Another account (here the elevated administrator: the VM has two accounts) can neither connect to the standard user's engine pipe
/// nor create an instance of it: its ACL admits only its owner and SYSTEM.
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=elevated, while a_per_user_engine_is_held_open_for_another_account runs as the standard user"]
fn another_account_cannot_reach_a_per_user_engine() {
    assert_eq!(expectation(), "elevated", "this row is the other account");
    let file = engine_file();
    let name = std::fs::read_to_string(&file).expect("the standard user's engine wrote its pipe name");
    let name = name.trim();
    assert!(pitcrew_core::service_paths::is_user_pipe_name(name), "{name}");
    let outcome = std::panic::catch_unwind(|| {
        let connect = pitcrew_core::service_paths::open_pipe_options().open(name);
        match connect {
            Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "connecting: {e}"),
            Ok(_) => panic!("another account connected to a user's engine pipe"),
        }
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let instance = runtime.block_on(async {
            tokio::net::windows::named_pipe::ServerOptions::new()
                .first_pipe_instance(false)
                .create(name)
        });
        match instance {
            Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "creating an instance: {e}"),
            Ok(_) => panic!("another account created an instance of a user's engine pipe"),
        }
    });
    // Whatever happened, the standard user's row is told to stop.
    std::fs::write(file.with_extension("done"), b"1").unwrap();
    if let Err(e) = outcome {
        std::panic::resume_unwind(e);
    }
}

/// An elevated token never starts the per-user engine: it takes requests from the user's own programs.
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=elevated"]
fn a_user_engine_refuses_to_start_from_an_elevated_token() {
    use pitcrew_service::ipc::ServeMode;
    assert_eq!(expectation(), "elevated", "this row is the elevated administrator");
    assert!(
        pitcrew_service::platform::win_session::is_elevated(),
        "the VM run is not elevated: the row proves nothing"
    );
    let data = scratch("elevated-engine");
    let started = Host::start_with(data.clone(), data.clone(), ServeMode::User, |e| panic!("the engine stopped: {e}"));
    let e = started.err().expect("an elevated engine was started");
    assert!(e.to_string().contains("must not run elevated"), "{e}");
    assert!(
        pitcrew_service::ipc::read_user_pipe_name(&data).is_none(),
        "an engine pipe name was recorded"
    );
    let _ = std::fs::remove_dir_all(&data);
}
