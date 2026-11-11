//! The one child loop (ADR 0017 §2, §5): a child process' `main` calls [`child_main`], which
//!
//! 1. reads its [`Bootstrap`] (one JSON line on stdin, or the file `--debrief-bootstrap <file>` names for an elevated
//!    child, which has no stdin pipe);
//! 2. connects to the host and authenticates;
//! 3. locks itself down ([`crate::sandbox::lockdown`]) before it handles anything;
//! 4. answers the host's calls with its handler until the connection closes, then returns (the process exits).
//!
//! A child also ends when its stdin closes (the host died); nothing is left behind. It ends through **one** exit: the
//! [`lifeline`] closes the connection, `main` returns and is the only `exit` (Rust guards two `exit`s only on Linux; on
//! macOS two threads ran the `atexit` handlers and DuckDB's static destructors together, and the store worker crashed).

use crate::elevation::{take_bootstrap, BOOTSTRAP_FLAG};
use crate::frame::ProtocolVersion;
use crate::launch::Bootstrap;
use crate::link::Token;
use crate::peer::{Handler, Peer, PeerOptions};
use std::io::{self, BufRead, Read};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// What a child's exit phase may take once its host is gone or stops it: the child's own stop budget. The parent waits at
/// least this plus [`STOP_MARGIN`] before it kills the child (`process::KILL_GRACE`), so a child that stops within its
/// budget is never killed in the middle of it.
pub const STOP_BUDGET: Duration = Duration::from_millis(1250);

/// Covers the parent's kill and reap, and a child's last log lines, on top of [`STOP_BUDGET`].
pub const STOP_MARGIN: Duration = Duration::from_millis(250);

/// Who ends the process: the child's `main` (past the connection) or, after a bound, the lifeline. One claim wins, under
/// one lock, so the two never both run an exit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Exit {
    Serving,
    MainExiting,
    LifelineExiting,
}

struct Phase {
    state: Mutex<Exit>,
    wake: Condvar,
}

static PHASE: Phase = Phase {
    state: Mutex::new(Exit::Serving),
    wake: Condvar::new(),
};

impl Phase {
    #[cfg(test)]
    fn new() -> Phase {
        Phase {
            state: Mutex::new(Exit::Serving),
            wake: Condvar::new(),
        }
    }

    /// `main` claims the exit; false when the lifeline claimed it first (this `main` must then park: the lifeline ends the
    /// process).
    fn claim_main(&self) -> bool {
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if *st == Exit::LifelineExiting {
            return false;
        }
        *st = Exit::MainExiting;
        self.wake.notify_all();
        true
    }

    /// Waits at most `max` for `main` to claim the exit; if it has not by then, the lifeline claims it and this says true.
    fn claim_fallback(&self, max: Duration) -> bool {
        let g = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut g = self
            .wake
            .wait_timeout_while(g, max, |st| *st == Exit::Serving)
            .map(|(g, _)| g)
            .unwrap_or_else(|p| p.into_inner().0);
        if *g == Exit::Serving {
            *g = Exit::LifelineExiting;
            return true;
        }
        false
    }
}

/// Says that this child's exit phase has begun: its `main` is past the connection and ends the process itself. A child
/// whose `main` calls [`child_main`] never calls this (it does); one that serves by [`connect_speaking`] and
/// [`lifeline_closing`] calls it when its own `wait_closed` returns. If the lifeline's fallback already took the exit
/// (a `main` that was too late), this waits at most [`STOP_MARGIN`] for the lifeline to end the process, then ends it itself.
pub fn exit_phase_begun() {
    begin_exit(&PHASE, STOP_MARGIN, || end_without_destructors(0));
}

/// `main`'s claim on `phase`; when the lifeline's fallback holds it, see [`main_lost_claim`].
fn begin_exit(phase: &Phase, wait: Duration, end: impl FnOnce()) {
    if !phase.claim_main() {
        main_lost_claim(wait, end);
    }
}

