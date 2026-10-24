//! Bodies, tokens, errors, endpoints and the handshake (Debrief's `wire`/`error` and Dyno's `token`/`endpoint`/`host`
//! cases). Nothing here measures time: timeouts are asserted by their outcome.

use crate::common;
use common::GUARD;
use paddock_ipc::endpoint::{self, Access, Listener, Stream};
use paddock_ipc::frame::{self, Frame, FrameLimit, ProtocolVersion};
use paddock_ipc::wire::{Json, decode, encode};
use paddock_ipc::{Client, Host, IpcError, RemoteError, Token, codes, panic_message};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{self, Read, Write};
use std::time::Duration;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Msg {
    name: String,
    args: Json,
    extra: Option<Json>,
}

#[test]
fn json_crosses_as_text_in_postcard_and_as_itself_in_json() {
    let m = Msg {
        name: "x".into(),
        args: json!({"a": [1, 2.5, "é"], "b": null}).into(),
        extra: None,
    };
    let bytes = encode(&m).unwrap();
    assert_eq!(decode::<Msg>(&bytes).unwrap(), m);
    let text = serde_json::to_string(&m).unwrap();
    assert!(text.contains("\"args\":{\"a\""), "{text}");
    assert_eq!(serde_json::from_str::<Msg>(&text).unwrap(), m);
    assert_eq!(m.args.get("b"), Some(&Value::Null));
    assert_eq!(Json::from(json!(1)).into_inner(), json!(1));
}

#[test]
fn a_json_number_crosses_unchanged() {
    // Found by the fuzz target: without exact float parsing a value could change by one unit in the last place each time
    // it crossed the IPC.
    let v: Json = serde_json::from_str::<Value>("55555750000000000179388785717011")
        .unwrap()
        .into();
    let once = decode::<Json>(&encode(&v).unwrap()).unwrap();
    assert_eq!(once, v);
    assert_eq!(decode::<Json>(&encode(&once).unwrap()).unwrap(), v);
}

#[test]
fn bad_bodies_are_errors() {
    let bytes = encode(&7u32).unwrap();
    let mut long = bytes.clone();
    long.push(1);
    assert!(decode::<u32>(&long).unwrap_err().contains("trailing"));
    assert!(decode::<String>(&[200]).is_err());
    assert!(decode::<Json>(&encode(&"{nope").unwrap()).is_err());
}

#[test]
fn typed_errors_roundtrip_and_describe_themselves() {
    let e = RemoteError::new("noModel", "no such model — café").with_data(json!({"limit": 5}));
    assert_eq!(e.to_string(), "no such model — café");
    assert!(e.is("noModel"));
    assert_eq!(
        serde_json::from_str::<RemoteError>(&serde_json::to_string(&e).unwrap()).unwrap(),
        e
    );
    assert_eq!(decode::<RemoteError>(&encode(&e).unwrap()).unwrap(), e);
    let all = [
        IpcError::Spawn("cannot".into()),
        IpcError::Closed("x".into()),
        IpcError::Timeout,
        IpcError::Protocol("p".into()),
        IpcError::TooLarge { len: 2, max: 1 },
        IpcError::Remote(RemoteError::new("c", "r")),
        IpcError::Version("v".into()),
        IpcError::Auth("a".into()),
        IpcError::Denied("d".into()),
        IpcError::io(io::ErrorKind::PermissionDenied, "no"),
    ];
    for e in &all {
        assert!(!e.to_string().is_empty());
    }
    assert_eq!(all[1].to_string(), "the connection ended (x)");
    assert!(IpcError::Timeout.is_process_failure());
    assert!(
        !all[4].is_process_failure()
            && !all[5].is_process_failure()
            && !all[8].is_process_failure()
    );
    assert!(matches!(
        IpcError::from(io::Error::from(io::ErrorKind::BrokenPipe)),
        IpcError::Closed(_)
    ));
    assert!(matches!(
        IpcError::from(io::Error::new(io::ErrorKind::PermissionDenied, "no")),
        IpcError::Io {
            kind: io::ErrorKind::PermissionDenied,
            ..
        }
    ));
    assert_eq!(panic_message(&"s"), "s");
    assert_eq!(panic_message(&String::from("t")), "t");
    assert_eq!(panic_message(&5u8), "unknown panic");
}

