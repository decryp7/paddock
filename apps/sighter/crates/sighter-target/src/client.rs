//! The VNC adapter of the core's [`Target`] port: one TCP connection, a reader thread that keeps the latest frame, and a
//! writer for input shared under a mutex. Timeouts are options, never hard-coded at a call site.

use crate::auth::vnc_auth_response;
use crate::canvas::Canvas;
use crate::dial::Dial;
use crate::error::RfbError;
use crate::input::Input;
use crate::proto::{self, Decoder, ServerEvent, Version};
use sighter_core::frame::Frame;
use sighter_core::geometry::{Logical, Pt};
use sighter_core::ports::{FrameSource, InputDriver, StopFlag, Target, TargetError};
use std::io::{BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Where to connect: parsed from `vnc://host:port` (the port defaults to 5900).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VncAddr {
    /// Host name or address.
    pub host: String,
    /// TCP port.
    pub port: u16,
}

impl VncAddr {
    /// Parses `vnc://host`, `vnc://host:5901` or `vnc://[::1]:5902`.
    pub fn parse(s: &str) -> Result<VncAddr, RfbError> {
        // The input is never echoed: an address typed with a login ("vnc://user:secret@host") must not reach a log.
        let bad = |why: &str| RfbError::Protocol(format!("target must look like vnc://host:5900 ({why})"));
        if s.contains('@') {
            return Err(bad("a login does not belong in the address; enter the password separately"));
        }
        let rest = s.strip_prefix("vnc://").ok_or_else(|| bad("wrong scheme"))?.trim_end_matches('/');
        let port_of = |p: &str| p.parse().map_err(|_| bad("bad port"));
        let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
            let (host, tail) = v6.split_once(']').ok_or_else(|| bad("unclosed ["))?;
            (
                host,
                if tail.is_empty() {
                    5900
                } else {
                    port_of(tail.strip_prefix(':').ok_or_else(|| bad("bad port"))?)?
                },
            )
        } else {
            match rest.rsplit_once(':') {
                Some((h, p)) => (h, port_of(p)?),
                None => (rest, 5900),
            }
        };
        if port == 0 {
            return Err(bad("port 0 is not connectable"));
        }
        if host.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(bad("the host has spaces or control characters"));
        }
        if host.is_empty() {
            return Err(bad("no host"));
        }
        Ok(VncAddr {
            host: host.to_string(),
            port,
        })
    }
}

/// Timeouts and pacing of a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectOptions {
    /// TCP connect.
    pub connect_timeout: Duration,
    /// Each read during the handshake.
    pub handshake_timeout: Duration,
    /// The whole handshake. // cap: a peer is untrusted and could answer one byte per `handshake_timeout` for ever; a real
    /// handshake is a few hundred bytes and takes milliseconds, so there is no progress to judge it by.
    pub handshake_total: Duration,
    /// Each write.
    pub write_timeout: Duration,
    /// How long `grab` waits for the first frame.
    pub frame_timeout: Duration,
    /// Pause between incremental update requests, so an idle desktop does not spin.
    pub update_interval: Duration,
    /// How often a waiting `grab` looks at the stop flag, so a stop ends a long wait within this time.
    pub stop_poll: Duration,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        ConnectOptions {
            connect_timeout: Duration::from_secs(10),
            handshake_timeout: Duration::from_secs(15),
            handshake_total: Duration::from_secs(60),
            write_timeout: Duration::from_secs(3),
            frame_timeout: Duration::from_secs(10),
            update_interval: Duration::from_millis(15),
            stop_poll: Duration::from_millis(25),
        }
    }
}

struct FrameState {
    seq: u64,
    frame: Option<Frame>,
    error: Option<TargetError>,
}

struct Shared {
    state: Mutex<FrameState>,
    cv: Condvar,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, FrameState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A writer shared by the input session and the reader thread (update requests).
#[derive(Clone)]
struct SharedWriter(Arc<Mutex<TcpStream>>);

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).flush()
    }
}

/// A connected VNC console.
pub struct VncTarget {
    name: String,
    desktop: String,
    shared: Arc<Shared>,
    input: Input<SharedWriter>,
    socket: TcpStream,
    reader: Option<JoinHandle<()>>,
    reader_probe: Weak<()>,
    stop: StopFlag,
    opts: ConnectOptions,
}