/// A `main` that finds the lifeline's fallback already took the exit: waits at most `wait` for the lifeline to end the
/// process, then calls `end` itself (never parks for good: the lifeline may have died, and this `main` holds what the child
/// holds).
fn main_lost_claim(wait: Duration, end: impl FnOnce()) {
    std::thread::park_timeout(wait);
    end();
}

/// Largest bootstrap line read from stdin.
const MAX_BOOTSTRAP: u64 = 64 * 1024;

/// Reads the bootstrap line from `input`.
pub fn read_bootstrap(input: impl Read) -> io::Result<Bootstrap> {
    let mut line = String::new();
    io::BufReader::new(input.take(MAX_BOOTSTRAP)).read_line(&mut line)?;
    serde_json::from_str(line.trim()).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad bootstrap: {e}")))
}

/// Where the bootstrap comes from: `--debrief-bootstrap <file>` among `args` (an elevated child), else stdin.
pub fn bootstrap_file(args: &[String]) -> Option<std::path::PathBuf> {
    args.iter()
        .position(|a| a == BOOTSTRAP_FLAG)
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from)
}

/// A child process' `main`; see the module docs. `label` names the host in messages ("Debrief"); `make` builds the
/// handler once the bootstrap is known (it may look at [`Bootstrap::role`]).
pub fn child_main(label: &str, make: impl FnOnce(&Bootstrap) -> Arc<dyn Handler>) -> io::Result<()> {
    child_main_speaking(label, ProtocolVersion::UNVERSIONED, make)
}

/// [`child_main`] for a child that speaks a versioned protocol (a plugin): `protocol` is sent in the handshake, and
/// a host that cannot serve it refuses the connection with a message saying which side to update.
pub fn child_main_speaking(label: &str, protocol: ProtocolVersion, make: impl FnOnce(&Bootstrap) -> Arc<dyn Handler>) -> io::Result<()> {
    child_main_then(label, protocol, make, |_, _| {})
}

/// [`child_main_speaking`] that runs `connected` once the connection is up and the child is locked down (e.g. to
/// start forwarding the child's own standard error over the connection).
pub fn child_main_then(
    label: &str,
    protocol: ProtocolVersion,
    make: impl FnOnce(&Bootstrap) -> Arc<dyn Handler>,
    connected: impl FnOnce(&Bootstrap, &Peer),
) -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let elevated = bootstrap_file(&args);
    let bootstrap = match &elevated {
        Some(file) => take_bootstrap(file)?,
        None => read_bootstrap(io::stdin().lock())?,
    };
    let peer = connect_speaking(&bootstrap, elevated.is_some(), label, protocol, make)?;
    connected(&bootstrap, &peer);
    if elevated.is_none() {
        // Started after the lockdown, like every thread of a child (see `connect_and_lock_down`).
        lifeline_closing(&peer);
    }
    peer.wait_closed(None);
    exit_phase_begun();
    Ok(())
}

/// The lifeline of a child that has no connection yet (or none to close): when the host closes stdin (or dies), exit at
/// once. Once the peer is up use [`lifeline_closing`]: two threads in `exit` crashed the store worker.
pub fn lifeline() {
    std::thread::spawn(|| {
        wait_stdin_closed(io::stdin());
        // `exit` runs no destructors: remove the endpoints of children this process was still starting.
        crate::endpoint::remove_live();
        std::process::exit(0);
    });
}

/// The lifeline of a child whose connection is up: when the host closes stdin (or dies) it **closes the connection**, so
/// the child's `main` (past `wait_closed`) is the only exit. If the exit phase has not begun within
/// [`STOP_BUDGET`] plus [`STOP_MARGIN`] (a `main` that hangs would keep the store's database lock), it ends the
/// process itself. Call [`exit_phase_begun`] from a `main` that does not go through [`child_main`].
pub fn lifeline_closing(peer: &Peer) {
    let peer = peer.clone();
    std::thread::spawn(move || {
        wait_stdin_closed(io::stdin());
        let end = || end_without_destructors(0);
        lifeline_end(&peer, &PHASE, LIFELINE_WAIT, &mut io::stderr(), end);
    });
}

