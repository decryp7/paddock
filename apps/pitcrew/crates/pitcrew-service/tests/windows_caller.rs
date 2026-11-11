#![allow(missing_docs)]
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

//! The release blocker of 2026-11-11 on a real Windows machine: the real pipe, the real caller token, the real folder ACL.
//!
//! What a caller may do depends on who runs the test: the standard user's desktop session (`vm-win.sh job`) and an elevated
//! administrator (`vm-win.sh ssh --admin`) each check their half; `PITCREW_TEST_EXPECT=standard|elevated` says which one this
//! run is, and the test refuses to run without it (a probe that cannot say who it is would pass either way, ER-33).

use paddock_ipc::endpoint::Stream;
use paddock_service::privileged::trust::{is_trusted_sid, Ace, Security, TrustPort};
use paddock_service::privileged::trust_real::RealTrust;
use pitcrew_core::protocol::{read_frame, write_frame, ErrorCode, Request, Response, PROTOCOL_VERSION};
use pitcrew_service::host::Host;
use pitcrew_service::protected::{ensure_protected_root, RootOutcome};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn expectation() -> String {
    let v = std::env::var("PITCREW_TEST_EXPECT").expect("set PITCREW_TEST_EXPECT=standard or elevated: the test must know who runs it");
    assert!(v == "standard" || v == "elevated", "PITCREW_TEST_EXPECT={v}");
    v
}

fn scratch(name: &str) -> PathBuf {
    let base = std::env::var_os("PITCREW_TEST_SCRATCH").map_or_else(std::env::temp_dir, PathBuf::from);
    let p = base.join(format!("pitcrew-win-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn connect(data: &Path) -> Stream {
    let name = pitcrew_service::ipc::pipe_name(data);
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        match Stream::connect(&name, Duration::from_secs(1)) {
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

/// The real pipe, the real token: a standard user is refused a change and still reads; an elevated administrator changes.
#[test]
fn the_pipe_refuses_changes_from_a_caller_that_is_not_an_elevated_administrator() {
    let who = expectation();
    let data = scratch("pipe");
    let host = Host::start_at(data.clone(), data.clone(), |e| panic!("the service stopped: {e}")).unwrap();
    let mut s = connect(&data);
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
        "standard" => assert!(
            matches!(&answer, Response::Error(e) if e.code == ErrorCode::PermissionDenied),
            "{answer:?}"
        ),
        _ => assert_eq!(answer, Response::Ok, "an elevated administrator may change settings"),
    }
    // The refusal does not end the connection.
    assert!(matches!(ask(&mut s, &Request::Status), Response::Status { .. }));
    drop(s);
    host.stop();
    let _ = std::fs::remove_dir_all(&data);
}

/// The folder the service makes: owned by nobody but the trusted accounts, nothing but read and execute for the others.
#[test]
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
/// `PITCREW_TEST_SQUATTED` names (directly below `C:\ProgramData`, as the real one is, so its ancestors are trusted and only
/// what the user made in it can fail): the standard user's run makes it (and cannot take it over), the elevated run, after it, finds the
/// user's configuration in it and moves it aside (an administrator's own files would be trusted, so it cannot make the folder).
#[test]
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
