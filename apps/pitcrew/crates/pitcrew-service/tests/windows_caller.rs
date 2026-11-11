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
use pitcrew_service::protected::{ensure_protected_root, folder_is_protected, grant_interactive, settle_runner_dir, RootOutcome};
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
    match (who.as_str(), squat) {
        ("standard", Err(e)) => assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}"),
        ("standard", Ok(_)) => panic!("a standard user created an instance of the service's pipe: it would receive an elevated caller"),
        // An administrator may (the ACL gives Administrators full control): the point is the interactive user.
        (_, _) => {}
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
    let outcome = ensure_protected_root(&mut machine, &dir, &["pitcrew.toml"], &["logs"]).unwrap();
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
    let result = ensure_protected_root(&mut RealTrust, &dir, &["pitcrew.toml"], &["logs"]);
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

/// P1-7, measured on a real machine. The elevated run lays the folders out as an older Pitcrew did (made in `C:\ProgramData`, so
/// they inherited what it gives users), prints what the guard says before and after, and settles them; the standard run, after it,
/// shows what the user can and cannot do there.
#[test]
#[ignore = "VM: PITCREW_TEST_EXPECT=standard|elevated; the elevated run first"]
fn settled_runner_folders_protect_service_runners_and_leave_interactive_ones_writable() {
    let who = expectation();
    let root = settle_root();
    let svc = root.join("runners").join("svc");
    let interactive = root.join("runners").join("ia");
    let unprotected = |p: &Path| !folder_is_protected(&mut RealTrust, p);
    if who == "elevated" {
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&svc).unwrap();
        println!(
            "MEASURED before (an older Pitcrew's folder): service runner folder protected = {}",
            !unprotected(&svc)
        );
        assert!(
            unprotected(&svc),
            "the old layout should not pass the guard: that is the upgrade problem"
        );
        let out = ensure_protected_root(&mut RealTrust, &root, &["pitcrew.toml"], &["logs"]).unwrap();
        println!("MEASURED data folder: {out:?}");
        let line = settle_runner_dir(&mut RealTrust, &root, &runner(&svc, "service"), false, &mut grant_interactive);
        println!("MEASURED settle service runner: {line:?}");
        assert!(
            folder_is_protected(&mut RealTrust, &svc),
            "after settling, the guard accepts the service runner's folder: {line:?}"
        );
        // A fresh interactive runner folder, made the way `register` makes it.
        std::fs::create_dir_all(&interactive).unwrap();
        let line = settle_runner_dir(
            &mut RealTrust,
            &root,
            &runner(&interactive, "interactive"),
            true,
            &mut grant_interactive,
        );
        println!("MEASURED settle interactive runner: {line:?}");
        assert!(line.as_deref().is_some_and(|l| l.contains("interactive users write")), "{line:?}");
        assert!(
            !folder_is_protected(&mut RealTrust, &interactive),
            "an interactive folder is writable by users: not protected"
        );
    } else {
        // The user's view: their runner's work and cache folders can be made; the service runner's folder and the shared ones cannot.
        std::fs::create_dir_all(interactive.join("work").join("job")).expect("the interactive runner can make its work folder");
        std::fs::write(interactive.join("work").join("job").join("f.txt"), b"x").expect("and write in it");
        std::fs::create_dir_all(interactive.join("cache")).expect("and its cache folder");
        println!("MEASURED standard user: wrote in the interactive runner's folder");
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
        assert!(
            folder_is_protected(&mut RealTrust, &svc),
            "the guard still accepts the service runner's folder"
        );
        println!("MEASURED standard user: refused in the service runner's folder and in runners\\");
    }
}