impl VncTarget {
    /// Connects, authenticates and starts receiving frames. `password` is used only for the DES challenge and is never
    /// logged or stored.
    pub fn connect(
        addr: &VncAddr,
        password: Option<&str>,
        stop: StopFlag,
        opts: ConnectOptions,
        dial: &Dial,
    ) -> Result<VncTarget, TargetError> {
        let addrs = dial.resolve(&addr.host, addr.port)?;
        let stream = connect_any(&addrs, opts.connect_timeout, &stop, TcpStream::connect_timeout).map_err(|e| {
            e.with_hint(&format!(
                "connecting to {}:{} (is the VM running with VNC enabled on that port?)",
                addr.host, addr.port
            ))
        })?;
        let init = Self::handshake_stream(&stream, password, &opts).map_err(|e| e.into_target(opts.handshake_timeout))?;
        Self::start(addr, stream, init, stop, opts).map_err(|e| e.into_target(opts.write_timeout))
    }

    fn handshake_stream(
        stream: &TcpStream,
        password: Option<&str>,
        opts: &ConnectOptions,
    ) -> Result<(proto::ServerInit, BufReader<TcpStream>), RfbError> {
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(opts.handshake_timeout))?;
        stream.set_write_timeout(Some(opts.write_timeout))?;
        let mut r = BufReader::new(stream.try_clone()?);
        let mut w = stream.try_clone()?;
        let mut limited = DeadlineRead {
            inner: &mut r,
            deadline: Instant::now() + opts.handshake_total,
        };
        let init = handshake(&mut limited, &mut w, password)?;
        proto::send(&mut w, &proto::set_pixel_format())?;
        proto::send(&mut w, &proto::set_encodings(&proto::PREFERRED_ENCODINGS))?;
        proto::send(&mut w, &proto::update_request(false, init.width, init.height))?;
        stream.set_read_timeout(None)?;
        Ok((init, r))
    }

    fn start(
        addr: &VncAddr,
        stream: TcpStream,
        (init, reader): (proto::ServerInit, BufReader<TcpStream>),
        stop: StopFlag,
        opts: ConnectOptions,
    ) -> Result<VncTarget, RfbError> {
        let fb = Canvas::new(u32::from(init.width), u32::from(init.height))?;
        let shared = Arc::new(Shared {
            state: Mutex::new(FrameState {
                seq: 0,
                frame: None,
                error: None,
            }),
            cv: Condvar::new(),
        });
        let writer = SharedWriter(Arc::new(Mutex::new(stream.try_clone()?)));
        let (sh, wr) = (shared.clone(), writer.clone());
        let alive = Arc::new(());
        let reader_probe = Arc::downgrade(&alive);
        let reader_thread = std::thread::Builder::new()
            .name("vnc-reader".into())
            .spawn(move || {
                let _alive = alive;
                let err = reader_loop(reader, &sh, wr, fb, opts.update_interval);
                sh.lock().error = Some(err.into_target(Duration::ZERO));
                sh.cv.notify_all();
            })
            .map_err(|e| RfbError::Io(format!("starting the VNC reader thread: {e}")))?;
        Ok(VncTarget {
            reader: Some(reader_thread),
            reader_probe,
            stop: stop.clone(),
            name: format!("vnc://{}:{}", addr.host, addr.port),
            desktop: init.name,
            shared,
            input: Input::new(writer, stop),
            socket: stream,
            opts,
        })
    }

    /// A probe on the reader thread: it upgrades while the thread runs and stops upgrading once it has ended (after drop).
    pub fn reader_probe(&self) -> Weak<()> {
        self.reader_probe.clone()
    }

    /// The desktop name the server reported (untrusted text).
    pub fn desktop_name(&self) -> &str {
        &self.desktop
    }

    /// The latest frame and its sequence number; waits for the first frame up to `frame_timeout`. A stop request ends the
    /// wait within `stop_poll`.
    pub fn latest(&self) -> Result<(u64, Frame), TargetError> {
        let ms = self.opts.frame_timeout;
        let deadline = Instant::now() + self.opts.frame_timeout;
        let mut st = self.shared.lock();
        while st.frame.is_none() && st.error.is_none() {
            self.check_stop()?;
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(TargetError::Timeout(ms));
            }
            st = self.wait(st, left.min(self.opts.stop_poll));
        }
        if let Some(e) = &st.error {
            return Err(e.clone());
        }
        st.frame.as_ref().map(|f| (st.seq, f.clone())).ok_or(TargetError::Timeout(ms))
    }

    /// Waits until a frame newer than `seq` arrives; returns the new sequence number, or the unchanged one on timeout. A
    /// stop request ends the wait within `stop_poll`.
    pub fn wait_newer(&self, seq: u64, timeout: Duration) -> Result<u64, TargetError> {
        let deadline = Instant::now() + timeout;
        let mut st = self.shared.lock();
        while st.seq <= seq && st.error.is_none() {
            self.check_stop()?;
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            st = self.wait(st, left.min(self.opts.stop_poll));
        }
        match &st.error {
            Some(e) => Err(e.clone()),
            None => Ok(st.seq),
        }
    }

    /// `Stopped` once the stop flag is set; held input is released by the next input call or by drop.
    fn check_stop(&self) -> Result<(), TargetError> {
        if self.stop.is_stopped() {
            Err(TargetError::Stopped)
        } else {
            Ok(())
        }
    }

    fn wait<'a>(&'a self, st: std::sync::MutexGuard<'a, FrameState>, slice: Duration) -> std::sync::MutexGuard<'a, FrameState> {
        self.shared.cv.wait_timeout(st, slice).unwrap_or_else(|e| e.into_inner()).0
    }

    /// Releases every held key and button now (used when a stop is requested while the driver is idle).
    pub fn release_input(&mut self) -> Result<(), TargetError> {
        self.input.release_all()
    }
}

