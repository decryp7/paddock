//! A child process for paddock-ipc's own tests; never shipped by an app. The first argument picks its behaviour:
//!
//! - `echo`: connects and answers every call with its body (`fail` → a typed error, `panic` → a panic, `big <n>` → an
//!   answer of n bytes, `pid` → its pid)
//! - `protocol <major.minor>`: `echo`, speaking that protocol
//! - `exit`: exits at once with code 3, without connecting
//! - `silent`: never connects
//! - `wrong-token`: connects with a token of its own; exits 0 when refused
//! - `stubborn`: `echo` without a lifeline, and keeps running after the connection closes
//! - `companion <discovery file> <build>`: a link server for that build until a client calls `stop`; prints `ready` once
//!   its discovery file is written
//! - `observe <file>` (feature `testing`): connects, reads the raw stream of its connection to the end and writes what it
//!   read to `<file>.bin` and how it ended (`eof`, `reset` or `timeout`) to `<file>.end` (the launcher's ending test)
//! - `out-of-files` (Unix): a host and a raw client in this process; the host accepts the client with the last free file
//!   descriptor, so its handshake cannot take the copies it needs. Prints `host: <its error>` and `client: <the first frame
//!   or the read error>`. Its own process because it lowers this process's limit of open files.
//! - `out-of-files-at-accept` (Unix): the same with no descriptor left for the host's accept itself.
//! - `out-of-files-idle` (Unix): with no descriptor left and nobody connecting, the host's accept (Linux fails it with EMFILE
//!   before it looks for a client) must keep its reserve: prints `free after the idle accept: <bool>`, then the `client:` of
//!   a client that connects once one descriptor is free.
//! - `exit-probe <folder>` (Unix): `echo` through the real child loop (`child::serve`) with a test-only `atexit` handler that records
//!   in the folder's `entered-1` and `entered-2` files whether a second thread entered `exit` while a first was still in
//!   the handlers (the files are opened first: a lockdown may forbid opening them later). The same probe as
//!   `debrief-ipc-fake-child --exit-probe` (the two child loops are copies, `docs/known-copies.toml` `ipc-child-launch`).
//! - `link-out-of-files <discovery file>` (Unix): a `LinkServer` with no descriptor left; prints `ready <endpoint>`, waits for a
//!   line on standard input (the test's client got its answer) and prints `refused: <what the server recorded>`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_ipc::link::{LinkServer, ServerConfig};
use paddock_ipc::{
    CallCtx, Client, FrameLimit, Handler, ProtocolVersion, RemoteError, Token, child,
};
use std::sync::{Arc, Condvar, Mutex};

fn echo(_: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
    match body {
        b"fail" => Err(RemoteError::new("bad", "typed failure")),
        b"panic" => panic!("handler panicked"),
        b"pid" => Ok(std::process::id().to_string().into_bytes()),
        other => match std::str::from_utf8(other)
            .ok()
            .and_then(|s| s.strip_prefix("big "))
        {
            Some(n) => Ok(vec![
                7;
                n.parse().map_err(|_| RemoteError::new(
                    "bad",
                    "not a number"
                ))?
            ]),
            None => Ok(other.to_vec()),
        },
    }
}

/// The test-only `atexit` handlers of `exit-probe`. `holder` is registered last, so it runs first: the first thread to enter it
/// waits on a bounded condition until another thread has been seen in `exit` (or `PROBE_WAIT` passes). `witness` is
/// registered before it, so a thread that reaches it from another thread than the holder's is a second exit: libc marks an
/// entry it is running as called, so a second thread skips `holder` and runs the entries after it (that is how both threads
/// ended up in the static destructors at once on macOS).
#[cfg(unix)]
mod exit_probe {
    use std::fs::File;
    use std::io::Write;
    use std::sync::{Condvar, Mutex, OnceLock};
    use std::time::Duration;

    /// Longest the first thread holds the handlers for a second one (it ends as soon as one arrives).
    const PROBE_WAIT: Duration = Duration::from_millis(500);

    struct State {
        holder: Option<usize>,
        second: bool,
    }

    static FILES: OnceLock<(Mutex<File>, Mutex<File>)> = OnceLock::new();
    static STATE: (Mutex<State>, Condvar) = (
        Mutex::new(State {
            holder: None,
            second: false,
        }),
        Condvar::new(),
    );

    /// The calling thread, without `std::thread::current` (not usable once a thread's locals are destroyed, as in `exit`).
    fn thread_id() -> usize {
        // SAFETY: `pthread_self` has no preconditions.
        unsafe { libc::pthread_self() as usize }
    }

    fn mark(file: &Mutex<File>) {
        if let Ok(mut f) = file.lock() {
            let _ = f.write_all(b"x");
            let _ = f.flush();
        }
    }

    fn second_seen(state: &mut State) {
        if !state.second {
            state.second = true;
            if let Some((_, second)) = FILES.get() {
                mark(second);
            }
            STATE.1.notify_all();
        }
    }

