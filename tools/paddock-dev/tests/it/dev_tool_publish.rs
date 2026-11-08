//! `conformance/ci/dev-tool-publish.sh` (ADR 0022 R0b) uploads the tool the way Gitea's generic package store accepts it. Gitea answers
//! HTTP 500 "request Content-Type isn't multipart/form-data" to a PUT that curl's `--data-binary` labelled as a form (run 7769).
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
#![cfg(unix)]
use super::wrapper::{Fixture, fake_rustc, shim_cargo, write_script};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A store that behaves like Gitea's generic packages: 404 for what was not uploaded, 500 for a PUT labelled as a form, else 201.
struct Store {
    addr: String,
    files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Store {
    fn start() -> Store {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let files = Arc::new(Mutex::new(BTreeMap::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (fs, st) = (files.clone(), stop.clone());
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
                let reply = if method == "PUT" {
                    if lower.contains("content-type: application/x-www-form-urlencoded") {
                        "500 Internal Server Error"
                    } else {
                        fs.lock().unwrap().insert(path, body);
                        "201 Created"
                    }
                } else if fs.lock().unwrap().contains_key(&path) {
                    "200 OK"
                } else {
                    "404 Not Found"
                };
                let _ = write!(c, "HTTP/1.1 {reply}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        });
        Store {
            addr,
            files,
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

#[test]
fn the_tool_is_uploaded_binary_first_then_its_checksum_and_never_labelled_as_a_form() {
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
    let store = Store::start();
    let path = format!("{}:{}", shims.display(), std::env::var("PATH").unwrap());
    let out = Command::new("bash")
        .arg(&script)
        .current_dir(f.root())
        .env("PATH", path)
        .env("CARGO_HOME", f.cargo_home.path())
        .env("PADDOCK_DEV_PACKAGES", format!("http://{}/generic", store.addr))
        .env("PUBLISH_TOKEN", "write-token")
        .env_remove("RUSTC")
        .env_remove("GITHUB_STEP_SUMMARY")
        .output()
        .unwrap();
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{stdout}\n{stderr}");
    let files = store.files.lock().unwrap();
    let names: Vec<_> = files.keys().map(|k| k.rsplit('/').next().unwrap().to_string()).collect();
    assert_eq!(names, ["paddock-dev-test-host", "paddock-dev-test-host.sha256"], "{stdout}");
    assert!(!stdout.contains("write-token") && !stderr.contains("write-token"));
}