/// Tries each checked address in turn; the first that connects wins, otherwise the last failure is reported. `connector` is
/// `TcpStream::connect_timeout` in production and a fake in the tests.
fn connect_any(
    addrs: &[SocketAddr],
    timeout: Duration,
    stop: &StopFlag,
    mut connector: impl FnMut(&SocketAddr, Duration) -> std::io::Result<TcpStream>,
) -> Result<TcpStream, TargetError> {
    let mut last = TargetError::Lost("the name has no address".into());
    for a in addrs {
        if stop.is_stopped() {
            return Err(TargetError::Stopped);
        }
        match connector(a, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => last = RfbError::from(e).into_target(timeout),
        }
    }
    Err(last)
}

/// A reader that fails with `TimedOut` once the handshake's overall deadline has passed.
struct DeadlineRead<'a, R> {
    inner: &'a mut R,
    deadline: Instant,
}

impl<R: Read> Read for DeadlineRead<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if Instant::now() >= self.deadline {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.inner.read(buf)
    }
}

trait WithHint {
    fn with_hint(self, hint: &str) -> TargetError;
}

impl WithHint for TargetError {
    fn with_hint(self, hint: &str) -> TargetError {
        match self {
            TargetError::Lost(m) => TargetError::Lost(format!("{hint}: {m}")),
            other => other,
        }
    }
}

impl Drop for VncTarget {
    fn drop(&mut self) {
        let _ = self.input.release_all();
        let _ = self.socket.shutdown(Shutdown::Both);
        // The shutdown ends the reader's blocking read; join it so no thread outlives the target.
        if let Some(t) = self.reader.take() {
            let _ = t.join();
        }
    }
}

/// The RFB handshake up to and including ServerInit.
pub fn handshake(r: &mut impl Read, w: &mut impl Write, password: Option<&str>) -> Result<proto::ServerInit, RfbError> {
    let mut ver = [0u8; 12];
    r.read_exact(&mut ver)?;
    let v = Version::negotiate(&ver)?;
    proto::send(w, v.client_string())?;
    let sec = if v == Version::V3_3 {
        match proto::read_u32(r)? {
            0 => return Err(RfbError::Refused(proto::read_string(r)?)),
            t => u8::try_from(t).unwrap_or(0),
        }
    } else {
        let n = proto::read_u8(r)?;
        if n == 0 {
            return Err(RfbError::Refused(proto::read_string(r)?));
        }
        let mut types = vec![0; n as usize];
        r.read_exact(&mut types)?;
        let pick = if password.is_some() && types.contains(&2) {
            2
        } else if types.contains(&1) {
            1
        } else if types.contains(&2) {
            return Err(RfbError::PasswordRequired);
        } else {
            return Err(RfbError::UnsupportedSecurity(types));
        };
        proto::send(w, &[pick])?;
        pick
    };
    match sec {
        1 => {}
        2 => {
            let mut challenge = [0u8; 16];
            r.read_exact(&mut challenge)?;
            let pw = password.ok_or(RfbError::PasswordRequired)?;
            proto::send(w, &vnc_auth_response(pw.as_bytes(), &challenge))?;
        }
        t => return Err(RfbError::UnsupportedSecurity(vec![t])),
    }
    // Version 3.3 and 3.7 send a SecurityResult only for VNC authentication; 3.8 always.
    if (sec == 2 || v == Version::V3_8) && proto::read_u32(r)? != 0 {
        let why = if v == Version::V3_8 {
            proto::read_string(r).unwrap_or_default()
        } else {
            String::new()
        };
        return Err(RfbError::AuthFailed(why));
    }
    proto::send(w, &[1])?; // shared: keep other viewers (and the Workstation console) connected
    proto::read_server_init(r)
}