    extern "C" fn holder() {
        let Some((first, _)) = FILES.get() else {
            return;
        };
        let Ok(mut st) = STATE.0.lock() else { return };
        let me = thread_id();
        match st.holder {
            Some(h) if h != me => second_seen(&mut st),
            Some(_) => {}
            None => {
                st.holder = Some(me);
                mark(first);
                let _ = STATE.1.wait_timeout_while(st, PROBE_WAIT, |s| !s.second);
            }
        }
    }

    extern "C" fn witness() {
        let Ok(mut st) = STATE.0.lock() else { return };
        if st.holder.is_some_and(|h| h != thread_id()) {
            second_seen(&mut st);
        }
    }

    /// Opens the marker files (before the lockdown).
    pub fn open(dir: &str) {
        let make = |name: &str| {
            Mutex::new(File::create(std::path::Path::new(dir).join(name)).expect("probe file"))
        };
        let _ = FILES.set((make("entered-1"), make("entered-2")));
    }

    /// Registers the handlers (once the connection is up).
    pub fn register() {
        // SAFETY: both are plain `extern "C" fn()`.
        unsafe {
            libc::atexit(witness);
            libc::atexit(holder);
        }
    }
}

fn park_forever() -> ! {
    loop {
        std::thread::park();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    match arg(1).as_str() {
        "exit" => std::process::exit(3),
        #[cfg(unix)]
        "exit-probe" => {
            exit_probe::open(&arg(2));
            // `main` returns after `serve`: its return is the child's one exit once the lifeline closes the connection.
            let r = child::serve(
                "host",
                ProtocolVersion::UNVERSIONED,
                |_| Arc::new(echo) as Arc<dyn Handler>,
                |_, _| exit_probe::register(),
            );
            if r.is_err() {
                std::process::exit(2);
            }
        }
        "silent" => park_forever(),
        "wrong-token" => {
            let b = child::read_bootstrap(std::io::stdin().lock()).unwrap();
            let r = Client::connect(
                &b.endpoint,
                &Token::from_bytes([9; 32]),
                FrameLimit::default(),
                ProtocolVersion::UNVERSIONED,
            );
            std::process::exit(if r.is_err() { 0 } else { 1 });
        }
        #[cfg(feature = "testing")]
        "observe" => {
            use paddock_ipc::testing::endings::raw::{READ_GUARD, RawEndKind, read_to_end};
            let b = child::read_bootstrap(std::io::stdin().lock()).unwrap();
            let mut conn = child::connect(&b, ProtocolVersion::UNVERSIONED).unwrap();
            let end = read_to_end(conn.stream_mut(), READ_GUARD);
            let file = arg(2);
            std::fs::write(format!("{file}.bin"), &end.bytes).unwrap();
            let kind = match end.kind {
                RawEndKind::Eof => "eof".to_string(),
                RawEndKind::TimedOut => "timeout".to_string(),
                k if k.is_reset() => "reset".to_string(),
                RawEndKind::Error(e) => format!("error {e:?}"),
            };
            std::fs::write(format!("{file}.end"), kind).unwrap();
            std::process::exit(0);
        }
        "stubborn" => {
            let b = child::read_bootstrap(std::io::stdin().lock()).unwrap();
            let conn = child::connect(&b, ProtocolVersion::UNVERSIONED).unwrap();
            let peer = paddock_ipc::Peer::start(
                conn,
                Arc::new(echo),
                paddock_ipc::PeerOptions::new("host"),
            )
            .unwrap();
            peer.wait_closed(None);
            park_forever();
        }
        "protocol" => {
            let version: ProtocolVersion = arg(2).parse().unwrap();
            let r = child::serve(
                "host",
                version,
                |_| Arc::new(echo) as Arc<dyn Handler>,
                |_, _| {},
            );
            std::process::exit(if r.is_ok() { 0 } else { 2 });
        }
        "companion" => {
            let stopped = Arc::new((Mutex::new(false), Condvar::new()));
            let s = stopped.clone();
            let handler = move |ctx: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> {
                if body == b"stop" {
                    *s.0.lock().unwrap() = true;
                    s.1.notify_all();
                    return Ok(Vec::new());
                }
                echo(ctx, body)
            };
            let server =
                LinkServer::start(ServerConfig::new(arg(2), arg(3)), Arc::new(handler)).unwrap();
            // The test reads this line as its "ready" event instead of polling.
            println!("ready");
            let guard = stopped.0.lock().unwrap();
            drop(stopped.1.wait_while(guard, |s| !*s).unwrap());
            drop(server);
        }
        #[cfg(unix)]
        "out-of-files" => out_of_files(true),
        #[cfg(unix)]
        "out-of-files-at-accept" => out_of_files(false),
        #[cfg(unix)]
        "out-of-files-idle" => out_of_files_idle(),
        #[cfg(unix)]
        "link-out-of-files" => link_out_of_files(&arg(2)),
        _ => {
            let r = child::serve(
                "host",
                ProtocolVersion::UNVERSIONED,
                |_| Arc::new(echo) as Arc<dyn Handler>,
                |_, _| {},
            );
            std::process::exit(if r.is_ok() { 0 } else { 2 });
        }
    }
}

/// The `out-of-files` modes: see the file header; with `for_accept`, one descriptor is free for the host's accept. Every
/// step is bounded (the client's read has a limit).
#[cfg(unix)]
fn out_of_files(for_accept: bool) {
    use paddock_ipc::Host;
    use paddock_ipc::endpoint::Stream;
    use paddock_ipc::frame::{self, Frame};
    use std::time::Duration;
    let limit = FrameLimit::default();
    let mut host = Host::listen(limit, ProtocolVersion::UNVERSIONED).unwrap();
    let mut fillers = fill_descriptors();
    // One free descriptor for the client, then (`for_accept`) one for the host's accept: none is left for what follows.
    drop(fillers.pop());
    let mut client = Stream::connect(host.endpoint(), Duration::from_secs(10)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let hello = Frame::Hello {
        version: frame::VERSION,
        protocol: ProtocolVersion::UNVERSIONED,
        token: *host.token().as_bytes(),
        pid: std::process::id(),
    };
    frame::write_frame(&mut client, &hello, limit).unwrap();
    if for_accept {
        drop(fillers.pop());
    }
    let host_result = host.accept(Duration::from_secs(10));
    let answer = frame::read_frame(&mut client, limit);
    drop(fillers);
    println!("host: {:?}", host_result.map(|_| "welcomed"));
    println!("client: {answer:?}");
}

/// Lowers this process's limit of open files (filling the table stays cheap) and opens files until none is left; the
/// caller frees exactly the descriptors it wants free by dropping fillers.
#[cfg(unix)]
fn fill_descriptors() -> Vec<std::fs::File> {
    // A low limit keeps filling the table cheap; what this process holds now is far below it.
    let mut limits = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: plain calls with a valid pointer to a local struct.
    #[allow(unsafe_code)]
    unsafe {
        assert_eq!(libc::getrlimit(libc::RLIMIT_NOFILE, &mut limits), 0);
        limits.rlim_cur = 64;
        assert_eq!(libc::setrlimit(libc::RLIMIT_NOFILE, &limits), 0);
    }
    let mut fillers = Vec::new();
    while let Ok(f) = std::fs::File::open("/dev/null") {
        fillers.push(f);
    }
    // A slot another thread held for a moment when the loop above hit EMFILE (Linux's accept4 takes one before it looks
    // for a client) is free again now: take every slot below the limit, so "no descriptor left" holds. Bounded.
    let below = libc::c_int::try_from(limits.rlim_cur).unwrap_or(libc::c_int::MAX);
    // SAFETY: F_GETFD only reads the flags of a descriptor number; a closed one is EBADF.
    #[allow(unsafe_code)]
    let free = || {
        (0..below)
            .filter(|fd| unsafe { libc::fcntl(*fd, libc::F_GETFD) } == -1)
            .count()
    };
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while free() > 0 && std::time::Instant::now() < until {
        match std::fs::File::open("/dev/null") {
            Ok(f) => fillers.push(f),
            Err(_) => std::thread::yield_now(),
        }
    }
    assert_eq!(free(), 0, "the descriptor table could not be filled");
    fillers
}

/// The `link-out-of-files` mode: see the file header.
#[cfg(unix)]
fn link_out_of_files(discovery: &str) {
    let server = LinkServer::start(ServerConfig::new(discovery, "b"), Arc::new(echo)).unwrap();
    // No descriptor left for the server's accept. The client is the test process (a table of its own), so nothing here
    // competes with the accept thread for a free slot.
    let fillers = fill_descriptors();
    println!("ready {}", server.endpoint());
    // The test writes a line once its client has the answer.
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    drop(fillers);
    println!("refused: {:?}", server.refused());
}

/// The `out-of-files-idle` mode: see the file header.
#[cfg(unix)]
fn out_of_files_idle() {
    use paddock_ipc::Host;
    use paddock_ipc::endpoint::Stream;
    use paddock_ipc::frame;
    use std::time::Duration;
    let limit = FrameLimit::default();
    let mut host = Host::listen(limit, ProtocolVersion::UNVERSIONED).unwrap();
    let mut fillers = fill_descriptors();
    let idle = host.accept(Duration::from_millis(200));
    // A reserve given up for nobody leaves a descriptor free here.
    let free = std::fs::File::open("/dev/null");
    let was_free = free.is_ok();
    drop(free);
    drop(fillers.pop());
    let mut client = Stream::connect(host.endpoint(), Duration::from_secs(10)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let host_result = host.accept(Duration::from_secs(10));
    let answer = frame::read_frame(&mut client, limit);
    drop(fillers);
    println!("idle: {:?}", idle.map(|_| "welcomed"));
    println!("free after the idle accept: {was_free}");
    println!("host: {:?}", host_result.map(|_| "welcomed"));
    println!("client: {answer:?}");
}
