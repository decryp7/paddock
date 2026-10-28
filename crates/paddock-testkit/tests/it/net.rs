use paddock_testkit::net::HeldPort;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// Writes a request and reads until the stream ends or fails.
fn exchange(port: &HeldPort) -> std::io::Result<Vec<u8>> {
    let mut s = TcpStream::connect(port.addr())?;
    // A hang guard only: the held port answers at once.
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    let _ = s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n");
    let mut got = Vec::new();
    s.read_to_end(&mut got)?;
    Ok(got)
}

#[test]
fn no_other_server_can_be_given_a_held_port() {
    for held in [HeldPort::hang_up(), HeldPort::reset()] {
        assert!(TcpListener::bind(held.addr()).is_err(), "the port is taken");
        assert_eq!(held.addr().port(), held.port());
        assert!(held.addr().ip().is_loopback());
        assert_eq!(held.url(), format!("http://127.0.0.1:{}", held.port()));
    }
}

#[test]
fn a_hang_up_port_answers_nothing() {
    let held = HeldPort::hang_up();
    for _ in 0..3 {
        // The end of the stream, or a reset when the close raced the unread request: never an answer.
        match exchange(&held) {
            Ok(got) => assert!(got.is_empty()),
            Err(e) => assert!(
                matches!(
                    e.kind(),
                    ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
                ),
                "{e:?}"
            ),
        }
    }
}

#[test]
fn a_reset_port_resets_every_connection() {
    let held = HeldPort::reset();
    for _ in 0..3 {
        let e = exchange(&held).expect_err("a reset, not an answer");
        assert!(
            matches!(
                e.kind(),
                ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::BrokenPipe
            ),
            "{e:?}"
        );
    }
}