fn reader_loop(r: impl Read, sh: &Shared, w: SharedWriter, fb: Canvas, interval: Duration) -> RfbError {
    match pump(r, sh, w, fb, interval) {
        Err(e) => e,
        Ok(never) => match never {},
    }
}

/// Reads server messages until one fails; every update publishes a new frame snapshot and asks for the next one.
fn pump(
    mut r: impl Read,
    sh: &Shared,
    mut w: SharedWriter,
    mut fb: Canvas,
    interval: Duration,
) -> Result<std::convert::Infallible, RfbError> {
    let mut dec = Decoder::default();
    loop {
        if let ServerEvent::Updated(_) | ServerEvent::Resized(..) = dec.read_message(&mut r, &mut fb)? {
            let snapshot = fb.snapshot()?;
            {
                let mut st = sh.lock();
                st.seq += 1;
                st.frame = Some(snapshot);
            }
            sh.cv.notify_all();
            std::thread::sleep(interval);
            let width = u16::try_from(fb.width()).unwrap_or(u16::MAX);
            let height = u16::try_from(fb.height()).unwrap_or(u16::MAX);
            proto::send(&mut w, &proto::update_request(true, width, height))?;
        }
    }
}

impl FrameSource for VncTarget {
    fn grab(&mut self) -> Result<Frame, TargetError> {
        self.latest().map(|(_, f)| f)
    }
}

impl InputDriver for VncTarget {
    fn move_to(&mut self, at: Pt<Logical>) -> Result<(), TargetError> {
        self.input.move_to(at)
    }
    fn click(&mut self, at: Pt<Logical>, button: sighter_core::ports::Button) -> Result<(), TargetError> {
        self.input.click(at, button)
    }
    fn chord(&mut self, keys: &[sighter_core::ports::Key]) -> Result<(), TargetError> {
        self.input.chord(keys)
    }
    fn type_text(&mut self, text: &str) -> Result<(), TargetError> {
        self.input.type_text(text)
    }
}

impl Target for VncTarget {
    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn init() -> Vec<u8> {
        let mut v = vec![0, 4, 0, 3];
        v.extend_from_slice(&proto::PIXEL_FORMAT);
        v.extend_from_slice(&3u32.to_be_bytes());
        v.extend_from_slice(b"vm1");
        v
    }

    fn string(s: &str) -> Vec<u8> {
        [&(s.len() as u32).to_be_bytes()[..], s.as_bytes()].concat()
    }

    fn run(server: Vec<u8>, password: Option<&str>) -> (Result<proto::ServerInit, RfbError>, Vec<u8>) {
        let mut sent = Vec::new();
        let r = handshake(&mut Cursor::new(server), &mut sent, password);
        (r, sent)
    }

    #[test]
    fn rfb_3_8_without_authentication() {
        let server = [b"RFB 003.008\n".to_vec(), vec![1, 1], 0u32.to_be_bytes().to_vec(), init()].concat();
        let (r, sent) = run(server, None);
        assert_eq!(r.unwrap().name, "vm1");
        assert_eq!(sent, [b"RFB 003.008\n".to_vec(), vec![1], vec![1]].concat());
    }

    #[test]
    fn rfb_3_8_with_a_password_sends_the_des_response() {
        let challenge: [u8; 16] = core::array::from_fn(|i| i as u8);
        let server = [
            b"RFB 003.008\n".to_vec(),
            vec![2, 1, 2],
            challenge.to_vec(),
            0u32.to_be_bytes().to_vec(),
            init(),
        ]
        .concat();
        let (r, sent) = run(server, Some("password"));
        assert!(r.is_ok());
        let want = [
            b"RFB 003.008\n".to_vec(),
            vec![2],
            vnc_auth_response(b"password", &challenge).to_vec(),
            vec![1],
        ]
        .concat();
        assert_eq!(sent, want);
    }

