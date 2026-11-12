//! A fake child for the launcher and supervisor tests (never bundled). It runs [`debrief_ipc::child::child_main`]
//! and answers text requests:
//!
//! - `echo <text>` → `<text>`; `env <NAME>` → the variable or `<unset>`; `exit <code>` → exits at once (a crash);
//!   `hang` → never answers; `panic` → panics in the handler; `big <n>` → n bytes; `stream <n>` → n items;
//!   `alloc <n>` → allocates and touches n bytes, `ok …` or `denied: …`; `hold <n> <ms>` → the same, kept for ms
//!   before answering (so the host's memory watcher sees it);
//!   `write <path>` → tries to create the file, answers `ok` or the error; `spawn` → tries to start a program;
//!   `ask <text>` → calls the host back with `<text>` and answers with the host's reply; `gaps` → the notes of its
//!   lockdown (JSON), so tests know what this system enforces; `read <path>` → `ok <bytes>` or the error;
//!   `connect <ip:port>` → opens a TCP connection, `ok` or the error.
//!
//! Flags: `--leak-endpoint=<prefix>` (makes a private endpoint, prints its address and exits without dropping it),
//! `--exit-now` (dies before connecting), `--no-connect` (reads the bootstrap, never connects),
//! `--bad-token` (connects with a wrong token), `--relay` (starts a copy of itself that connects instead: a
//! process the host did not start), `--garbage` (sends bytes that are not a frame after the handshake),
//! `--oversize` (announces a 1 GiB frame after the handshake), `--say=<text>` (a line on stderr first),
//! `--protocol=<major.minor>` (speaks that protocol version in the handshake), `--skip-lockdown` (serves without
//! locking itself down: a binary that ignores the sandbox, to test what the launcher enforces), `--exit-probe=<folder>`
//! (runs the real child loop, [`debrief_ipc::child::child_main_then`], with a test-only `atexit` handler that records in
//! the folder's `entered-1` and `entered-2` files whether a second thread entered `exit` while a first was still in the
//! handlers; the files are opened before the lockdown, which may forbid opening them later), `--slow-exit=<folder>`
//! (the child loop, then 600 ms of work before `main` returns, as the store worker joining its threads, and a `finished`
//! file in the folder when it did: a parent that kills it sooner leaves the file empty).

use debrief_ipc::child::{connect_speaking, lifeline, read_bootstrap};
use debrief_ipc::frame::ProtocolVersion;
use debrief_ipc::peer::{code, CallCtx, Handler};
use debrief_ipc::RemoteError;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