#[test]
fn tokens_are_random_roundtrip_through_hex_and_compare_exactly() {
    let t = Token::random().unwrap();
    assert_ne!(t.as_bytes(), Token::random().unwrap().as_bytes());
    let hex = t.to_hex();
    assert_eq!(hex.len(), 64);
    assert!(Token::from_hex(&hex).unwrap().matches(t.as_bytes()));
    let mut other = *t.as_bytes();
    other[31] ^= 1;
    assert!(!t.matches(&other));
    assert!(Token::from_hex("zz").is_none());
    assert!(Token::from_hex(&"g".repeat(64)).is_none());
    assert!(Token::from_hex(&"é".repeat(32)).is_none());
    assert_eq!(format!("{t:?}"), "Token(..)");
    assert!(Token::from_bytes([1; 32]).matches(&[1; 32]));
}

fn pair_with_pid() -> (Stream, Stream, Option<u32>) {
    let mut l = Listener::bind().unwrap();
    let name = l.name().to_string();
    let client = std::thread::spawn(move || Stream::connect(&name, GUARD).unwrap());
    let (server, pid) = l.accept(GUARD).unwrap();
    (server, client.join().unwrap(), pid)
}

#[test]
fn endpoints_are_random_private_and_carry_bytes_both_ways() {
    let a = Listener::bind().unwrap();
    let b = Listener::bind().unwrap();
    assert_ne!(a.name(), b.name());
    assert!(a.name().contains("paddock-"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::path::Path::new(a.name())
            .parent()
            .unwrap()
            .to_path_buf();
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        drop(a);
        assert!(!dir.exists(), "the private folder goes with the listener");
    }
    let (mut s, mut c, pid) = pair_with_pid();
    if cfg!(any(windows, target_os = "linux", target_os = "macos")) {
        assert_eq!(pid, Some(std::process::id()));
    }
    #[cfg(unix)]
    assert!(s.peer_uid().is_some());
    c.write_all(b"ping").unwrap();
    let mut buf = [0u8; 4];
    s.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"ping");
    s.write_all(b"pong").unwrap();
    s.flush().unwrap();
    c.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"pong");
}

#[test]
fn an_endpoint_goes_in_the_runtime_folder_when_the_path_fits() {
    let dir = tempfile::tempdir().unwrap();
    let l = Listener::bind_in(dir.path()).unwrap();
    #[cfg(unix)]
    assert!(
        l.name().starts_with(&*dir.path().to_string_lossy()) || dir.path().as_os_str().len() > 60,
        "{}",
        l.name()
    );
    let name = l.name().to_string();
    drop(l);
    assert!(Stream::connect(&name, Duration::from_millis(10)).is_err());
    // A folder too deep for a socket path falls back to the temporary folder.
    let deep = dir.path().join("d".repeat(120));
    assert!(Listener::bind_in(&deep).is_ok());
}

#[test]
fn reading_and_writing_on_two_threads_do_not_block_each_other() {
    let (s, mut c, _) = pair_with_pid();
    let mut reader = s.try_clone().unwrap();
    let mut writer = s;
    let r = std::thread::spawn(move || {
        let mut buf = [0u8; 5];
        reader.read_exact(&mut buf).map(|()| buf)
    });
    writer.write_all(b"hello").unwrap();
    let mut buf = [0u8; 5];
    c.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"hello");
    c.write_all(b"world").unwrap();
    assert_eq!(&r.join().unwrap().unwrap(), b"world");
}