/// Ends this process at once without running `atexit` handlers or static destructors (`_exit`, `TerminateProcess` on
/// itself): for a child whose exit phase overran, where running the C++ static destructors under a thread still inside
/// native code is what crashed the store worker. Stdio is not flushed: the caller logs first.
#[cfg(unix)]
#[rustfmt::skip]
pub fn end_without_destructors(code: i32) -> ! { /* SAFETY: `_exit` takes no pointers and never returns. */ unsafe { libc::_exit(code) } }

/// [`end_without_destructors`] on Windows (`TerminateProcess` on this process).
#[cfg(windows)]
pub fn end_without_destructors(code: i32) -> ! {
    use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    // SAFETY: the pseudo-handle of this process; `TerminateProcess` notifies no DLL, so no detach code can wait on a
    // thread that was cut short.
    unsafe {
        // conformance: pid-kill — this process itself (`GetCurrentProcess`), not a pid read earlier
        let _ = TerminateProcess(GetCurrentProcess(), code as u32);
    }
    std::process::abort()
}

fn wait_stdin_closed(mut stdin: impl Read) {
    let mut sink = [0u8; 256];
    while matches!(stdin.read(&mut sink), Ok(n) if n > 0) {}
}

/// What the lifeline does once stdin is closed: close `peer`, wait for `main` to claim the exit, and if it does not within
/// the child's stop budget plus margin, say so on `err` and `end` the process.
fn lifeline_end(peer: &Peer, phase: &Phase, wait: Duration, err: &mut dyn io::Write, end: impl FnOnce()) {
    close_then_end(peer, phase, wait, || end_after_fallback(err, end));
}

/// The last steps of the lifeline's fallback: say why and remove the endpoints of children this process was still starting
/// (`exit` runs no destructors), then `end`. Nothing before `end` may stop it: a broken stderr (the host is gone) or a
/// writer that panics must not keep the process alive, holding what the child holds (the store's database lock).
fn end_after_fallback(err: &mut dyn std::io::Write, end: impl FnOnce()) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = writeln!(
            err,
            "the exit phase did not begin within {} ms of the host going away; ending the process (exit code 0)",
            (STOP_BUDGET + STOP_MARGIN).as_millis()
        );
        crate::endpoint::remove_live();
    }));
    end();
}

/// How long the lifeline waits for `main` to claim the exit after the host went away.
const LIFELINE_WAIT: Duration = STOP_BUDGET.saturating_add(STOP_MARGIN);

/// Closes `peer`, then waits at most `wait` for `phase` to begin; `fallback` runs when it does not.
fn close_then_end(peer: &Peer, phase: &Phase, wait: Duration, fallback: impl FnOnce()) {
    // Endpoints of children this process was still starting: `main`'s exit runs no destructors for them either.
    crate::endpoint::remove_live();
    peer.close();
    if phase.claim_fallback(wait) {
        fallback();
    }
}

/// Steps 2–4 of [`child_main`]: connect, lock down, serve. Returns the running peer.
pub fn connect_and_lock_down(
    bootstrap: &Bootstrap,
    elevated: bool,
    label: &str,
    make: impl FnOnce(&Bootstrap) -> Arc<dyn Handler>,
) -> io::Result<Peer> {
    connect_speaking(bootstrap, elevated, label, ProtocolVersion::UNVERSIONED, make)
}

/// [`connect_and_lock_down`] for a child that speaks `protocol`.
pub fn connect_speaking(
    bootstrap: &Bootstrap,
    elevated: bool,
    label: &str,
    protocol: ProtocolVersion,
    make: impl FnOnce(&Bootstrap) -> Arc<dyn Handler>,
) -> io::Result<Peer> {
    let token =
        Token::from_hex(&bootstrap.token).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad token in the bootstrap"))?;
    let stream = handshake_then_lock_down(bootstrap, &token, elevated, label, protocol)?;
    let handler = make(bootstrap);
    let mut opts = PeerOptions::new(label);
    opts.max_frame = bootstrap.max_frame;
    let rt = crate::runtime::shared().map_err(io::Error::other)?;
    Ok(Peer::start(stream, handler, opts, rt.handle()))
}

