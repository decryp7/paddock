//! `dev heavy-install` against a fake release server (a std listener serving files): a good checksum installs atomically and records
//! the version, a bad checksum is refused with an `::error::` line and installs nothing, a rerun of the same version downloads nothing,
//! and the token reaches the server as a header and never the output.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// windows-twin: none: it serves the release over curl on a loopback port and checks Unix file modes
#![cfg(unix)]
use paddock_dev::heavy_install::{Done, Install, asset_name, install};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Serves the files of a release: GET of a known path answers the bytes, anything else 404; records every request line and the
/// Authorization header it saw.
/// A request line's path and its Authorization header.
type Seen = Vec<(String, Option<String>)>;

struct Server {
    base: String,
    requests: Arc<Mutex<Seen>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn start(files: BTreeMap<String, Vec<u8>>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (rq, st) = (requests.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            for conn in listener.incoming() {
                if st.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut c) = conn else { continue };
                let mut head = Vec::new();
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") && c.read(&mut b).map(|n| n == 1).unwrap_or(false) {
                    head.push(b[0]);
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                let auth = head.lines().find_map(|l| {
                    l.to_lowercase()
                        .starts_with("authorization:")
                        .then(|| l.split_once(':').unwrap().1.trim().to_string())
                });
                rq.lock().unwrap().push((path.clone(), auth));
                let (reply, body) = files.get(&path).map_or(("404 Not Found", Vec::new()), |b| ("200 OK", b.clone()));
                let _ = write!(c, "HTTP/1.1 {reply}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = c.write_all(&body);
            }
        });
        Server {
            base: format!("http://{addr}"),
            requests,
            stop,
            thread: Some(thread),
        }
    }
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(self.base.trim_start_matches("http://"));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

const VERSION: &str = "9.9.9";
const BINARY: &[u8] = b"#!/bin/sh\necho heavy-test\n";

fn hex(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

fn release(sum: &str) -> BTreeMap<String, Vec<u8>> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let asset = asset_name(VERSION, os, arch).expect("this machine has a release asset");
    BTreeMap::from([
        (format!("/heavy-v{VERSION}/{asset}"), BINARY.to_vec()),
        (format!("/heavy-v{VERSION}/SHA256SUMS"), format!("{sum}  {asset}\n").into_bytes()),
    ])
}

fn run(dir: &std::path::Path, base: &str, token: Option<&str>) -> Result<Done, String> {
    let env = HashMap::from([("PATH".to_string(), std::env::var("PATH").unwrap_or_default())]);
    install(&Install {
        dir,
        version: VERSION,
        base,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        token,
        env: &env,
    })
}

#[test]
fn a_good_checksum_installs_atomically_with_the_version_recorded_and_the_token_only_as_a_header() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("heavy");
    let server = Server::start(release(&hex(BINARY)));
    let done = run(&dir, &server.base, Some("s3cret-token")).unwrap();
    let bin = dir.join("bin/heavy");
    assert_eq!(done, Done::Installed(bin.clone()));
    assert_eq!(std::fs::read(&bin).unwrap(), BINARY);
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&bin).unwrap().permissions().mode() & 0o111, 0o111, "executable");
    }
    let record = std::fs::read_to_string(dir.join("VERSION")).unwrap();
    assert!(record.starts_with(&format!("{VERSION}\n{}\n", hex(BINARY))), "{record}");
    // No download folder and no half-written file is left.
    let left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left.len(), 2, "{left:?}");
    assert!(std::fs::read_dir(dir.join("bin")).unwrap().count() == 1);
    // The token was sent as the Authorization header of both requests, and is in no file the install wrote.
    let seen = server.requests.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(seen.iter().all(|(_, a)| a.as_deref() == Some("token s3cret-token")), "{seen:?}");
    assert!(!record.contains("s3cret-token"));
}

#[test]
fn a_bad_checksum_is_refused_with_an_error_line_and_installs_nothing() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("heavy");
    let server = Server::start(release(&hex(b"something else")));
    let err = run(&dir, &server.base, None).unwrap_err();
    assert!(err.starts_with("::error::heavy-install: the downloaded heavy-9.9.9-"), "{err}");
    assert!(
        err.contains("SHA256SUMS says") && err.contains("it is not installed") && err.contains("Next step"),
        "{err}"
    );
    assert!(!dir.join("bin/heavy").exists(), "nothing installed");
    assert!(!dir.join("VERSION").exists(), "nothing recorded");
    // A release that does not carry the asset is an error too, not an install.
    let server = Server::start(BTreeMap::from([(
        format!("/heavy-v{VERSION}/SHA256SUMS"),
        b"00  heavy-other\n".to_vec(),
    )]));
    let err = run(&dir, &server.base, None).unwrap_err();
    assert!(err.contains("does not list"), "{err}");
    let server = Server::start(BTreeMap::new());
    let err = run(&dir, &server.base, None).unwrap_err();
    assert!(err.starts_with("::error::heavy-install: cannot download"), "{err}");
}

#[test]
fn a_rerun_of_the_same_version_downloads_nothing_and_a_changed_binary_is_installed_again() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("heavy");
    let server = Server::start(release(&hex(BINARY)));
    run(&dir, &server.base, None).unwrap();
    let first = server.count();
    assert_eq!(
        run(&dir, &server.base, None).unwrap(),
        Done::AlreadyInstalled(dir.join("bin/heavy"))
    );
    assert_eq!(server.count(), first, "no request on the rerun");
    // The installed file was changed under us: it is not the recorded one, so it is installed again.
    std::fs::write(dir.join("bin/heavy"), b"tampered").unwrap();
    assert_eq!(run(&dir, &server.base, None).unwrap(), Done::Installed(dir.join("bin/heavy")));
    assert_eq!(std::fs::read(dir.join("bin/heavy")).unwrap(), BINARY);
}