    #[test]
    fn a_password_given_but_only_none_offered_uses_none() {
        let server = [b"RFB 003.007\n".to_vec(), vec![1, 1], init()].concat();
        let (r, sent) = run(server, Some("x"));
        assert!(r.is_ok());
        assert_eq!(sent, [b"RFB 003.007\n".to_vec(), vec![1], vec![1]].concat());
    }

    #[test]
    fn handshake_failures_are_typed() {
        let v38 = b"RFB 003.008\n".to_vec();
        let cases: Vec<(Vec<u8>, Option<&str>, RfbError)> = vec![
            (
                [v38.clone(), vec![0], string("too many sessions")].concat(),
                None,
                RfbError::Refused("too many sessions".into()),
            ),
            ([v38.clone(), vec![1, 2]].concat(), None, RfbError::PasswordRequired),
            ([v38.clone(), vec![1, 5]].concat(), None, RfbError::UnsupportedSecurity(vec![5])),
            ([v38.clone(), vec![1, 2], vec![0; 16]].concat(), None, RfbError::PasswordRequired),
            (
                [v38.clone(), vec![1, 2], vec![0; 16], 1u32.to_be_bytes().to_vec(), string("bad")].concat(),
                Some("pw"),
                RfbError::AuthFailed("bad".into()),
            ),
            (
                [b"RFB 003.003\n".to_vec(), 0u32.to_be_bytes().to_vec(), string("no")].concat(),
                None,
                RfbError::Refused("no".into()),
            ),
            (
                [b"RFB 003.003\n".to_vec(), 9u32.to_be_bytes().to_vec()].concat(),
                None,
                RfbError::UnsupportedSecurity(vec![9]),
            ),
            (
                [
                    b"RFB 003.003\n".to_vec(),
                    2u32.to_be_bytes().to_vec(),
                    vec![0; 16],
                    1u32.to_be_bytes().to_vec(),
                ]
                .concat(),
                Some("pw"),
                RfbError::AuthFailed(String::new()),
            ),
        ];
        for (server, pw, want) in cases {
            assert_eq!(run(server, pw).0.unwrap_err(), want);
        }
        assert!(matches!(run(b"HTTP/1.1 200".to_vec(), None).0, Err(RfbError::NotVnc(_))));
        assert!(
            matches!(run(b"RFB 003.008\n".to_vec(), None).0, Err(RfbError::Io(_))),
            "closed after the version"
        );
    }

    #[test]
    fn rfb_3_3_with_none_and_with_a_password() {
        let none = [b"RFB 003.003\n".to_vec(), 1u32.to_be_bytes().to_vec(), init()].concat();
        assert!(run(none, None).0.is_ok());
        let auth = [
            b"RFB 003.003\n".to_vec(),
            2u32.to_be_bytes().to_vec(),
            vec![0; 16],
            0u32.to_be_bytes().to_vec(),
            init(),
        ]
        .concat();
        let (r, sent) = run(auth, Some("pw"));
        assert!(r.is_ok());
        assert_eq!(sent.len(), 12 + 16 + 1);
    }

    #[test]
    fn addresses_parse() {
        assert_eq!(
            VncAddr::parse("vnc://127.0.0.1:5901").unwrap(),
            VncAddr {
                host: "127.0.0.1".into(),
                port: 5901
            }
        );
        assert_eq!(VncAddr::parse("vnc://vmhost/").unwrap().port, 5900);
        assert_eq!(
            VncAddr::parse("vnc://[::1]:5902").unwrap(),
            VncAddr {
                host: "::1".into(),
                port: 5902
            }
        );
        assert_eq!(VncAddr::parse("vnc://[::1]").unwrap().port, 5900);
        for bad in [
            "rdp://x",
            "vnc://:5900",
            "vnc://h:notaport",
            "vnc://",
            "",
            "vnc://[::1",
            "vnc://[::1]x",
            "vnc://[::1]:p",
            "vnc://host:0",
            "vnc://[::1]:0",
            "vnc://ho st:5900",
            "vnc://ho\u{1b}st",
        ] {
            assert!(matches!(VncAddr::parse(bad), Err(RfbError::Protocol(_))), "{bad}");
        }
    }

