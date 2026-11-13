//! The `./dev` wrapper's prebuilt download (ADR 0022 R0b): a hit, a 404, a bad checksum, a binary for another hash and a server that
//! never answers, against a small HTTP server in the test. The wrapper never trusts what it fetched and always falls back to a build.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// windows-twin: none: it runs the ./dev shell wrapper, which Windows runs through Git bash on every job
#![cfg(unix)]
use super::wrapper::{Fixture, fake_rustc, shim_cargo, write_script};
use paddock_testkit::TestCommand;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// What the server does for one path.
#[derive(Clone)]
enum Reply {
    Body(Vec<u8>),
    /// Accepts the connection and never answers.
    Hang,
    /// Answers with this status line (for example `401 Unauthorized`) and no body.
    Status(&'static str),
}

/// A one-thread HTTP server: serves the routes it was given, 404 for the rest, and records every request's head.
struct Server {
    addr: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    fn start(routes: Vec<(String, Reply)>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (req, st) = (requests.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let mut hung: Vec<TcpStream> = Vec::new();
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
                req.lock().unwrap().push(head);
                match routes.iter().find(|(p, _)| *p == path).map(|(_, r)| r.clone()) {
                    Some(Reply::Body(body)) => {
                        let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        let _ = c.write_all(&body);
                    }
                    Some(Reply::Hang) => hung.push(c),
                    Some(Reply::Status(line)) => {
                        let _ = write!(c, "HTTP/1.1 {line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                    None => {
                        let _ = write!(c, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                }
            }
        });
        Server {
            addr,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn base(&self) -> String {
        format!("http://{}/generic", self.addr)
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(&self.addr);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The text of a stand-in tool that says it was downloaded and claims `hash` as the one it was built for.
fn published_tool(hash: &str) -> Vec<u8> {
    format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 0; exit 0; fi\nif [ \"$1\" = self ]; then echo \"embedded {hash}\"; exit 0; fi\necho downloaded-binary\n")
        .into_bytes()
}

fn sha256_of(bytes: &[u8], scratch: &Path) -> String {
    let p = scratch.join("sum-input");
    std::fs::write(&p, bytes).unwrap();
    let out = TestCommand::new("sh")
        .args(["-c", "(sha256sum \"$1\" 2>/dev/null || shasum -a 256 \"$1\") | cut -d' ' -f1"])
        .arg("x")
        .arg(&p)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A fixture, the fake compiler, the hash for it and a scratch folder.
struct Setup {
    f: Fixture,
    scratch: tempfile::TempDir,
    rustc: String,
    hash: String,
}

fn setup() -> Setup {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let (hash, _) = f.both(&[("RUSTC", rustc.clone())]);
    Setup { f, scratch, rustc, hash }
}

impl Setup {
    fn route(&self, file: &str) -> String {
        format!("/generic/paddock-dev/{}/{file}", self.hash)
    }

    /// The two routes of a published tool: the file and its checksum line.
    fn published(&self, body: &[u8], sha: &str) -> Vec<(String, Reply)> {
        vec![
            (self.route("paddock-dev-test-host"), Reply::Body(body.to_vec())),
            (
                self.route("paddock-dev-test-host.sha256"),
                Reply::Body(format!("{sha}  paddock-dev-test-host\n").into_bytes()),
            ),
        ]
    }

    /// Runs `./dev hello` with a stand-in cargo. `env` is added to a base that downloads from `server`.
    fn run(&self, server: &Server, env: &[(&str, &str)]) -> (i32, String, String) {
        let shims = self.f.cargo_home.path().join("shims");
        write_script(&shims.join("cargo"), &shim_cargo(0));
        let path = format!("{}:{}", shims.display(), std::env::var("PATH").unwrap());
        let mut cmd = TestCommand::new("sh");
        cmd.arg(self.f.root().join("dev"))
            .arg("hello")
            .current_dir(self.f.root())
            .env("PATH", path)
            .env("PADDOCK_DEV_HOME", self.scratch.path().join("home"))
            .env("PADDOCK_DEV_TARGET_DIR", self.f.cargo_home.path().join("target"))
            .env("CARGO_HOME", self.f.cargo_home.path())
            .env("RUSTC", &self.rustc)
            .env("PADDOCK_DEV_PACKAGES", server.base())
            .env_remove("GITHUB_ACTIONS")
            .env_remove("GITHUB_TOKEN")
            .env_remove("PADDOCK_DEV_TOKEN")
            .env_remove("PADDOCK_DEV_DOWNLOAD");
        cmd.envs(env.iter().copied());
        let out = cmd.output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn built(&self) -> bool {
        self.f.cargo_home.path().join("target").exists()
    }

    fn cached(&self) -> std::path::PathBuf {
        self.scratch.path().join("home/bin").join(&self.hash).join("paddock-dev-test-host")
    }

    /// No half-written download is left next to the cached tool.
    fn assert_clean(&self) {
        let dir = self.cached().parent().unwrap().to_path_buf();
        let names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["paddock-dev-test-host"], "leftovers: {names:?}");
    }
}

#[test]
fn a_published_tool_is_downloaded_checked_cached_and_run_without_a_build() {
    let s = setup();
    let body = published_tool(&s.hash);
    let sha = sha256_of(&body, s.scratch.path());
    let server = Server::start(s.published(&body, &sha));
    let (code, out, err) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "1"), ("PADDOCK_DEV_TOKEN", "sekrit-token")]);
    assert_eq!((code, out.trim()), (0, "downloaded-binary"), "{err}");
    assert!(!s.built(), "a hit must not build: {err}");
    assert!(
        err.contains("downloaded paddock-dev") && err.contains(&s.hash) && err.contains("transport checks passed"),
        "phases are logged: {err}"
    );
    s.assert_clean();
    // The auth header came from the environment and reached the server, and nothing printed it.
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.to_lowercase().contains("authorization: token sekrit-token")),
        "{:?}",
        server.requests()
    );
    assert!(!err.contains("sekrit-token") && !out.contains("sekrit-token"));
    // The next run is a cache hit: no request at all.
    let before = server.requests().len();
    let (code, out, _) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "1")]);
    assert_eq!((code, out.trim()), (0, "downloaded-binary"));
    assert_eq!(server.requests().len(), before);
}