/// Unix: landlock restricts only the thread that locks down and the threads it starts afterwards, so the IPC
/// runtime's threads (which run the handler) must start after the lockdown. The handshake runs on a short-lived
/// thread of its own, with a single-threaded runtime, and is over before the lockdown; the stream then moves to
/// the shared runtime.
#[cfg(unix)]
fn handshake_then_lock_down(
    bootstrap: &Bootstrap,
    token: &Token,
    elevated: bool,
    label: &str,
    protocol: ProtocolVersion,
) -> io::Result<Box<dyn crate::peer::Duplex>> {
    let (stream, tid) = std::thread::scope(|s| {
        s.spawn(|| -> io::Result<(std::os::unix::net::UnixStream, u32)> {
            let tid = current_tid();
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
            let stream = rt
                .block_on(crate::link::platform_handshake(&bootstrap.address, token, label, protocol))
                .map_err(|e| io::Error::other(e.to_string()))?;
            Ok((stream.into_std()?, tid))
        })
        .join()
        .map_err(|_| io::Error::other("the handshake thread panicked"))?
    })?;
    // `join` returns when the thread's user code is over, but the kernel lists its task until it is torn down: landlock's
    // count of threads that predate the lockdown would take that exiting task for a thread that escapes it.
    // A task that outlives the bound is a thread that really runs on: the lockdown then reports it, as it should.
    wait_task_gone(tid, TASK_GONE_BOUND);
    lock_down(bootstrap, elevated)?;
    // `from_std` registers with the reactor of the runtime entered: the shared one, started now.
    let _g = crate::runtime::shared().map_err(io::Error::other)?.enter();
    Ok(Box::new(tokio::net::UnixStream::from_std(stream)?))
}

/// How long to wait for a joined thread's task to leave the kernel's list; it is gone within microseconds, the bound
/// only keeps a broken `/proc` from stalling the child.
#[cfg(unix)]
const TASK_GONE_BOUND: Duration = Duration::from_secs(2);