    #[test]
    fn a_login_in_the_address_is_refused_and_never_echoed() {
        for bad in [
            "vnc://user:hunter2@host",
            "vnc://hunter2@host:5900",
            "vnc://host:5900/hunter2@x",
            "rdp://hunter2",
        ] {
            let e = VncAddr::parse(bad).unwrap_err();
            let shown = format!("{e} {e:?}");
            assert!(!shown.contains("hunter2"), "{bad}: {shown}");
        }
        assert!(
            VncAddr::parse("vnc://user:pw@host")
                .unwrap_err()
                .to_string()
                .contains("password separately")
        );
    }

    #[test]
    fn the_handshake_deadline_ends_a_dripping_peer() {
        let mut inner = Cursor::new(vec![1u8, 2, 3]);
        let mut open = DeadlineRead {
            inner: &mut inner,
            deadline: Instant::now() + Duration::from_secs(600),
        };
        let mut b = [0u8; 2];
        assert_eq!(open.read(&mut b).unwrap(), 2);
        let mut expired = DeadlineRead {
            inner: &mut inner,
            deadline: Instant::now(),
        };
        assert_eq!(expired.read(&mut b).unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(RfbError::from(expired.read(&mut b).unwrap_err()), RfbError::Timeout);
    }

    #[test]
    fn connect_any_tries_every_address_and_reports_the_last_failure() {
        // The test holds this listener; the fake connector never touches the network for the refused address.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let good = listener.local_addr().unwrap();
        let refused: SocketAddr = "192.0.2.1:5900".parse().unwrap();
        let t = Duration::from_secs(20);
        let stop = StopFlag::default();
        let mut tried = Vec::new();
        let connector = |tried: &mut Vec<SocketAddr>, a: &SocketAddr, _: Duration| {
            tried.push(*a);
            if *a == refused {
                Err(std::io::Error::from(std::io::ErrorKind::ConnectionRefused))
            } else {
                TcpStream::connect(a)
            }
        };
        assert!(connect_any(&[refused, good], t, &stop, |a, d| connector(&mut tried, a, d)).is_ok());
        assert_eq!(tried, vec![refused, good], "the second address is tried after the first is refused");
        let err = connect_any(&[refused], t, &stop, |a, d| connector(&mut Vec::new(), a, d)).err();
        assert!(matches!(err, Some(TargetError::Lost(_))), "{err:?}");
        let timed_out = connect_any(&[refused], t, &stop, |_, _| Err(std::io::ErrorKind::TimedOut.into())).err();
        assert_eq!(timed_out, Some(TargetError::Timeout(t)));
        let none = connect_any(&[], t, &stop, |a, d| connector(&mut Vec::new(), a, d)).err();
        assert!(matches!(none, Some(TargetError::Lost(m)) if m.contains("no address")));
        stop.stop();
        let stopped = connect_any(&[good], t, &stop, |a, d| connector(&mut Vec::new(), a, d)).err();
        assert_eq!(stopped, Some(TargetError::Stopped));
    }

    #[test]
    fn the_shared_writer_writes_and_flushes() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut peer, _) = listener.accept().unwrap();
        let mut w = SharedWriter(Arc::new(Mutex::new(stream)));
        w.write_all(b"hi").unwrap();
        w.flush().unwrap();
        let mut got = [0u8; 2];
        peer.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"hi");
    }

    #[test]
    fn the_defaults_are_pinned_in_one_place() {
        let o = ConnectOptions::default();
        assert_eq!(
            (
                o.connect_timeout.as_secs(),
                o.handshake_timeout.as_secs(),
                o.write_timeout.as_secs()
            ),
            (10, 15, 3)
        );
        assert_eq!(
            (o.frame_timeout.as_secs(), o.update_interval.as_millis(), o.stop_poll.as_millis()),
            (10, 15, 25)
        );
    }

    #[test]
    fn hints_only_decorate_a_lost_target() {
        assert_eq!(
            TargetError::Timeout(Duration::from_millis(5)).with_hint("x"),
            TargetError::Timeout(Duration::from_millis(5))
        );
        assert!(matches!(TargetError::Lost("m".into()).with_hint("h"), TargetError::Lost(m) if m == "h: m"));
    }
}
