//! `conformance/ci/dev-tool-publish.sh` (ADR 0022 R0b) uploads the tool the way Gitea's generic package store accepts it. Gitea answers
//! HTTP 500 "request Content-Type isn't multipart/form-data" to a PUT that curl's `--data-binary` labelled as a form (run 7769).
#![allow(clippy::unwrap_used, clippy::expect_used)]
// windows-twin: none: it runs a shell script that only runs on Linux and macOS
#![cfg(unix)]
use super::wrapper::{Fixture, fake_rustc, shim_cargo, write_script};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A store that behaves like Gitea's generic packages: 404 for what was not uploaded, 500 for a PUT labelled as a form, 409 for a
/// file that exists, else 201; a GET returns the stored bytes. `refuse` makes the PUT of a file with that name ending answer a status.
struct Store {
    addr: String,
    files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    order: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Store {
    fn start(seed: BTreeMap<String, Vec<u8>>, refuse: Option<(&'static str, &'static str)>) -> Store {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let files = Arc::new(Mutex::new(seed));
        let order = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (fs, od, st) = (files.clone(), order.clone(), stop.clone());
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
                let lower = head.to_lowercase();
                let mut first = head.split_whitespace();
                let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
                let len: usize = lower
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0)))
                    .unwrap_or(0);
                let mut body = vec![0u8; len];
                let _ = c.read_exact(&mut body);
                let (reply, out) = if method == "PUT" {
                    if lower.contains("content-type: application/x-www-form-urlencoded") {
                        ("500 Internal Server Error", Vec::new())
                    } else if let Some((_, status)) = refuse.filter(|(suffix, _)| path.ends_with(suffix)) {
                        (status, Vec::new())
                    } else {
                        let mut files = fs.lock().unwrap();
                        match files.entry(path.clone()) {
                            std::collections::btree_map::Entry::Occupied(_) => ("409 Conflict", Vec::new()),
                            std::collections::btree_map::Entry::Vacant(v) => {
                                v.insert(body);
                                od.lock().unwrap().push(path);
                                ("201 Created", Vec::new())
                            }
                        }
                    }
                } else if let Some(stored) = fs.lock().unwrap().get(&path) {
                    ("200 OK", stored.clone())
                } else {
                    ("404 Not Found", Vec::new())
                };
                let _ = write!(c, "HTTP/1.1 {reply}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", out.len());
                let _ = c.write_all(&out);
            }
        });
        Store {
            addr,
            files,
            order,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(&self.addr);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Run {
    out: std::process::Output,
    consumer_hash: String,
}

impl Run {
    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.out.stdout).into_owned()
    }
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.out.stderr).into_owned()
    }
}

/// Runs the publish script (with `bash -x`, so a leaked secret would show in the trace) in a fixture against `store`.
#[expect(
    clippy::disallowed_methods,
    reason = "a test starts the script under test; nothing here ends a process by pid"
)]
fn publish(store: &Store) -> Run {
    let f = Fixture::new();
    let scratch = tempfile::tempdir().unwrap();
    let rustc = fake_rustc(scratch.path(), "rustc", "1.90.0", "aaaa", "test-host");
    let shims = f.cargo_home.path().join("shims");
    write_script(&shims.join("cargo"), &shim_cargo(0));
    std::fs::copy(rustc, shims.join("rustc")).unwrap();
    std::fs::create_dir_all(f.root().join("conformance/ci")).unwrap();
    let script = f.root().join("conformance/ci/dev-tool-publish.sh");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join("conformance/ci/dev-tool-publish.sh"),
        &script,
    )
    .unwrap();
    let (consumer_hash, _) = f.both(&[("RUSTC", shims.join("rustc").display().to_string())]);
    let path = format!("{}:{}", shims.display(), std::env::var("PATH").unwrap());
    let out = Command::new("bash")
        .arg("-x")
        .arg(&script)
        .current_dir(f.root())
        .env("PATH", path)
        .env("CARGO_HOME", f.cargo_home.path())
        .env("PADDOCK_DEV_PACKAGES", format!("http://{}/generic", store.addr))
        .env("PUBLISH_TOKEN", SECRET)
        .env_remove("RUSTC")
        .env_remove("GITHUB_STEP_SUMMARY")
        .output()
        .unwrap();
    Run { out, consumer_hash }
}

const SECRET: &str = "write-token-0123456789";

fn names(keys: &[String]) -> Vec<String> {
    keys.iter().map(|k| k.rsplit('/').next().unwrap().to_string()).collect()
}

#[test]
fn the_tool_is_uploaded_binary_first_then_its_checksum_under_the_consumers_hash_and_no_secret_is_printed() {
    let store = Store::start(BTreeMap::new(), None);
    let r = publish(&store);
    assert!(r.out.status.success(), "{}\n{}", r.stdout(), r.stderr());
    let order = store.order.lock().unwrap().clone();
    assert_eq!(
        names(&order),
        ["paddock-dev-test-host", "paddock-dev-test-host.sha256"],
        "binary first, checksum last"
    );
    // The consumer (the wrapper every CI job runs) asks for the version the script published under.
    assert!(
        order.iter().all(|k| k.contains(&format!("/paddock-dev/{}/", r.consumer_hash))),
        "published under {order:?}, consumers ask for {}",
        r.consumer_hash
    );
    // The trace (bash -x) and the output never hold the token.
    assert!(
        !r.stdout().contains(SECRET) && !r.stderr().contains(SECRET),
        "the token leaked into the log"
    );
}

#[test]
fn a_server_error_on_the_binary_stops_the_job_and_no_checksum_is_uploaded() {
    let store = Store::start(BTreeMap::new(), Some(("paddock-dev-test-host", "503 Service Unavailable")));
    let r = publish(&store);
    assert!(!r.out.status.success());
    assert!(
        r.stderr()
            .contains("::error::dev-tool: upload of paddock-dev-test-host failed (HTTP 503"),
        "{}",
        r.stderr()
    );
    assert!(
        store.order.lock().unwrap().is_empty(),
        "nothing was stored, in particular no checksum"
    );
}

#[test]
fn a_binary_that_is_already_stored_is_checked_before_its_checksum_is_uploaded() {
    // First a normal publish, to get the bytes this script produces; then a store that holds only the binary (a half-finished run).
    let first = Store::start(BTreeMap::new(), None);
    assert!(publish(&first).out.status.success());
    let stored = first.files.lock().unwrap().clone();
    let binary_only: BTreeMap<_, _> = stored
        .iter()
        .filter(|(k, _)| !k.ends_with(".sha256"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let same = Store::start(binary_only.clone(), None);
    let r = publish(&same);
    assert!(r.out.status.success(), "{}\n{}", r.stdout(), r.stderr());
    assert_eq!(
        names(&same.order.lock().unwrap()),
        ["paddock-dev-test-host.sha256"],
        "only the missing checksum is uploaded"
    );

    let other: BTreeMap<_, _> = binary_only.keys().map(|k| (k.clone(), b"another build".to_vec())).collect();
    let differs = Store::start(other, None);
    let r = publish(&differs);
    assert!(!r.out.status.success());
    assert!(
        r.stderr().contains("::error::dev-tool:") && r.stderr().contains("differs"),
        "{}",
        r.stderr()
    );
    assert!(
        differs.order.lock().unwrap().is_empty(),
        "no checksum for a binary that is not ours"
    );
}
