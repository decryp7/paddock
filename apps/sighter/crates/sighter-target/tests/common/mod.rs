#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The in-process fake VNC server for the integration tests. It binds to loopback on an ephemeral port only, serves one
//! connection and never touches a real VNC server or the user's machine. Tests drive it through channels and wait on
//! conditions (an event received, a frame newer than a sequence number), never on sleeps; dropping it joins every thread.

use flate2::{Compress, Compression, FlushCompress};
use paddock_jobs::time::FakeClock;
use paddock_net::{Egress, FakeWire, Policy, Reach, StaticLookup};
use sighter_core::ports::StopFlag;
use sighter_target::auth::vnc_auth_response;
use sighter_target::proto::PIXEL_FORMAT;
use sighter_target::{ConnectOptions, Dial, Scope, VncAddr, VncTarget};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

/// A generous upper bound for a wait on a condition; reaching it means a test bug, not a slow machine.
pub const WAIT: Duration = Duration::from_secs(20);
pub const CHALLENGE: [u8; 16] = [7; 16];
pub const W: u16 = 64;
pub const H: u16 = 48;

#[derive(Clone)]
pub enum Mode {
    /// RFB 3.8, security None.
    NoAuth,
    /// RFB 3.8, VNC password authentication.
    Password(&'static str),
    /// RFB 3.8 offering only a password type while the client has none, or only an unsupported type.
    OnlyTypes(Vec<u8>),
    /// Accepts, then says nothing.
    Silent,
    /// Completes the handshake, never sends a frame.
    NoFrames,
    /// Sends exactly these bytes, then closes.
    Bytes(Vec<u8>),
}

pub struct Fake {
    pub port: u16,
    pub to_client: Sender<Vec<u8>>,
    pub events: Receiver<Vec<u8>>,
    accepted: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.to_client.send(Vec::new());
        if !self.accepted.load(Ordering::SeqCst) {
            // Nobody connected: connect to ourselves so the blocking accept returns.
            let _ = TcpStream::connect(("127.0.0.1", self.port));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn read_exact(s: &mut TcpStream, n: usize) -> Option<Vec<u8>> {
    let mut b = vec![0; n];
    s.read_exact(&mut b).ok().map(|_| b)
}

/// Splits the client's messages and forwards each whole one to `tx`; returns when the socket closes.
fn forward_client_messages(mut s: TcpStream, tx: Sender<Vec<u8>>) {
    loop {
        let Some(t) = read_exact(&mut s, 1) else { return };
        let rest = match t[0] {
            0 => 19,
            2 => {
                let Some(h) = read_exact(&mut s, 3) else { return };
                let n = u16::from_be_bytes([h[1], h[2]]) as usize * 4;
                let Some(body) = read_exact(&mut s, n) else { return };
                let _ = tx.send([t, h, body].concat());
                continue;
            }
            3 => 9,
            4 => 7,
            5 => 5,
            _ => return,
        };
        let Some(body) = read_exact(&mut s, rest) else { return };
        let _ = tx.send([t, body].concat());
    }
}

pub fn start(mode: Mode) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (to_client, cmds) = channel::<Vec<u8>>();
    let (tx, events) = channel();
    let accepted = Arc::new(AtomicBool::new(false));
    let flag = accepted.clone();
    let thread = std::thread::spawn(move || {
        let Ok((mut s, _)) = listener.accept() else { return };
        flag.store(true, Ordering::SeqCst);
        match &mode {
            Mode::Silent => {
                let _ = cmds.recv();
                return;
            }
            Mode::Bytes(b) => {
                let _ = s.write_all(b);
                // Half-close, then drain until the client hangs up: closing both ways at once would race the client's own
                // writes (a broken pipe instead of the end-of-data it should see).
                let _ = s.shutdown(Shutdown::Write);
                let _ = std::io::copy(&mut s, &mut std::io::sink());
                return;
            }
            _ => {}
        }
        s.write_all(b"RFB 003.008\n").unwrap();
        read_exact(&mut s, 12).unwrap();
        match &mode {
            Mode::Password(pw) => {
                s.write_all(&[1, 2]).unwrap();
                read_exact(&mut s, 1).unwrap();
                s.write_all(&CHALLENGE).unwrap();
                let got = read_exact(&mut s, 16).unwrap();
                if got == vnc_auth_response(pw.as_bytes(), &CHALLENGE) {
                    s.write_all(&0u32.to_be_bytes()).unwrap();
                } else {
                    let why = b"wrong password";
                    s.write_all(&[&1u32.to_be_bytes()[..], &(why.len() as u32).to_be_bytes(), why].concat())
                        .unwrap();
                    return;
                }
            }
            Mode::OnlyTypes(types) => {
                let _ = s.write_all(&[&[types.len() as u8][..], types].concat());
                // The client closes without answering; wait for that, then end.
                let _ = read_exact(&mut s, 1);
                return;
            }
            _ => {
                s.write_all(&[1, 1]).unwrap();
                read_exact(&mut s, 1).unwrap();
                s.write_all(&0u32.to_be_bytes()).unwrap();
            }
        }
        read_exact(&mut s, 1).unwrap(); // the shared flag
        let name = b"fake-vm";
        let init = [
            &W.to_be_bytes()[..],
            &H.to_be_bytes(),
            &PIXEL_FORMAT,
            &(name.len() as u32).to_be_bytes(),
            name,
        ]
        .concat();
        s.write_all(&init).unwrap();
        let reader = s.try_clone().unwrap();
        let forwarder = std::thread::spawn(move || forward_client_messages(reader, tx));
        for bytes in cmds {
            if bytes.is_empty() || s.write_all(&bytes).is_err() {
                break;
            }
        }
        let _ = s.shutdown(Shutdown::Both);
        let _ = forwarder.join();
    });
    Fake {
        port,
        to_client,
        events,
        accepted,
        thread: Some(thread),
    }
}

/// A one-rectangle update header.
pub fn rect(x: u16, y: u16, w: u16, h: u16, enc: i32) -> Vec<u8> {
    let mut v = vec![0, 0, 0, 1];
    for n in [x, y, w, h] {
        v.extend(n.to_be_bytes());
    }
    v.extend(enc.to_be_bytes());
    v
}

/// A Raw update of the whole screen in one colour (B, G, R).
pub fn raw_screen(bgr: [u8; 3]) -> Vec<u8> {
    let mut m = rect(0, 0, W, H, sighter_target::proto::ENC_RAW);
    for _ in 0..u32::from(W) * u32::from(H) {
        m.extend([bgr[0], bgr[1], bgr[2], 0]);
    }
    m
}

/// A ZRLE update whose payload is `tiles` deflated with `z` (one zlib stream for the session).
pub fn zrle_update(z: &mut Compress, x: u16, y: u16, w: u16, h: u16, tiles: &[u8]) -> Vec<u8> {
    let mut comp = Vec::with_capacity(tiles.len() + 1024);
    z.compress_vec(tiles, &mut comp, FlushCompress::Sync).unwrap();
    let mut m = rect(x, y, w, h, sighter_target::proto::ENC_ZRLE);
    m.extend((comp.len() as u32).to_be_bytes());
    m.extend(comp);
    m
}

pub fn zlib() -> Compress {
    Compress::new(Compression::default(), true)
}

pub fn fast() -> ConnectOptions {
    ConnectOptions {
        connect_timeout: WAIT,
        handshake_timeout: WAIT,
        handshake_total: WAIT,
        write_timeout: WAIT,
        frame_timeout: WAIT,
        update_interval: Duration::from_millis(1),
        stop_poll: Duration::from_millis(5),
    }
}

pub fn addr(f: &Fake) -> VncAddr {
    VncAddr {
        host: "127.0.0.1".into(),
        port: f.port,
    }
}

/// A door that resolves only from a fixed table (no DNS ever) and reaches this computer.
pub fn dial() -> Dial {
    let lookup = StaticLookup::new()
        .with("vm.test", &["127.0.0.1"])
        .with("multi.test", &["::1", "127.0.0.1"])
        .with("public.test", &["93.184.216.34"]);
    let egress = Egress::new(
        Policy::default().reach(Reach::Loopback),
        Arc::new(FakeWire::new()),
        Arc::new(lookup),
        Arc::new(FakeClock::new()),
    );
    Dial::new(Arc::new(egress), Scope::Loopback)
}

pub fn connect(f: &Fake, pw: Option<&str>, stop: StopFlag) -> Result<VncTarget, sighter_core::ports::TargetError> {
    VncTarget::connect(&addr(f), pw, stop, fast(), &dial())
}

pub fn next_event(f: &Fake) -> Vec<u8> {
    f.events.recv_timeout(WAIT).unwrap()
}

/// Waits for the next key event and returns `(keysym, down)`.
pub fn next_key(f: &Fake) -> (u32, u8) {
    loop {
        let e = next_event(f);
        if e[0] == 4 {
            return (u32::from_be_bytes([e[4], e[5], e[6], e[7]]), e[1]);
        }
    }
}