fn handle(ctx: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
    let text = String::from_utf8_lossy(body).into_owned();
    let (cmd, arg) = text.split_once(' ').unwrap_or((text.as_str(), ""));
    let fail = |m: String| RemoteError::new(code::INTERNAL, m);
    match cmd {
        "echo" => Ok(arg.as_bytes().to_vec()),
        "gaps" => Ok(serde_json::to_vec(debrief_ipc::sandbox::applied_notes()).map_err(|e| fail(e.to_string()))?),
        "env" => Ok(std::env::var(arg).unwrap_or_else(|_| "<unset>".into()).into_bytes()),
        "exit" => std::process::exit(arg.parse().unwrap_or(3)),
        "hang" => loop {
            std::thread::sleep(Duration::from_secs(3600));
        },
        "panic" => panic!("handler panicked"),
        "big" => Ok(vec![b'x'; arg.parse().unwrap_or(0)]),
        "alloc" => {
            // Allocates and touches n bytes without aborting when it cannot (`denied`).
            let n: usize = arg.parse().unwrap_or(0);
            let mut v: Vec<u8> = Vec::new();
            Ok(match v.try_reserve_exact(n) {
                Ok(()) => {
                    v.resize(n, 1);
                    format!("ok {}", v.iter().step_by(4096).map(|b| *b as usize).sum::<usize>()).into_bytes()
                }
                Err(e) => format!("denied: {e}").into_bytes(),
            })
        }
        "hold" => {
            let (n, ms) = arg.split_once(' ').unwrap_or((arg, "0"));
            let n: usize = n.parse().unwrap_or(0);
            let v = vec![1u8; n];
            std::thread::sleep(Duration::from_millis(ms.parse().unwrap_or(0)));
            Ok(format!("ok {}", v.iter().step_by(4096).map(|b| *b as usize).sum::<usize>()).into_bytes())
        }
        "stream" => {
            for i in 0..arg.parse::<u32>().unwrap_or(0) {
                ctx.send_item(i.to_le_bytes().to_vec())?;
            }
            Ok(b"done".to_vec())
        }
        "read" => Ok(match std::fs::read(arg) {
            Ok(b) => format!("ok {}", b.len()).into_bytes(),
            Err(e) => format!("denied: {e}").into_bytes(),
        }),
        "connect" => Ok(
            match arg
                .parse()
                .map(|a| std::net::TcpStream::connect_timeout(&a, Duration::from_secs(3)))
            {
                Ok(Ok(_)) => b"ok".to_vec(),
                Ok(Err(e)) => format!("denied: {e}").into_bytes(),
                Err(e) => format!("bad address: {e}").into_bytes(),
            },
        ),
        "write" => Ok(match std::fs::write(arg, b"x") {
            Ok(()) => {
                let _ = std::fs::remove_file(arg);
                b"ok".to_vec()
            }
            Err(e) => format!("denied: {e}").into_bytes(),
        }),
        "spawn" => {
            #[allow(clippy::disallowed_methods)]
            let r = std::process::Command::new(std::env::current_exe().map_err(|e| fail(e.to_string()))?)
                .arg("--exit-now")
                .status();
            Ok(match r {
                Ok(_) => b"ok".to_vec(),
                Err(e) => format!("denied: {e}").into_bytes(),
            })
        }
        "ask" => ctx
            .peer()
            .call(arg.as_bytes().to_vec(), Some(Duration::from_secs(10)))
            .map_err(|e| fail(e.to_string())),
        _ => Err(RemoteError::new(code::UNIMPLEMENTED, format!("unknown request {cmd}"))),
    }
}

/// The test-only `atexit` handlers of `--exit-probe`. `holder` is registered last, so it runs first: the first thread to
/// enter it waits on a bounded condition until another thread has been seen in `exit` (or [`PROBE_WAIT`] passes).
/// `witness` is registered before it, so a thread that reaches it from another thread than the holder's is a second
/// exit: libc marks an entry it is running as called, so a second thread skips `holder` and runs the entries after it
/// (that is how both threads ended up in the static destructors at once on macOS).
mod exit_probe {
    use std::fs::File;
    use std::io::Write;
    use std::sync::{Condvar, Mutex, OnceLock};
    use std::time::Duration;

    /// Longest the first thread holds the handlers for a second one (never a sleep: it ends as soon as one arrives).
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
    #[cfg(unix)]
    fn thread_id() -> usize {
        // SAFETY: `pthread_self` has no preconditions.
        unsafe { libc::pthread_self() as usize }
    }

    #[cfg(not(unix))]
    fn thread_id() -> usize {
        0
    }

    fn mark(file: &Mutex<File>) {
        if let Ok(mut f) = file.lock() {
            let _ = f.write_all(b"x");
            let _ = f.flush();
        }
    }

