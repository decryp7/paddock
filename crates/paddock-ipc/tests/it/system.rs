//! The system endpoint (ADR 0027 §2): the caller's identity from OS facts, the client's check of the server, the access
//! lists. Nothing here touches the user's machine: sockets live in temporary folders, pipes have random names.

use paddock_ipc::endpoint::{self, Access, Listener, Stream, SystemConnectError};
use paddock_ipc::identity::PeerSource;
use paddock_ipc::{AccountId, Untrusted};
use std::io::{Read, Write};
use std::time::Duration;

const T: Duration = Duration::from_secs(10);

#[test]
fn the_helper_access_list_names_system_as_owner_and_the_old_service_list_does_too() {
    for a in [
        Access::system_for_all_users(),
        Access::service_for_interactive_users(),
    ] {
        assert!(a.sddl.starts_with("O:SYD:P("), "{}", a.sddl);
    }
    let all = Access::system_for_all_users();
    assert_eq!(all.mode, 0o666);
    assert!(all.sddl.contains("(D;;GA;;;NU)") && all.sddl.contains("(A;;GRGW;;;IU)"));
}

/// A connected pair; the server end has read one byte, so Windows may impersonate.
fn served_pair() -> (Stream, Stream) {
    let (mut server, mut client) = endpoint::pair().unwrap();
    client.write_all(b"x").unwrap();
    server.read_exact(&mut [0u8; 1]).unwrap();
    (server, client)
}

#[cfg(unix)]
#[test]
fn the_peer_facts_come_from_the_kernel_not_from_a_pid() {
    use paddock_ipc::identity::{AdminRule, caller_of};
    // SAFETY: plain getters.
    #[allow(unsafe_code)]
    let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
    let (server, _client) = served_pair();
    let raw = server.raw_peer().unwrap();
    assert_eq!(raw.account, AccountId::Uid(uid));
    match &raw.admin {
        paddock_ipc::identity::AdminEvidence::Groups(g) => assert!(g.contains(&gid), "{g:?}"),
        other => panic!("{other:?}"),
    }
    // The pid is only for the audit line.
    assert_eq!(raw.audit.pid_for_log(), Some(std::process::id()));
    let caller = caller_of(&server, &AdminRule::new([gid])).unwrap();
    assert!(caller.is_admin(), "a member of a configured admin group");
    assert_eq!(
        caller_of(&server, &AdminRule::default())
            .unwrap()
            .is_admin(),
        uid == 0
    );
}

#[cfg(windows)]
#[test]
fn the_peer_facts_come_from_the_callers_own_token() {
    let (server, _client) = served_pair();
    let raw = server.raw_peer().unwrap();
    assert!(
        matches!(&raw.account, AccountId::Sid(s) if s.starts_with("S-1-")),
        "{:?}",
        raw.account
    );
    let admin = match raw.admin {
        paddock_ipc::identity::AdminEvidence::Token(admin) => admin,
        other => panic!("{other:?}"),
    };
    assert!(raw.audit.program_for_log().is_none());
    // The local VM runs this binary as several accounts and says what each must see (never set in CI): a standard user is
    // no administrator, a full or UAC-filtered administrator token is one (D9), SYSTEM is one.
    if let Ok(expected) = std::env::var("PADDOCK_EXPECT_ADMIN") {
        assert_eq!(admin, expected == "yes", "account {:?}", raw.account);
    }
    if let Ok(sid) = std::env::var("PADDOCK_EXPECT_SID") {
        assert_eq!(raw.account, AccountId::Sid(sid));
    }
}

