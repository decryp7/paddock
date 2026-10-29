//! `dev fetch` through the real binary and the real HTTP transport, against a server of the test's own on the loopback (no
//! other network, nothing installed on the machine).
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn dev(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(args)
        .env("NO_PROXY", "*")
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("GITHUB_ENV")
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Serves `body` to every GET until dropped.
struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn start(body: Vec<u8>) -> Server {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = std::thread::spawn(move || {
            for conn in listener.incoming() {
                if s.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(mut c) = conn else { continue };
                let mut buf = [0u8; 4096];
                let _ = c.read(&mut buf);
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = c.write_all(head.as_bytes());
                let _ = c.write_all(&body);
            }
        });
        Server {
            port,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn zip_with_browser() -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file(
        "chrome-linux64/chrome",
        zip::write::SimpleFileOptions::default().unix_permissions(0o755),
    )
    .unwrap();
    w.write_all(b"not a real browser").unwrap();
    w.finish().unwrap().into_inner()
}

fn catalogue(port: u16, zip: &[u8], sha: &str) -> String {
    format!(
        "format = 1\nname = \"chromium\"\nversion = \"7.7.7\"\n[[platform]]\nkey = \"linux-x86_64\"\nurl = \"http://127.0.0.1:{port}/7.7.7/linux64/chrome-linux64.zip\"\nsize = {}\nsha256 = \"{sha}\"\n",
        zip.len()
    )
}

#[test]
fn chrome_installs_over_http_prints_the_exports_and_reuses_the_install() {
    let zip = zip_with_browser();
    let sha: String = Sha256::digest(&zip).iter().map(|b| format!("{b:02x}")).collect();
    let server = Server::start(zip.clone());
    let work = tempfile::tempdir().unwrap();
    let cat = work.path().join("chromium.toml");
    std::fs::write(&cat, catalogue(server.port, &zip, &sha)).unwrap();
    let dir = work.path().join("browser");
    let args = [
        "--json",
        "fetch",
        "chrome",
        "--catalogue",
        cat.to_str().unwrap(),
        "--dir",
        dir.to_str().unwrap(),
        "--platform",
        "linux-x86_64",
        "--env-prefix",
        "TEST",
    ];
    let (code, out, err) = dev(&args);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(
        (v["ok"].clone(), v["cached"].clone(), v["version"].clone()),
        (true.into(), false.into(), "7.7.7".into())
    );
    assert!(v["executable"].as_str().unwrap().ends_with("chrome"));
    assert_eq!(v["env"][1], "TEST_REQUIRE_TEST_BROWSER=1");
    for phase in ["Connecting", "Downloading", "Verifying", "Unpacking", "Installing"] {
        assert!(err.contains(phase), "progress names {phase}: {err}");
    }
    let (code, out, _) = dev(&args);
    assert_eq!(code, 0);
    assert_eq!(serde_json::from_str::<Value>(out.trim()).unwrap()["cached"], true);
}

#[test]
fn a_wrong_hash_is_a_failure_that_leaves_no_install() {
    let zip = zip_with_browser();
    let server = Server::start(zip.clone());
    let work = tempfile::tempdir().unwrap();
    let cat = work.path().join("chromium.toml");
    std::fs::write(&cat, catalogue(server.port, &zip, &"0".repeat(64))).unwrap();
    let dir = work.path().join("browser");
    let (code, _, err) = dev(&[
        "fetch",
        "chrome",
        "--catalogue",
        cat.to_str().unwrap(),
        "--dir",
        dir.to_str().unwrap(),
        "--platform",
        "linux-x86_64",
    ]);
    assert_eq!(code, 1, "{err}");
    assert!(!dir.exists());
}

#[test]
fn pin_measures_over_http_and_rewrites_the_catalogue() {
    let zip = zip_with_browser();
    let server = Server::start(zip.clone());
    let work = tempfile::tempdir().unwrap();
    let cat = work.path().join("chromium.toml");
    std::fs::write(
        &cat,
        catalogue(server.port, &zip, &"0".repeat(64)).replace("size =", "# measured\nsize ="),
    )
    .unwrap();
    let (code, out, err) = dev(&[
        "fetch",
        "pin",
        cat.to_str().unwrap(),
        "--work",
        work.path().join("w").to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("pinned 7.7.7"));
    let text = std::fs::read_to_string(&cat).unwrap();
    let sha: String = Sha256::digest(&zip).iter().map(|b| format!("{b:02x}")).collect();
    assert!(text.contains(&sha) && text.contains("# measured"), "{text}");
}

#[test]
fn usage_errors_and_missing_inputs_exit_with_the_stable_codes() {
    for args in [
        &["fetch", "--help"][..],
        &["fetch", "chrome", "--help"],
        &["fetch", "pin", "--help"],
    ] {
        let (code, out, _) = dev(args);
        assert_eq!(code, 0, "{args:?}");
        assert!(out.contains("Usage"));
    }
    let (code, _, err) = dev(&["fetch", "chrome", "--platform", "nope", "--dir", "x"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("platform key"));
    let (code, out, _) = dev(&["--json", "fetch", "chrome", "--env-prefix", "bad", "--dir", "x"]);
    assert_eq!(code, 2);
    assert_eq!(serde_json::from_str::<Value>(out.trim()).unwrap()["kind"], "usage");
    let (code, _, _) = dev(&["fetch", "pin", "/nonexistent/c.toml"]);
    assert_eq!(code, 1);
    let (code, _, _) = dev(&["fetch", "chrome", "--jobs", "99"]);
    assert_eq!(code, 2);
}