    /// Records a thread other than the holder's.
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
        let Some((first, _)) = FILES.get() else { return };
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
        let make = |name: &str| Mutex::new(File::create(std::path::Path::new(dir).join(name)).expect("probe file"));
        let _ = FILES.set((make("entered-1"), make("entered-2")));
    }

    /// Registers the handlers (after the connection is up, so they run among the handlers DuckDB would have registered).
    pub fn register() {
        // SAFETY: both are plain `extern "C" fn()`.
        #[cfg(unix)]
        unsafe {
            libc::atexit(witness);
            libc::atexit(holder);
        }
        #[cfg(not(unix))]
        let _ = (holder, witness); // the probe is a Unix test (std's `exit` on Windows is `ExitProcess`)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(dir) = args.iter().find_map(|a| a.strip_prefix("--exit-probe=")) {
        exit_probe::open(dir);
        let r = debrief_ipc::child::child_main_then(
            "Debrief",
            ProtocolVersion::UNVERSIONED,
            |_| Arc::new(handle),
            |_, _| exit_probe::register(),
        );
        if let Err(e) = r {
            eprintln!("fake child: {e}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(dir) = args.iter().find_map(|a| a.strip_prefix("--slow-exit=")) {
        // Opened before the lockdown, which may forbid opening it later.
        let mut finished = std::fs::File::create(std::path::Path::new(dir).join("finished")).expect("marker file");
        let r = debrief_ipc::child::child_main_speaking("Debrief", ProtocolVersion::UNVERSIONED, |_| Arc::new(handle));
        std::thread::sleep(Duration::from_millis(600));
        let _ = std::io::Write::write_all(&mut finished, b"x");
        if let Err(e) = r {
            eprintln!("fake child: {e}");
            std::process::exit(1);
        }
        return;
    }
    for say in args.iter().filter_map(|a| a.strip_prefix("--say=")) {
        eprintln!("{say}");
    }
    if let Some(prefix) = args.iter().find_map(|a| a.strip_prefix("--leak-endpoint=")) {
        // A host that makes its private endpoint and then ends without running destructors (a crash, `exit`).
        let l = debrief_ipc::link::Listener::create(prefix, debrief_ipc::link::Access::Owner).expect("endpoint");
        println!("{}", l.address());
        std::process::exit(0);
    }
    if args.iter().any(|a| a == "--exit-now") {
        std::process::exit(3);
    }
    if args.iter().any(|a| a == "--relay") {
        // Hand the bootstrap to a copy of itself, which connects with the right token but another pid.
        let mut line = String::new();
        let _ = std::io::BufRead::read_line(&mut std::io::BufReader::new(std::io::stdin().take(64 * 1024)), &mut line);
        #[allow(clippy::disallowed_methods)]
        let mut c = std::process::Command::new(std::env::current_exe().expect("exe"))
            .stdin(std::process::Stdio::piped())
            .spawn()
            .expect("spawn");
        if let Some(mut i) = c.stdin.take() {
            use std::io::Write;
            let _ = i.write_all(line.as_bytes());
            let _ = i.flush();
            std::thread::sleep(Duration::from_secs(30));
        }
        let _ = c.kill();
        let _ = c.wait();
        return;
    }
    let mut bootstrap = match read_bootstrap(std::io::stdin().lock()) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("fake child: {e}");
            std::process::exit(1);
        }
    };
    if args.iter().any(|a| a == "--no-connect") {
        lifeline();
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
    if args.iter().any(|a| a == "--bad-token") {
        bootstrap.token = "ab".repeat(32);
    }
    if args.iter().any(|a| a == "--garbage" || a == "--oversize") {
        let token = debrief_ipc::link::Token::from_hex(&bootstrap.token).expect("token");
        let address = bootstrap.address.clone();
        let bytes: Vec<u8> = if args.iter().any(|a| a == "--garbage") {
            vec![5, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0xff]
        } else {
            (1u32 << 30).to_le_bytes().to_vec()
        };
        lifeline();
        let _ = debrief_ipc::runtime::run(async move {
            use tokio::io::AsyncWriteExt;
            if let Ok(mut io) = debrief_ipc::link::handshake(&address, &token, "Debrief").await {
                let _ = io.write_all(&bytes).await;
                let _ = io.flush().await;
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        });
        return;
    }
    let handler: Arc<dyn Handler> = Arc::new(handle);
    if args.iter().any(|a| a == "--skip-lockdown") {
        let token = debrief_ipc::link::Token::from_hex(&bootstrap.token).expect("token");
        let address = bootstrap.address.clone();
        let rt = debrief_ipc::runtime::shared().expect("runtime");
        let io = rt
            .block_on(async move { debrief_ipc::link::handshake(&address, &token, "Debrief").await })
            .expect("handshake");
        let peer = debrief_ipc::Peer::start(io, handler, debrief_ipc::PeerOptions::new("Debrief"), rt.handle());
        peer.wait_closed(None);
        return;
    }
    let protocol = args
        .iter()
        .find_map(|a| a.strip_prefix("--protocol="))
        .and_then(|p| p.parse().ok())
        .unwrap_or(ProtocolVersion::UNVERSIONED);
    match connect_speaking(&bootstrap, false, "Debrief", protocol, |_| handler) {
        Ok(peer) => {
            // As child_main does: after the lockdown, so the thread is locked down too.
            debrief_ipc::child::lifeline_closing(&peer);
            peer.wait_closed(None);
            debrief_ipc::child::exit_phase_begun();
        }
        Err(e) => {
            eprintln!("fake child: {e}");
            std::process::exit(1);
        }
    }
}
