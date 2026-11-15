//! The `--smoke` part of `verify-bundle`: starts the bundled `pitwall-server` on a free port with `PITWALL_HOME` in a throw-away
//! folder, waits for its `discovery.json`, calls the management API with and without the admin key, starts the app with `--hidden`
//! (tray only, no window, so nothing opens on screen), keeps it a few seconds, checks it stays alive and that quitting it leaves the
//! server running, then ends both. Every process it started is ended as a tree by identity (rule 53), whatever happens.

use crate::bundle::Lines;
use paddock_proc::{command, kill_tree, ProcessId};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

/// How long the app must stay up.
const APP_UP: Duration = Duration::from_secs(6);
/// How long the server has to write its discovery file.
const DISCOVERY_WAIT: Duration = Duration::from_secs(10);

/// The status code of `POST <path>` to `127.0.0.1:<port>`, with a bearer key when given.
pub fn post_status(port: u16, path: &str, key: Option<&str>) -> Result<u16, String> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let t = Duration::from_secs(5);
    let mut c = std::net::TcpStream::connect_timeout(&addr, t).map_err(|e| e.to_string())?;
    c.set_read_timeout(Some(t)).map_err(|e| e.to_string())?;
    c.set_write_timeout(Some(t)).map_err(|e| e.to_string())?;
    let auth = key.map_or(String::new(), |k| format!("Authorization: Bearer {k}\r\n"));
    let req = format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Content-Length: 0\r\nConnection: close\r\n\r\n");
    c.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut head = [0u8; 64];
    let n = c.read(&mut head).map_err(|e| e.to_string())?;
    String::from_utf8_lossy(&head[..n])
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| "no HTTP status line".to_string())
}

/// A free local port: bound to 0, read, released.
pub fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| format!("no free port: {e}"))
}

/// The last three lines of a file, joined by spaces (what the script put after "exited early").
pub fn tail3(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(3)..].join(" ")
}

/// A started process that is ended as a tree when it goes out of scope.
struct Started {
    child: Option<Child>,
}

impl Started {
    fn spawn(program: &Path, args: &[&str], home: &Path, log: &Path) -> Result<Started, String> {
        let out = std::fs::File::create(log).map_err(|e| format!("cannot create {}: {e}", log.display()))?;
        let err = out.try_clone().map_err(|e| e.to_string())?;
        let child = command(program)
            .args(args)
            .env("PITWALL_HOME", home)
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", program.display()))?;
        Ok(Started { child: Some(child) })
    }