#[test]
fn a_tool_that_is_not_published_falls_back_to_a_build_with_a_notice() {
    let s = setup();
    let server = Server::start(vec![]);
    let (code, out, err) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "1")]);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(
        err.contains("::notice::") && err.contains("not published") && err.contains("404"),
        "{err}"
    );
    // A miss can be explained from the log: the compiler, the hashed variable names and the config digests (no values).
    assert!(err.contains("inputs: ") && err.contains("release: 1.90.0"), "{err}");
    assert!(err.contains("| env: ") && err.contains("| configs: "), "{err}");
    assert!(s.built());
    s.assert_clean();
}

#[test]
fn a_download_whose_checksum_differs_is_refused_and_built_instead() {
    let s = setup();
    let body = published_tool(&s.hash);
    let server = Server::start(s.published(&body, &"0".repeat(64)));
    let (code, out, err) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "1")]);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(err.contains("::warning::") && err.contains("checksum"), "{err}");
    s.assert_clean();
}

#[test]
fn a_download_built_for_another_hash_is_refused_and_built_instead() {
    let s = setup();
    let body = published_tool(&"f".repeat(40));
    let sha = sha256_of(&body, s.scratch.path());
    let server = Server::start(s.published(&body, &sha));
    let (code, out, err) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "1")]);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(err.contains("::warning::") && err.contains("not for hash"), "{err}");
    s.assert_clean();
}

#[test]
fn a_server_that_never_answers_ends_at_the_timeout_and_the_tool_is_built() {
    let s = setup();
    let server = Server::start(vec![(s.route("paddock-dev-test-host.sha256"), Reply::Hang)]);
    let (code, out, err) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "1"), ("PADDOCK_DEV_DOWNLOAD_TIMEOUT", "1")]);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(err.contains("::warning::") && err.contains("could not download"), "{err}");
    s.assert_clean();
}

#[test]
fn locally_nothing_is_downloaded_unless_asked_and_in_ci_it_is_by_default() {
    let s = setup();
    let body = published_tool(&s.hash);
    let sha = sha256_of(&body, s.scratch.path());
    let server = Server::start(s.published(&body, &sha));
    let (code, out, err) = s.run(&server, &[]);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(
        server.requests().is_empty(),
        "a developer's machine builds from source: {:?}",
        server.requests()
    );
    // In CI the job token is the read credential when no PADDOCK_DEV_TOKEN is set.
    let s = setup();
    let server = Server::start(s.published(&body, &sha));
    let mount = s.scratch.path().join("missing");
    let (code, out, err) = s.run(
        &server,
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_TOKEN", "job-token"),
            ("PADDOCK_CI_MOUNT", mount.to_str().unwrap()),
        ],
    );
    assert_eq!((code, out.trim()), (0, "downloaded-binary"), "{err}");
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.to_lowercase().contains("authorization: token job-token"))
    );
    assert!(!err.contains("job-token"));
}

#[test]
fn a_release_style_fresh_build_never_downloads() {
    let s = setup();
    let body = published_tool(&s.hash);
    let sha = sha256_of(&body, s.scratch.path());
    let server = Server::start(s.published(&body, &sha));
    let (code, out, err) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "1"), ("PADDOCK_DEV_FRESH", "1")]);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(server.requests().is_empty());
}