#[test]
fn accept_and_reads_time_out_and_the_listener_still_works() {
    let mut l = Listener::bind().unwrap();
    assert_eq!(
        l.accept(Duration::from_millis(1)).err().map(|e| e.kind()),
        Some(io::ErrorKind::TimedOut)
    );
    let name = l.name().to_string();
    let client = std::thread::spawn(move || Stream::connect(&name, GUARD).unwrap());
    let (mut s, _) = l.accept(GUARD).unwrap();
    let _c = client.join().unwrap();
    s.set_read_timeout(Some(Duration::from_millis(1))).unwrap();
    let err = s.read(&mut [0u8; 1]).unwrap_err();
    assert!(
        matches!(
            err.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ),
        "{err:?}"
    );
    s.set_read_timeout(None).unwrap();
    assert!(
        Stream::connect(
            &format!("{}-gone", Listener::bind().unwrap().name()),
            Duration::from_millis(1)
        )
        .is_err()
    );
}

#[test]
fn a_closer_wakes_a_blocked_reader_and_the_other_side_sees_the_end() {
    let (s, mut c, _) = pair_with_pid();
    let closer = s.try_clone().unwrap().closer().unwrap();
    let mut reader = s;
    let r = std::thread::spawn(move || reader.read(&mut [0u8; 1]));
    closer.close();
    closer.close();
    assert_eq!(r.join().unwrap().unwrap(), 0);
    drop(closer);
    assert_eq!(c.read(&mut [0u8; 1]).unwrap_or(0), 0);
}

#[test]
fn a_fixed_endpoint_replaces_a_stale_socket_but_nothing_else() {
    let access = Access::owner_only();
    assert_ne!(Access::service_for_interactive_users().sddl, access.sddl);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sock").to_string_lossy().into_owned();
        let l = Listener::bind_fixed(&path, &access).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // A crashed run leaves the socket file: the next one replaces it.
        std::mem::forget(l);
        let l = Listener::bind_fixed(&path, &access).unwrap();
        drop(l);
        assert!(!std::path::Path::new(&path).exists());
        let file = dir.path().join("f").to_string_lossy().into_owned();
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            Listener::bind_fixed(&file, &access).err().map(|e| e.kind()),
            Some(io::ErrorKind::AlreadyExists)
        );
        assert!(Listener::bind_fixed(&"x".repeat(200), &access).is_err());
    }
    let (_, _) = endpoint::pair().unwrap();
}

fn host(protocol: ProtocolVersion) -> Host {
    Host::listen(FrameLimit::default(), protocol)
        .unwrap()
        .named("Debrief")
}

fn connect_from_thread(
    h: &Host,
    token: Token,
    protocol: ProtocolVersion,
) -> std::thread::JoinHandle<Result<paddock_ipc::Connection, IpcError>> {
    let name = h.endpoint().to_string();
    std::thread::spawn(move || Client::connect(&name, &token, FrameLimit::default(), protocol))
}

#[test]
fn the_handshake_authenticates_the_token_and_the_pid() {
    let mut h = host(ProtocolVersion::new(1, 2));
    assert_eq!(h.limit(), FrameLimit::default());
    assert_eq!(h.protocol(), ProtocolVersion::new(1, 2));
    let client = connect_from_thread(&h, h.token().clone(), ProtocolVersion::new(1, 1));
    let conn = h.accept(GUARD).unwrap();
    assert_eq!(conn.peer_pid(), Some(std::process::id()));
    assert_eq!(conn.protocol(), ProtocolVersion::new(1, 1));
    assert_eq!(conn.limit(), FrameLimit::default());
    #[cfg(unix)]
    assert!(conn.peer_uid().is_some());
    let theirs = client.join().unwrap().unwrap();
    assert_eq!(theirs.protocol(), ProtocolVersion::new(1, 2));
    assert_eq!(theirs.peer_pid(), None);

    // A wrong token is refused on both sides.
    let client = connect_from_thread(&h, Token::from_bytes([9; 32]), ProtocolVersion::new(1, 0));
    assert!(matches!(h.accept(GUARD), Err(IpcError::Auth(m)) if m.contains("token")));
    let got = client.join().unwrap();
    assert!(matches!(got, Err(IpcError::Auth(_))), "{got:?}");

    // Only the process this host started.
    let client = connect_from_thread(&h, h.token().clone(), ProtocolVersion::new(1, 0));
    assert!(matches!(
        h.accept_while(Some(std::process::id() + 1), GUARD, &mut || Ok(())),
        Err(IpcError::Auth(_))
    ));
    assert!(matches!(client.join().unwrap(), Err(IpcError::Auth(_))));

    // Nobody came, or the caller stops waiting.
    assert_eq!(h.accept(Duration::ZERO).err(), Some(IpcError::Timeout));
    assert_eq!(
        h.accept_while(None, GUARD, &mut || Err(IpcError::Spawn("gone".into())))
            .err(),
        Some(IpcError::Spawn("gone".into()))
    );
}