/// The kernel's id of the calling thread (the name its task has in `/proc/self/task`); 0 where there is none.
#[cfg(unix)]
fn current_tid() -> u32 {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: `gettid` takes no arguments and cannot fail.
        u32::try_from(unsafe { libc::syscall(libc::SYS_gettid) }).unwrap_or(0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// Waits (at most `bound`) until thread `tid` of this process has left `/proc/self/task`; true when it has. Nothing to
/// wait for (true) where there is no such directory or for tid 0. Giving up is logged on stderr, the child's log.
#[cfg(unix)]
fn wait_task_gone(tid: u32, bound: Duration) -> bool {
    if tid == 0 {
        return true;
    }
    let path = std::path::PathBuf::from(format!("/proc/self/task/{tid}"));
    let started = std::time::Instant::now();
    let gone = wait_until(|| !path.exists(), bound);
    if !gone {
        eprintln!(
            "the handshake thread {tid} was still listed after {} ms: the lockdown will count it as a thread that escapes it",
            started.elapsed().as_millis()
        );
    }
    gone
}

/// Polls `done` until it is true or `bound` has passed; true when it was met. The wait is on the condition: a short
/// pause between looks only spares the CPU, it never decides the outcome.
#[cfg(unix)]
fn wait_until(mut done: impl FnMut() -> bool, bound: Duration) -> bool {
    let end = std::time::Instant::now() + bound;
    let mut looks = 0u32;
    loop {
        if done() {
            return true;
        }
        if std::time::Instant::now() >= end {
            return false;
        }
        looks += 1;
        if looks <= 64 {
            std::thread::yield_now();
        } else {
            std::thread::sleep(Duration::from_micros(200));
        }
    }
}

/// Windows: the lowered integrity level and the job hold for every thread of the process.
#[cfg(not(unix))]
fn handshake_then_lock_down(
    bootstrap: &Bootstrap,
    token: &Token,
    elevated: bool,
    label: &str,
    protocol: ProtocolVersion,
) -> io::Result<Box<dyn crate::peer::Duplex>> {
    let (address, token, label_owned) = (bootstrap.address.clone(), token.clone(), label.to_string());
    let stream = crate::runtime::run(async move { crate::link::handshake_speaking(&address, &token, &label_owned, protocol).await })
        .map_err(io::Error::other)?
        .map_err(|e| io::Error::other(e.to_string()))?;
    lock_down(bootstrap, elevated)?;
    Ok(stream)
}

/// Nothing has been read from the host yet but the handshake: lock down before the first request.
fn lock_down(bootstrap: &Bootstrap, elevated: bool) -> io::Result<()> {
    let notes = crate::sandbox::lockdown(&bootstrap.sandbox, elevated).map_err(|e| io::Error::other(format!("cannot lock down: {e}")))?;
    // On stderr, so the child's log has them (a gap as a Warning): never less sandboxed without saying so.
    for n in notes {
        eprintln!("{}", n.log_line());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_peer() -> Peer {
        let (x, _y) = tokio::io::duplex(1 << 16);
        let rt = crate::runtime::shared().unwrap();
        Peer::start(x, Arc::new(crate::peer::NoCalls), PeerOptions::new("test"), rt.handle())
    }

    #[test]
    fn the_lifeline_closes_the_peer_and_leaves_the_exit_to_main_once_the_phase_begins() {
        let peer = a_peer();
        let phase = Arc::new(Phase::new());
        let t = {
            let (peer, phase) = (peer.clone(), phase.clone());
            std::thread::spawn(move || {
                let mut said = Vec::new();
                lifeline_end(&peer, &phase, LIFELINE_WAIT, &mut said, || panic!("took"));
                said
            })
        };
        // `main` sees the connection close and begins the phase.
        assert!(peer.wait_closed(Some(Duration::from_secs(30))));
        assert!(phase.claim_main());
        assert!(t.join().unwrap().is_empty(), "the lifeline left the exit to main and said nothing");
    }

    #[test]
    fn the_lifeline_says_why_and_ends_when_main_never_claims_the_exit() {
        let peer = a_peer();
        let (mut said, mut ended) = (Vec::new(), false);
        let wait = Duration::from_millis(30);
        lifeline_end(&peer, &Phase::new(), wait, &mut said, || ended = true);
        assert!(ended && String::from_utf8(said).unwrap().contains("did not begin"));
    }

    #[test]
    fn a_main_that_lost_the_claim_goes_through_begin_exit() {
        let phase = Phase::new();
        assert!(phase.claim_fallback(Duration::ZERO));
        let mut ended = false;
        begin_exit(&phase, Duration::from_millis(30), || ended = true);
        assert!(ended, "a main that lost the claim ends the process itself");
        let mut kept = false;
        begin_exit(&Phase::new(), Duration::from_millis(30), || kept = true);
        assert!(!kept, "a main that wins the claim does not");
    }

    struct Broken;
    impl io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct Panicking;
    impl io::Write for Panicking {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            panic!("a writer that panics")
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_fallback_ends_the_process_whatever_its_message_does() {
        let mut said = Vec::new();
        let mut ended = 0;
        end_after_fallback(&mut said, || ended += 1);
        assert_eq!((ended, String::from_utf8(said).unwrap().contains("did not begin")), (1, true));
        // The host is gone: a broken pipe, and a writer that panics, still end it.
        end_after_fallback(&mut Broken, || ended += 1);
        end_after_fallback(&mut Panicking, || ended += 1);
        assert_eq!(ended, 3);
        io::Write::flush(&mut Broken).unwrap();
        io::Write::flush(&mut Panicking).unwrap();
    }

    #[test]
    fn a_main_that_lost_the_claim_ends_the_process_within_the_bound() {
        // On a thread, so a `main` that parked for good fails this test instead of hanging it.
        let (done, ended) = std::sync::mpsc::channel();
        std::thread::spawn(move || main_lost_claim(Duration::from_millis(30), || done.send(()).unwrap()));
        assert!(
            ended.recv_timeout(Duration::from_secs(30)).is_ok(),
            "the late main did not end the process"
        );
    }

    #[test]
    fn the_lifeline_ends_the_process_itself_when_the_phase_never_begins() {
        let peer = a_peer();
        let phase = Phase::new();
        let mut ended = false;
        close_then_end(&peer, &phase, Duration::from_millis(50), || ended = true);
        assert!(peer.is_closed(), "it closed the peer first");
        assert!(ended, "after the bound it falls back to ending the process");
        // A `main` that comes late finds the exit taken: it must park, never run a second exit.
        assert!(!phase.claim_main(), "the lifeline owns the exit now");
    }

    #[cfg(unix)]
    #[test]
    fn wait_until_meets_its_condition_or_gives_up_at_the_bound() {
        let mut looks = 0;
        assert!(wait_until(
            || {
                looks += 1;
                looks == 3
            },
            Duration::from_secs(30)
        ));
        assert_eq!(looks, 3);
        assert!(!wait_until(|| false, Duration::from_millis(20)));
        // A tid that never existed, or none at all, is already gone.
        assert!(wait_task_gone(0, Duration::from_secs(30)));
        assert!(wait_task_gone(u32::MAX, Duration::from_secs(30)));
    }

    /// A thread that is still running stays listed: the wait gives up at the bound and says so.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_live_thread_is_not_waited_for_beyond_the_bound() {
        let (tid_tx, tid_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let t = std::thread::spawn(move || {
            tid_tx.send(current_tid()).unwrap();
            let _ = stop_rx.recv();
        });
        let tid = tid_rx.recv().unwrap();
        assert!(!wait_task_gone(tid, Duration::from_millis(20)));
        stop_tx.send(()).unwrap();
        t.join().unwrap();
    }

    /// The race behind a lockdown that reported "1 thread(s) started before the lockdown": a joined thread still
    /// listed in `/proc/self/task` (about one run in five on a loaded Linux machine).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_joined_thread_leaves_the_task_list_once_waited_for() {
        for i in 0..300 {
            let tid = std::thread::spawn(current_tid).join().unwrap();
            assert_ne!(tid, 0);
            assert!(wait_task_gone(tid, TASK_GONE_BOUND));
            assert!(
                !std::path::Path::new(&format!("/proc/self/task/{tid}")).exists(),
                "round {i}: thread {tid} is still listed"
            );
        }
    }

    #[test]
    fn the_bootstrap_comes_from_stdin_or_a_named_file() {
        let b = Bootstrap {
            address: "a".into(),
            token: "t".into(),
            sandbox: Default::default(),
            max_frame: 1024,
            role: String::new(),
        };
        let line = format!("{}\nrest", serde_json::to_string(&b).unwrap());
        assert_eq!(read_bootstrap(line.as_bytes()).unwrap(), b);
        assert_eq!(read_bootstrap(&b"nope\n"[..]).unwrap_err().kind(), io::ErrorKind::InvalidData);
        let long = format!("{{\"address\":\"{}\"}}\n", "a".repeat(70_000));
        assert!(read_bootstrap(long.as_bytes()).is_err());
        let args: Vec<String> = ["x", "--plugin", "p", BOOTSTRAP_FLAG, "/tmp/b.json"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(bootstrap_file(&args), Some("/tmp/b.json".into()));
        assert_eq!(bootstrap_file(&args[..4]), None);
        assert_eq!(bootstrap_file(&[]), None);
    }
}