#[test]
fn a_refused_read_says_whether_a_token_was_sent_and_the_tool_is_built() {
    let s = setup();
    let server = Server::start(vec![(s.route("paddock-dev-test-host.sha256"), Reply::Status("401 Unauthorized"))]);
    let (code, out, err) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "1")]);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(
        err.contains("::warning::") && err.contains("HTTP 401") && err.contains("no read token is set"),
        "{err}"
    );
    let s = setup();
    let server = Server::start(vec![(s.route("paddock-dev-test-host.sha256"), Reply::Status("401 Unauthorized"))]);
    let (code, _, err) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "1"), ("PADDOCK_DEV_TOKEN", "t0k")]);
    assert_eq!(code, 0, "{err}");
    assert!(
        err.contains("HTTP 401") && err.contains("a read token was sent") && !err.contains("t0k"),
        "{err}"
    );
}

#[test]
fn a_download_never_touches_the_source_only_cache() {
    let s = setup();
    let body = published_tool(&s.hash);
    let sha = sha256_of(&body, s.scratch.path());
    let server = Server::start(s.published(&body, &sha));
    let (code, out, err) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "1")]);
    assert_eq!((code, out.trim()), (0, "downloaded-binary"), "{err}");
    assert!(s.cached().exists());
    assert!(
        !s.scratch.path().join("home/bin-source").exists(),
        "release runs' cache is written by builds only"
    );
}

#[test]
fn a_run_that_never_downloads_builds_into_its_own_cache_whatever_bin_holds() {
    let s = setup();
    // An earlier run (or a developer) left a downloaded tool for this very hash in the shared cache.
    write_script(&s.cached(), "echo downloaded-binary");
    let server = Server::start(vec![]);
    let (code, out, err) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "0")]);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    let source = s.scratch.path().join("home/bin-source").join(&s.hash).join("paddock-dev-test-host");
    assert!(source.exists(), "built into bin-source: {err}");
    assert!(server.requests().is_empty());
    // The shared cache is untouched, and the source cache serves the next release run without a build.
    let kept = std::fs::read_to_string(s.cached()).unwrap();
    assert!(kept.contains("downloaded-binary"));
    std::fs::remove_dir_all(s.f.cargo_home.path().join("target")).unwrap();
    let (code, out, _) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "0")]);
    assert_eq!((code, out.trim()), (0, "built-by-shim"));
    assert!(!s.built(), "the second release run found its own build");
}

#[test]
fn a_server_error_and_a_binary_that_404s_while_its_checksum_exists_both_fall_back_to_a_build() {
    let s = setup();
    let server = Server::start(vec![(
        s.route("paddock-dev-test-host.sha256"),
        Reply::Status("503 Service Unavailable"),
    )]);
    let (code, out, err) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "1")]);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(err.contains("::warning::") && err.contains("HTTP 503"), "{err}");
    s.assert_clean();

    let s = setup();
    let body = published_tool(&s.hash);
    let sha = sha256_of(&body, s.scratch.path());
    let only_sum: Vec<_> = s
        .published(&body, &sha)
        .into_iter()
        .filter(|(p, _)| p.ends_with(".sha256"))
        .collect();
    let server = Server::start(only_sum);
    let (code, out, err) = s.run(&server, &[("PADDOCK_DEV_DOWNLOAD", "1")]);
    assert_eq!((code, out.trim()), (0, "built-by-shim"), "{err}");
    assert!(
        err.contains("::notice::") && err.contains("HTTP 404 for paddock-dev-test-host)"),
        "{err}"
    );
    s.assert_clean();
}

#[test]
fn a_shell_trace_of_a_download_never_prints_the_token() {
    let s = setup();
    let server = Server::start(vec![(s.route("paddock-dev-test-host.sha256"), Reply::Status("401 Unauthorized"))]);
    let shims = s.f.cargo_home.path().join("shims");
    write_script(&shims.join("cargo"), &shim_cargo(0));
    let path = format!("{}:{}", shims.display(), std::env::var("PATH").unwrap());
    let out = TestCommand::new("sh")
        .arg("-x")
        .arg(s.f.root().join("dev"))
        .arg("hello")
        .current_dir(s.f.root())
        .env("PATH", path)
        .env("PADDOCK_DEV_HOME", s.scratch.path().join("home"))
        .env("PADDOCK_DEV_TARGET_DIR", s.f.cargo_home.path().join("target"))
        .env("CARGO_HOME", s.f.cargo_home.path())
        .env("RUSTC", &s.rustc)
        .env("PADDOCK_DEV_PACKAGES", server.base())
        .env("PADDOCK_DEV_DOWNLOAD", "1")
        .env("PADDOCK_DEV_TOKEN", "trace-secret-token-9876")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(err.contains("HTTP 401") && err.contains("a read token was sent"), "{err}");
    assert!(!err.contains("trace-secret-token-9876"), "the token reached the trace");
}