/// Measured on Windows 11 (build 26300): the OS lets the server impersonate a client that has connected but sent nothing,
/// although the documentation of `ImpersonateNamedPipeClient` says a first read is needed. `PeerError::NotReady` maps the
/// documented error code for systems that behave as documented.
#[cfg(windows)]
#[test]
fn the_caller_is_identified_as_soon_as_it_has_connected() {
    let mut l = Listener::bind().unwrap();
    let name = l.name().to_string();
    let c = std::thread::spawn(move || Stream::connect(&name, T));
    let (server, _) = l.accept(T).unwrap();
    let _client = c.join().unwrap().unwrap();
    let raw = server.raw_peer();
    assert!(
        matches!(raw, Ok(_) | Err(paddock_ipc::PeerError::NotReady)),
        "{raw:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_socket_in_a_folder_the_service_does_not_own_is_not_trusted() {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: a plain getter.
    #[allow(unsafe_code)]
    let me = unsafe { libc::geteuid() };
    let dir = tempfile::tempdir().unwrap();
    let name = dir
        .path()
        .join("helper.sock")
        .to_string_lossy()
        .into_owned();
    // A folder open to others cannot hold a system socket.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
    let e = Listener::bind_system(&name, &Access::system_for_all_users())
        .err()
        .unwrap();
    assert!(matches!(e, Untrusted::FolderWritable { .. }), "{e}");
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut listener = Listener::bind_system(&name, &Access::system_for_all_users()).unwrap();
    // The folder is this account's, not root's: a client that expects root refuses before it connects (unless root runs this).
    if me != 0 {
        let e = Stream::connect_system(&name, T).err().unwrap();
        assert!(
            matches!(e, SystemConnectError::Untrusted(Untrusted::FolderOwner { uid }) if uid == me),
            "{e}"
        );
        assert!(e.to_string().contains("not by the service's account"));
    }
    assert!(
        listener.accept(Duration::from_millis(20)).is_err(),
        "nothing connected"
    );
}

#[cfg(unix)]
#[test]
fn a_system_endpoint_in_a_missing_or_linked_folder_is_refused_with_the_cause() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir
        .path()
        .join("no/such/helper.sock")
        .to_string_lossy()
        .into_owned();
    let e = Listener::bind_system(&missing, &Access::system_for_all_users())
        .err()
        .unwrap();
    assert!(matches!(e, Untrusted::Unreadable { .. }), "{e}");
    // A symlink as the last folder component is not followed.
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let via = link.join("helper.sock").to_string_lossy().into_owned();
    let e = Stream::connect_system(&via, T).err().unwrap();
    assert!(
        matches!(
            e,
            SystemConnectError::Untrusted(Untrusted::Unreadable { .. })
        ),
        "{e}"
    );
    // A name without a folder part means the current folder, which is checked like any other.
    assert!(Stream::connect_system("helper.sock", T).is_err());
}

#[cfg(windows)]
#[test]
fn a_pipe_not_owned_by_system_is_refused_before_anything_is_sent() {
    // The test's own account creates the pipe, so it owns it: the same thing a squatter's pipe looks like.
    let mut squatter = Listener::bind().unwrap();
    let name = squatter.name().to_string();
    let t = std::thread::spawn(move || {
        squatter
            .accept(Duration::from_secs(2))
            .err()
            .map(|e| e.kind())
    });
    match Stream::connect_system(&name, T) {
        Err(e) => assert!(
            matches!(&e, SystemConnectError::Untrusted(Untrusted::PipeOwner { owner }) if owner != "S-1-5-18"),
            "{e}"
        ),
        // Run as SYSTEM (the local VM does), the test's own pipe is SYSTEM's and a client rightly trusts it.
        Ok(_) => assert_eq!(
            std::env::var("PADDOCK_EXPECT_SID").as_deref(),
            Ok("S-1-5-18")
        ),
    }
    let _ = t.join();
}

#[cfg(windows)]
#[test]
fn only_system_can_create_a_system_owned_pipe_and_the_first_instance_wins() {
    let name = format!(r"\\.\pipe\paddock-test-{}", std::process::id());
    match Listener::bind_system(&name, &Access::system_for_all_users()) {
        // Not SYSTEM: a user cannot name SYSTEM as owner (ERROR_INVALID_OWNER, 1307).
        Err(e) => assert!(e.to_string().contains("1307"), "{e}"),
        // SYSTEM (the local VM runs this test as LocalSystem): the pipe is owned by SYSTEM, so a client trusts it.
        Ok(mut listener) => {
            let server = std::thread::spawn(move || listener.accept(T).map(|_| ()));
            let client = Stream::connect_system(&name, T);
            assert!(client.is_ok(), "{:?}", client.err());
            server.join().unwrap().unwrap();
        }
    }
    // The first instance wins: a second pipe with the same name fails, it never shares.
    let name = format!(r"\\.\pipe\paddock-test-first-{}", std::process::id());
    let _first = Listener::bind_fixed(&name, &Access::owner_only()).unwrap();
    assert!(Listener::bind_system(&name, &Access::owner_only()).is_err());
}

#[test]
fn a_connection_error_reads_as_a_missing_service() {
    let e = SystemConnectError::Connect(std::io::Error::other("refused"));
    assert!(e.to_string().contains("could not be reached") && e.to_string().contains("refused"));
}