#[test]
fn a_client_of_another_protocol_is_told_which_side_to_update() {
    let mut h = host(ProtocolVersion::new(2, 3));
    let client = connect_from_thread(&h, h.token().clone(), ProtocolVersion::new(2, 4));
    let ours = h.accept(GUARD).unwrap_err();
    assert!(
        matches!(&ours, IpcError::Version(m) if m.contains("needs a newer Debrief")),
        "{ours:?}"
    );
    assert!(matches!(client.join().unwrap(), Err(IpcError::Version(m)) if m.contains("2.4")));
    let client = connect_from_thread(&h, h.token().clone(), ProtocolVersion::new(1, 9));
    assert!(matches!(h.accept(GUARD), Err(IpcError::Version(m)) if m.contains("too old")));
    assert!(client.join().unwrap().is_err());
}

#[test]
fn a_first_frame_that_is_not_a_hello_or_of_another_frame_version_is_refused() {
    let mut h = host(ProtocolVersion::UNVERSIONED);
    for bytes in [
        frame::encode(&Frame::Cancel { id: 1 }, FrameLimit::default()).unwrap(),
        {
            // A version-1 hello: refused by its frame version.
            let body = postcard::to_stdvec(&(0u32, 1u16, [1u8; 32], 7u32)).unwrap();
            let mut b = (body.len() as u32).to_le_bytes().to_vec();
            b.extend(body);
            b
        },
        // A hello with a valid start but a broken rest.
        vec![3, 0, 0, 0, 0, 2, 0],
    ] {
        let name = h.endpoint().to_string();
        let t = std::thread::spawn(move || {
            let mut s = Stream::connect(&name, GUARD).unwrap();
            s.write_all(&bytes).unwrap();
            let mut rest = Vec::new();
            let _ = s.read_to_end(&mut rest);
        });
        let e = h.accept(GUARD).unwrap_err();
        assert!(
            matches!(e, IpcError::Protocol(_) | IpcError::Version(_)),
            "{e:?}"
        );
        drop(t.join());
    }
    // A client that leaves before its hello.
    let name = h.endpoint().to_string();
    std::thread::spawn(move || drop(Stream::connect(&name, GUARD).unwrap()))
        .join()
        .unwrap();
    assert!(matches!(h.accept(GUARD), Err(IpcError::Closed(_))));
    // A host that answers garbage, or closes.
    let mut l = Listener::bind().unwrap();
    let name = l.name().to_string();
    let t = std::thread::spawn(move || {
        Client::connect(
            &name,
            &Token::from_bytes([1; 32]),
            FrameLimit::default(),
            ProtocolVersion::UNVERSIONED,
        )
    });
    let (mut s, _) = l.accept(GUARD).unwrap();
    let _hello = frame::read_frame(&mut s, FrameLimit::default()).unwrap();
    frame::write_frame(&mut s, &Frame::Cancel { id: 1 }, FrameLimit::default()).unwrap();
    assert!(matches!(t.join().unwrap(), Err(IpcError::Protocol(_))));
    let name = l.name().to_string();
    let t = std::thread::spawn(move || {
        Client::connect(
            &name,
            &Token::from_bytes([1; 32]),
            FrameLimit::default(),
            ProtocolVersion::UNVERSIONED,
        )
    });
    let (s, _) = l.accept(GUARD).unwrap();
    drop(s);
    assert!(t.join().unwrap().is_err());
    assert!(codes::TOO_LARGE != codes::BUSY);
}
