//! The one child loop (ADR 0017 §2, §5): a child process' `main` calls [`child_main`], which
//!
//! 1. reads its [`Bootstrap`] (one JSON line on stdin, or the file `--debrief-bootstrap <file>` names for an elevated
//!    child, which has no stdin pipe);
//! 2. connects to the host and authenticates;
//! 3. locks itself down ([`crate::sandbox::lockdown`]) before it handles anything;
//! 4. answers the host's calls with its handler until the connection closes, then returns (the process exits).
//!
//! A child also exits when its stdin closes (the host died); nothing is left behind.

use crate::elevation::{take_bootstrap, BOOTSTRAP_FLAG};
use crate::frame::ProtocolVersion;
use crate::launch::Bootstrap;
use crate::link::Token;
use crate::peer::{Handler, Peer, PeerOptions};
use std::io::{self, BufRead, Read};
use std::sync::Arc;

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
    child_main_hooked(label, protocol, make, connected, || {}).map(drop)
}

/// How a child's connection ended when it was not the host closing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broke(pub String);

/// [`child_main_then`] that also runs `stdin_closed` on the lifeline's thread when the host closed stdin or died, just before the
/// process exits (a child that keeps a log writes its exit line there: `exit` runs no destructors). `Ok(None)`: the host closed
/// the connection; `Ok(Some(Broke))`: it broke (an I/O error, a protocol error) with the reason.
pub fn child_main_hooked(
    label: &str,
    protocol: ProtocolVersion,
    make: impl FnOnce(&Bootstrap) -> Arc<dyn Handler>,
    connected: impl FnOnce(&Bootstrap, &Peer),
    stdin_closed: impl FnOnce() + Send + 'static,
) -> io::Result<Option<Broke>> {
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
        lifeline_then(stdin_closed);
    }
    peer.wait_closed(None);
    Ok(peer.closed().and_then(|why| broke(&why)))
}

/// Whether `why` is a break (not the host closing the connection): the reason text of a break says "broke".
fn broke(why: &crate::IpcError) -> Option<Broke> {
    let text = why.to_string();
    text.contains("broke").then_some(Broke(text))
}

/// The lifeline: when the host closes stdin (or dies), exit at once.
pub fn lifeline() {
    lifeline_then(|| {});
}

/// [`lifeline`] that runs `on_closed` before the exit.
pub fn lifeline_then(on_closed: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        let mut sink = [0u8; 256];
        while matches!(io::stdin().read(&mut sink), Ok(n) if n > 0) {}
        on_closed();
        // `exit` runs no destructors: remove the endpoints of children this process was still starting.
        crate::endpoint::remove_live();
        std::process::exit(0);
    });
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
    let stream = std::thread::scope(|s| {
        s.spawn(|| -> io::Result<std::os::unix::net::UnixStream> {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
            let stream = rt
                .block_on(crate::link::platform_handshake(&bootstrap.address, token, label, protocol))
                .map_err(|e| io::Error::other(e.to_string()))?;
            stream.into_std()
        })
        .join()
        .map_err(|_| io::Error::other("the handshake thread panicked"))?
    })?;
    lock_down(bootstrap, elevated)?;
    // `from_std` registers with the reactor of the runtime entered: the shared one, started now.
    let _g = crate::runtime::shared().map_err(io::Error::other)?.enter();
    Ok(Box::new(tokio::net::UnixStream::from_std(stream)?))
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

    #[test]
    fn a_break_of_the_connection_is_told_from_the_host_closing_it() {
        use crate::IpcError;
        let broken = broke(&IpcError::Died("the connection to Debrief broke (os error 32)".into()));
        assert!(broken.is_some_and(|b| b.0.contains("os error")));
        assert_eq!(broke(&IpcError::Died("Debrief closed the connection".into())), None);
        assert_eq!(broke(&IpcError::Died("the connection to Debrief was closed".into())), None);
    }

    /// The lifeline runs its hook before the process exits: the test starts itself as the child (`exit` ends a process).
    #[test]
    #[allow(clippy::disallowed_methods)] // a test starts itself as the child; no console window to avoid
    fn the_lifeline_runs_its_hook_when_stdin_closes_and_then_ends_the_process() {
        const FILE: &str = "DEBRIEF_TEST_LIFELINE_FILE";
        if let Some(file) = std::env::var_os(FILE) {
            lifeline_then(move || std::fs::write(&file, "closed").unwrap());
            // The lifeline ends this process; the wait is only a bound.
            std::thread::sleep(std::time::Duration::from_secs(60));
            std::process::exit(3);
        }
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("hook-ran");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "child::tests::the_lifeline_runs_its_hook_when_stdin_closes_and_then_ends_the_process",
                "--test-threads=1",
            ])
            .env(FILE, &marker)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        drop(child.stdin.take());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let status = loop {
            if let Some(s) = child.try_wait().unwrap() {
                break s;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the child did not end when its stdin closed");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(status.code(), Some(0), "the lifeline exits with 0");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "closed", "the hook ran before the exit");
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
