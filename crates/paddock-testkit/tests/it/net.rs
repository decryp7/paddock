use paddock_testkit::net::HeldPort;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// Writes a request and reads until the stream ends or fails.
fn exchange(port: &HeldPort) -> std::io::Result<Vec<u8>> {
    talk(TcpStream::connect(port.addr())?)
}

/// Writes a request on `s` and reads until the stream ends or fails; the first failure, the write's or the read's, is the
/// result. A socket reports a reset once: when it reaches the write first (the peer's reset raced the request), the read
/// after it only sees the end of the stream, so a write failure must not be thrown away.
fn talk(mut s: TcpStream) -> std::io::Result<Vec<u8>> {
    // A hang guard only: the held port answers at once. Setting it fails (EINVAL on macOS) on a socket that was already
    // reset, which a read cannot hang on, so that failure is not the result: the write or the read reports the reset.
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")?;
    let mut got = Vec::new();
    s.read_to_end(&mut got)?;
    Ok(got)
}

/// The kinds a client sees for a connection the peer reset or closed under it.
fn is_reset(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::BrokenPipe
    )
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
            Err(e) => assert!(is_reset(&e), "{e:?}"),
        }
    }
}

#[test]
fn a_reset_port_resets_every_connection() {
    let held = HeldPort::reset();
    for _ in 0..3 {
        let e = exchange(&held).expect_err("a reset, not an answer");
        assert!(is_reset(&e), "{e:?}");
    }
}

/// Regression: the request's write failing was ignored, so when the reset reached the write before the read (CI, Linux,
/// under load) the read saw only the end of the stream and the reset port seemed to answer with nothing. Here the reset is
/// made to reach a write first, every time: writing until a write fails waits for it without a clock.
#[test]
fn a_reset_that_reaches_the_write_first_is_still_a_reset() {
    let held = HeldPort::reset();
    // On macOS the reset can reach `connect` itself; that is a reset too, but the write must see it here, so connect again.
    let mut s = (0..1000)
        .find_map(|_| match TcpStream::connect(held.addr()) {
            Ok(s) => Some(s),
            Err(e) => {
                assert!(is_reset(&e), "{e:?}");
                None
            }
        })
        .expect("a connection that the reset has not reached yet");
    let first = loop {
        if let Err(e) = s.write_all(b"x") {
            break e;
        }
    };
    assert!(is_reset(&first), "{first:?}");
    let e = talk(s).expect_err("a reset, not an answer");
    assert!(is_reset(&e), "{e:?}");
}