    fn alive(&mut self) -> bool {
        self.child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    /// Ends the tree and reaps the child.
    fn end(&mut self) {
        if let Some(mut c) = self.child.take() {
            if let Some(id) = ProcessId::of(c.id()) {
                let _ = kill_tree(id);
            }
            let _ = c.wait();
        }
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        self.end();
    }
}

fn scratch() -> Result<PathBuf, String> {
    let base = ["TMPDIR", "TEMP", "TMP"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .ok_or("no temp folder is set (TMPDIR); set it to a scratch folder")?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let dir = Path::new(&base).join(format!("pitwall-smoke-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// The smoke run; its lines go into `l`.
pub fn run(server: &Path, app: &Path, l: &mut Lines) {
    l.text.push_str("\nSmoke test (throw-away data folder, hidden app)\n");
    let home = match scratch() {
        Ok(h) => h,
        Err(e) => return l.bad("smoke test", &e),
    };
    smoke(server, app, &home, l);
    let _ = std::fs::remove_dir_all(&home);
}

fn smoke(server: &Path, app: &Path, home: &Path, l: &mut Lines) {
    let port = match free_port() {
        Ok(p) => p,
        Err(e) => return l.bad("smoke test", &e),
    };
    let server_log = home.join("server.log");
    let mut srv = match Started::spawn(
        server,
        &["--port", &port.to_string(), "--data-dir", &home.to_string_lossy()],
        home,
        &server_log,
    ) {
        Ok(s) => s,
        Err(e) => return l.bad("bundled server", &e),
    };
    let discovery = home.join("discovery.json");
    let t0 = Instant::now();
    while !discovery.is_file() && t0.elapsed() < DISCOVERY_WAIT && srv.alive() {
        std::thread::sleep(Duration::from_millis(100));
    }
    if !discovery.is_file() {
        let tail = std::fs::read_to_string(&server_log).map(|t| tail3(&t)).unwrap_or_default();
        return l.bad("bundled server", &format!("no discovery.json after 10 s: {tail}"));
    }
    l.ok("bundled server wrote discovery.json");
    let key = std::fs::read_to_string(home.join("admin.key")).unwrap_or_default();
    let overview = "/mgmt/v1/rpc/overview";
    match post_status(port, overview, Some(key.trim())) {
        Ok(200) => l.ok("management API answers overview (200)"),
        other => l.bad("management API", &format!("overview answered {}", describe(&other))),
    }
    match post_status(port, overview, None) {
        Ok(401) => l.ok("management API refuses a call without the admin key (401)"),
        other => l.bad(
            "management API auth",
            &format!("a call without a key answered {}", describe(&other)),
        ),
    }
    let app_log = home.join("app.log");
    match Started::spawn(app, &["--hidden"], home, &app_log) {
        Err(e) => l.bad("app", &e),
        Ok(mut a) => {
            std::thread::sleep(APP_UP);
            if a.alive() {
                let pid = a.child.as_ref().map_or(0, Child::id);
                l.ok(&format!("the app started hidden and stayed up for 6 s (pid {pid})"));
            } else {
                let tail = std::fs::read_to_string(&app_log).map(|t| tail3(&t)).unwrap_or_default();
                l.bad("app", &format!("exited early; see: {tail}"));
            }
            a.end();
            if srv.alive() {
                l.ok("quitting the app leaves the server running");
            } else {
                l.bad("server", "died with the app");
            }
        }
    }
    srv.end();
}

fn describe(r: &Result<u16, String>) -> String {
    match r {
        Ok(c) => c.to_string(),
        Err(e) => format!("nothing ({e})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn the_status_is_read_and_the_key_is_sent_only_when_given() {
        for (key, want) in [(Some("k"), "Authorization: Bearer k\r\n"), (None, "")] {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            let h = std::thread::spawn(move || {
                let (mut c, _) = l.accept().unwrap();
                let mut b = [0u8; 512];
                let n = c.read(&mut b).unwrap();
                c.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n").unwrap();
                String::from_utf8_lossy(&b[..n]).into_owned()
            });
            assert_eq!(post_status(port, "/x", key), Ok(401));
            let req = h.join().unwrap();
            assert!(
                req.starts_with("POST /x HTTP/1.1") && (want.is_empty() != req.contains("Authorization")),
                "{req}"
            );
            assert!(want.is_empty() || req.contains(want));
        }
    }

    #[test]
    fn a_closed_port_is_an_error_not_a_status() {
        let port = free_port().unwrap();
        assert!(post_status(port, "/x", None).is_err());
    }

    #[test]
    fn the_log_tail_is_the_last_three_lines() {
        assert_eq!(tail3("a\nb\nc\nd\n"), "b c d");
        assert_eq!(tail3("a"), "a");
        assert_eq!(tail3(""), "");
    }

    /// A stand-in program that stays and leaves a child behind: started, seen alive, ended with its child, and its pids are gone.
    /// The test waits for the stand-in's own "ready" connection, not for time.
    #[cfg(unix)]
    #[test]
    fn a_started_process_is_ended_as_a_tree_and_nothing_is_left() {
        let d = tempfile::tempdir().unwrap();
        let ready = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = ready.local_addr().unwrap().port();
        let script = d.path().join("stay");
        let pids = d.path().join("pids");
        let body = format!(
            "#!/bin/bash\necho $$ > '{p}'\nsleep 300 &\necho $! >> '{p}'\necho ok > /dev/tcp/127.0.0.1/{port}\nwait\n",
            p = pids.display()
        );
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let mut s = Started::spawn(&script, &[], d.path(), &d.path().join("log")).unwrap();
        let (mut c, _) = ready.accept().unwrap();
        let mut buf = [0u8; 8];
        let _ = c.read(&mut buf).unwrap();
        assert!(s.alive());
        let listed = std::fs::read_to_string(&pids).unwrap();
        // Identities taken while both processes run: after the end a reused pid cannot be mistaken for them (rule 53).
        let ids: Vec<ProcessId> = listed
            .split_whitespace()
            .map(|p| ProcessId::of(p.parse().unwrap()).unwrap())
            .collect();
        assert_eq!(ids.len(), 2, "{listed}");
        s.end();
        assert!(!s.alive());
        // A killed process may still be mid-exit for a moment: wait on the condition (bounded), not on time.
        let t0 = Instant::now();
        for id in ids {
            while !id.has_ended() && t0.elapsed() < Duration::from_secs(10) {
                std::thread::yield_now();
            }
            assert!(id.has_ended(), "process {} is still running", id.pid());
        }
    }
}
