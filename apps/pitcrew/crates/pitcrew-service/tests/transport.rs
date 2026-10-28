//! The real transport end to end: the service host creates the named pipe (with its ACL) or the Unix socket, and a
//! client says hello, asks for the status, subscribes and gets a pushed status.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use pitcrew_core::protocol::{read_frame, write_frame, Request, Response, ServiceEvent, PROTOCOL_VERSION};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

trait Stream: Read + Write {}
impl<T: Read + Write> Stream for T {}

fn connect(_data: &std::path::Path) -> Box<dyn Stream> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        #[cfg(windows)]
        let attempt = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(pitcrew_service::ipc::pipe_name(_data))
            .map(|f| Box::new(f) as Box<dyn Stream>);
        #[cfg(unix)]
        let attempt =
            std::os::unix::net::UnixStream::connect(_data.join(pitcrew_service::ipc::SOCKET_FILE)).map(|s| Box::new(s) as Box<dyn Stream>);
        match attempt {
            Ok(s) => return s,
            Err(e) => {
                assert!(Instant::now() < deadline, "the service did not listen: {e}");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

#[test]
fn a_client_talks_to_the_running_service() {
    let data = std::env::temp_dir().join(format!("pitcrew-it-transport-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("pitcrew.toml"), "schema_version = 1\n").unwrap();
    let host = pitcrew_service::host::Host::start(data.clone(), |e| panic!("the IPC server failed: {e}")).unwrap();

    let mut c = connect(&data);
    write_frame(
        &mut c,
        &Request::Hello {
            protocol: PROTOCOL_VERSION,
        },
    )
    .unwrap();
    assert!(matches!(read_frame::<Response>(&mut c).unwrap(), Response::Hello { .. }));
    write_frame(&mut c, &Request::Status).unwrap();
    let Response::Status { runners, service } = read_frame::<Response>(&mut c).unwrap() else {
        panic!()
    };
    assert!(runners.is_empty());
    assert_eq!(service.data_dir, data.display().to_string());

    let mut sub = connect(&data);
    write_frame(
        &mut sub,
        &Request::Hello {
            protocol: PROTOCOL_VERSION,
        },
    )
    .unwrap();
    let _ = read_frame::<Response>(&mut sub).unwrap();
    write_frame(&mut sub, &Request::Subscribe).unwrap();
    assert_eq!(read_frame::<Response>(&mut sub).unwrap(), Response::Ok);
    assert!(matches!(
        read_frame::<Response>(&mut sub).unwrap(),
        Response::Event {
            event: ServiceEvent::Status { .. }
        }
    ));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(data.join(pitcrew_service::ipc::SOCKET_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "only the agent's user may connect");
    }
    drop((c, sub));
    host.stop();
    let _ = std::fs::remove_dir_all(&data);
}
