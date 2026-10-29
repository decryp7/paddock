//! Runs the built `pitwall` binary against a tiny fake of Pitwall's local API on an ephemeral 127.0.0.1 port (never the
//! production port, never the user's data folder or credential store: the data folder is a temp dir and the admin key
//! comes from the environment).
// Tests start the built binary directly (no console window concerns; each run is short and bounded).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

const ADMIN: &str = "test-admin-key-0123456789";
const APP_KEY: &str = "pw_app_key_for_chat_tests";

type Handler = Box<dyn Fn(&Value) -> (u16, String, &'static str) + Send + Sync>;

/// A fake server: routes by path; records `(path, authorization, body)`.
struct Fake {
    port: u16,
    seen: Arc<Mutex<Vec<(String, String, String)>>>,
}

fn json_reply(status: u16, v: Value) -> (u16, String, &'static str) {
    (status, v.to_string(), "application/json")
}

impl Fake {
    fn start(routes: HashMap<&'static str, Handler>) -> Fake {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { break };
                let routes = &routes;
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let (head_end, len) = loop {
                    let n = s.read(&mut tmp).unwrap_or(0);
                    if n == 0 {
                        break (0, 0);
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..p]).to_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        break (p + 4, len);
                    }
                };
                while head_end > 0 && buf.len() < head_end + len {
                    let n = s.read(&mut tmp).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                let head = String::from_utf8_lossy(&buf[..head_end.min(buf.len())]).into_owned();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                let auth = head
                    .lines()
                    .find_map(|l| l.to_lowercase().starts_with("authorization:").then(|| l[14..].trim().to_string()))
                    .unwrap_or_default();
                let body_text = String::from_utf8_lossy(&buf[head_end.min(buf.len())..]).into_owned();
                let body: Value = serde_json::from_str(&body_text).unwrap_or(Value::Null);
                seen2.lock().unwrap().push((path.clone(), auth, body_text));
                let (status, text, ctype) = match routes.get(path.as_str()) {
                    Some(h) => h(&body),
                    None => json_reply(404, json!({"code": "notFound", "message": "no such call"})),
                };
                let reply = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                    text.len()
                );
                let _ = s.write_all(reply.as_bytes());
            }
        });
        Fake { port, seen }
    }

    fn requests(&self) -> Vec<(String, String, String)> {
        self.seen.lock().unwrap().clone()
    }
}

fn rpc(m: &str) -> String {
    format!("/mgmt/v1/rpc/{m}")
}

/// The CLI with an empty environment, except what Windows sockets need to start (without SystemRoot every connect fails).
fn bare(args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_pitwall"));
    c.args(args).env_clear();
    for k in ["SystemRoot", "windir"] {
        if let Some(v) = std::env::var_os(k) {
            c.env(k, v);
        }
    }
    c
}

fn pitwall(home: &std::path::Path, args: &[&str]) -> Output {
    bare(args)
        .env("PITWALL_HOME", home)
        .env("PITWALL_ADMIN_KEY", ADMIN)
        .env("PITWALL_API_KEY", APP_KEY)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn overview() -> Value {
    json!({
        "server": {"version": "0.1.0", "apiVersion": 1, "pid": 4242, "port": 11435, "startedAt": 1, "serving": "serving", "offline": true,
                   "lan": {"enabled": false, "url": null, "certFingerprint": null}},
        "memory": {"at": 1, "ramTotal": 32_000_000_000u64, "ramAvailable": 12_500_000_000u64, "commitUsed": null, "commitLimit": null,
                   "swapUsed": 0, "pressure": "normal", "cpuPercent": 3.0, "gpus": [{"name": "Test GPU", "vramTotal": 8_000_000_000u64,
                   "vramUsed": 2_000_000_000u64, "utilisation": 10.0, "unified": false}], "pitwallRss": 1, "foregroundBusy": false},
        "models": [loaded()],
        "engines": [],
        "queue": {"running": [], "waiting": [], "depth": {"interactive": 0, "background": 0, "batch": 0}},
        "throughput": {"promptTps": 0.0, "genTps": 41.6, "ttftP50Ms": 0.0, "requestsPerMin": 2.0, "activeRequests": 0},
        "apps": [],
        "preset": "balanced",
        "pairings": [],
        "issues": []
    })
}

fn loaded() -> Value {
    json!({"modelId": "acme--tiny--q4", "name": "Tiny", "engine": "llama.cpp", "build": "vulkan", "state": "ready", "ramBytes": 1_500_000_000u64,
           "vramBytes": 0, "contextTokens": 4096, "slots": 2, "pinned": true, "activeRequests": 0, "loadedAt": 1, "lastUsedAt": 1,
           "idleReleaseAt": null, "coldStartMs": 900, "capabilities": ["chat"]})
}

fn local_model() -> Value {
    json!({"id": "acme--tiny--q4", "repo": "acme/tiny", "file": "tiny-q4.gguf", "name": "Tiny", "quant": "Q4_K_M", "sizeBytes": 400_000_000u64,
           "paramsB": 0.5, "contextMax": 32768, "architecture": "llama", "capabilities": ["chat"], "projector": null, "addedAt": 1,
           "lastUsedAt": null, "loaded": true, "pinned": false, "idleSecs": null, "engine": null, "slots": 1, "slotsCustom": false,
           "loadedSlots": 1,
           "fit": {"verdict": "fits", "needRam": 1, "needVram": 0, "haveRam": 2, "haveVram": 0, "offload": "none", "message": "ok"}})
}

fn app_json(keys: Value) -> Value {
    json!({"id": "app-1", "name": "Debrief", "suite": true, "connected": true, "lastSeenAt": null, "keys": keys, "permissions": {}, "today": {}})
}

fn route(path: String, h: impl Fn(&Value) -> (u16, String, &'static str) + Send + Sync + 'static) -> (&'static str, Handler) {
    (Box::leak(path.into_boxed_str()), Box::new(h))
}

fn fake(list: Vec<(&'static str, Handler)>) -> Fake {
    Fake::start(list.into_iter().collect())
}

fn home() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn status_text_and_json_authenticate_with_the_admin_key() {
    let f = fake(vec![route(rpc("overview"), |_| json_reply(200, overview()))]);
    let h = home();
    let port = f.port.to_string();
    let out = pitwall(h.path(), &["status", "--port", &port]);
    let s = text(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        s.contains("127.0.0.1:11435") && s.contains("(offline mode)") && s.contains("12.5 GB free of 32.0 GB"),
        "{s}"
    );
    assert!(s.contains("acme--tiny--q4") && s.contains("Test GPU"), "{s}");
    let out = pitwall(h.path(), &["--json", "status", "--port", &port]);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["server"]["pid"], 4242);
    let reqs = f.requests();
    assert!(reqs.iter().all(|r| r.1 == format!("Bearer {ADMIN}")));
    // the key is never printed
    assert!(!text(&out.stdout).contains(ADMIN) && !text(&out.stderr).contains(ADMIN));
}

#[test]
fn the_discovery_file_names_the_port() {
    let f = fake(vec![route(rpc("overview"), |_| json_reply(200, overview()))]);
    let h = home();
    let d = pitwall_core::discovery::Discovery::new("0.1.0", 1, f.port, 0);
    std::fs::write(h.path().join("discovery.json"), serde_json::to_string(&d).unwrap()).unwrap();
    let out = pitwall(h.path(), &["ps"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("acme--tiny--q4") && text(&out.stdout).contains("pinned"));
    let out = pitwall(h.path(), &["ps", "--json"]);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["modelId"], "acme--tiny--q4");
}

#[test]
fn models_list_load_and_unload() {
    let f = fake(vec![
        route(rpc("local_models"), |_| json_reply(200, json!([local_model()]))),
        route(rpc("load_model"), |b| {
            if b["id"] == "acme--tiny--q4" {
                json_reply(200, Value::Null)
            } else {
                json_reply(404, json!({"code": "notFound", "message": "the model is not installed"}))
            }
        }),
        route(rpc("unload_model"), |_| {
            json_reply(409, json!({"code": "conflict", "message": "the model is not loaded"}))
        }),
    ]);
    let h = home();
    let port = f.port.to_string();
    let out = pitwall(h.path(), &["models", "--port", &port]);
    let s = text(&out.stdout);
    assert!(
        s.contains("Q4_K_M") && s.contains("400 MB") && s.contains("loaded") && s.contains("fits"),
        "{s}"
    );
    assert!(pitwall(h.path(), &["models", "--json", "--port", &port]).stdout.starts_with(b"["));
    let out = pitwall(h.path(), &["load", "acme--tiny--q4", "--port", &port]);
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stderr).contains("Loaded"));
    let out = pitwall(h.path(), &["load", "nope", "--port", &port]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("not installed"));
    let out = pitwall(h.path(), &["unload", "x", "--port", &port]);
    assert_eq!(out.status.code(), Some(4), "a conflict is a refusal");
}

#[test]
fn exit_codes() {
    let f = fake(vec![
        route(rpc("load_model"), |_| {
            json_reply(503, json!({"code": "insufficientMemory", "message": "needs 9.0 GB, 3.0 GB free"}))
        }),
        route(rpc("overview"), |_| {
            json_reply(401, json!({"code": "unauthorized", "message": "wrong key"}))
        }),
        route(rpc("download_model"), |_| {
            json_reply(403, json!({"code": "offline", "message": "offline mode is on"}))
        }),
    ]);
    let h = home();
    let port = f.port.to_string();
    let out = pitwall(h.path(), &["load", "big", "--port", &port]);
    assert_eq!(out.status.code(), Some(4));
    assert!(text(&out.stderr).contains("pitwall unload"));
    let out = pitwall(h.path(), &["status", "--port", &port]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("PITWALL_ADMIN_KEY") && !text(&out.stderr).contains(ADMIN));
    let out = pitwall(h.path(), &["pull", "a/b/c.gguf", "--port", &port]);
    assert_eq!(out.status.code(), Some(4));
    assert!(text(&out.stderr).contains("offline"));
    // usage
    assert_eq!(pitwall(h.path(), &["frobnicate"]).status.code(), Some(2));
    let out = pitwall(h.path(), &["status", "--key", "abc"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(!text(&out.stderr).contains("abc "));
    assert_eq!(pitwall(h.path(), &["--help"]).status.code(), Some(0));
    assert!(text(&pitwall(h.path(), &["--version"]).stdout).starts_with("pitwall "));
    // a port that answers nothing (held for the whole test, never freed: the OS cannot hand it to another test's server)
    let held = paddock_testkit::net::HeldPort::hang_up();
    let out = pitwall(h.path(), &["status", "--port", &held.port().to_string()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("Is something else using port"), "{}", text(&out.stderr));
    // no admin key anywhere readable: an unusable --key-file
    let out = pitwall(h.path(), &["status", "--port", &port, "--key-file", "/nonexistent/key"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("--key-file") || text(&out.stderr).contains("/nonexistent/key"));
}

#[test]
fn key_file_is_used_when_given() {
    let f = fake(vec![route(rpc("overview"), |_| json_reply(200, overview()))]);
    let h = home();
    let kf = h.path().join("k");
    std::fs::write(&kf, "from-a-file-key\n").unwrap();
    let out = pitwall(
        h.path(),
        &["status", "--port", &f.port.to_string(), "--key-file", kf.to_str().unwrap()],
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(f.requests()[0].1, "Bearer from-a-file-key");
}

#[test]
fn keys_create_shows_the_secret_once_on_stdout() {
    let created = |app: &str| json!({"appId": app, "key": {"id": "key-9", "prefix": "pw_abc", "createdAt": 1, "lastUsedAt": null}, "secret": "pw_abc_SECRET_VALUE"});
    let f = fake(vec![
        route(rpc("apps"), |_| {
            json_reply(
                200,
                json!([app_json(
                    json!([{"id": "key-1", "prefix": "pw_old", "createdAt": 0, "lastUsedAt": null}])
                )]),
            )
        }),
        route(rpc("create_app"), |b| {
            let mut a = app_json(json!([]));
            a["id"] = json!("app-2");
            a["name"] = b["name"].clone();
            json_reply(200, a)
        }),
        route(rpc("create_key"), move |b| {
            json_reply(200, created(b["appId"].as_str().unwrap_or("?")))
        }),
        route(rpc("revoke_key"), |b| {
            if b["keyId"] == "key-1" {
                json_reply(200, Value::Null)
            } else {
                json_reply(404, json!({"code": "notFound", "message": "no such key"}))
            }
        }),
    ]);
    let h = home();
    let port = f.port.to_string();
    let out = pitwall(h.path(), &["keys", "--port", &port]);
    let s = text(&out.stdout);
    assert!(
        s.contains("Debrief") && s.contains("key-1 (pw_old...)") && s.contains("never"),
        "{s}"
    );
    // an existing app by name (any case)
    let out = pitwall(h.path(), &["keys", "create", "debrief", "--port", &port]);
    assert_eq!(text(&out.stdout), "pw_abc_SECRET_VALUE\n");
    assert!(text(&out.stderr).contains("only time"));
    assert!(f.requests().iter().any(|r| r.0 == rpc("create_key") && r.2.contains("app-1")));
    // a new app
    let out = pitwall(h.path(), &["keys", "create", "Notes", "--new", "--port", &port]);
    assert_eq!(text(&out.stdout), "pw_abc_SECRET_VALUE\n");
    assert!(f.requests().iter().any(|r| r.0 == rpc("create_key") && r.2.contains("app-2")));
    // json
    let out = pitwall(h.path(), &["keys", "create", "app-1", "--json", "--port", &port]);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["secret"], "pw_abc_SECRET_VALUE");
    // errors name the fix
    let out = pitwall(h.path(), &["keys", "create", "Ghost", "--port", &port]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("--new"));
    let out = pitwall(h.path(), &["keys", "create", "Debrief", "--new", "--port", &port]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("already exists"));
    // listing never prints a secret
    assert!(!text(&pitwall(h.path(), &["keys", "--json", "--port", &port]).stdout).contains("SECRET"));
    let out = pitwall(h.path(), &["keys", "revoke", "Debrief", "key-1", "--port", &port]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        pitwall(h.path(), &["keys", "revoke", "Debrief", "key-x", "--port", &port])
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn presets_list_and_show() {
    let entries = json!([{
        "preset": {"alias": "fast", "target": "auto:chat,fast", "temperature": 0.2, "systemPrompt": "Be brief."},
        "status": {"state": "ready", "modelId": "acme--tiny--q4", "chosenBy": "capabilities", "reason": "smallest", "loaded": true}
    }]);
    let f = fake(vec![route(rpc("presets"), move |_| json_reply(200, entries.clone()))]);
    let h = home();
    let port = f.port.to_string();
    let s = text(&pitwall(h.path(), &["presets", "--port", &port]).stdout);
    assert!(
        s.contains("fast") && s.contains("auto:chat,fast") && s.contains("ready: acme--tiny--q4"),
        "{s}"
    );
    let s = text(&pitwall(h.path(), &["presets", "show", "fast", "--port", &port]).stdout);
    assert!(s.contains("Temperature:") && s.contains("9 characters"), "{s}");
    let out = pitwall(h.path(), &["presets", "show", "slow", "--port", &port]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("Known presets: fast"));
}

#[test]
fn pull_follows_progress_until_done() {
    let polls = Arc::new(Mutex::new(0u32));
    let dl = |state: &str, done: u64| {
        json!({"id": "dl-1", "kind": "model", "name": "tiny-q4.gguf", "host": "huggingface.co", "bytesDone": done, "bytesTotal": 400_000_000u64,
               "speedBps": 8_000_000, "state": state, "error": null, "startedAt": 1})
    };
    let f = fake(vec![
        route(rpc("download_model"), move |b| {
            assert_eq!(
                (b["repo"].as_str(), b["file"].as_str()),
                (Some("acme/tiny"), Some("sub/tiny-q4.gguf"))
            );
            json_reply(200, dl("downloading", 0))
        }),
        route(rpc("downloads"), move |_| {
            let mut n = polls.lock().unwrap();
            *n += 1;
            json_reply(
                200,
                json!([if *n < 2 {
                    dl("downloading", 200_000_000)
                } else {
                    dl("done", 400_000_000)
                }]),
            )
        }),
    ]);
    let h = home();
    let port = f.port.to_string();
    let out = pitwall(h.path(), &["pull", "acme/tiny/sub/tiny-q4.gguf", "--port", &port]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let e = text(&out.stderr);
    assert!(e.contains("from huggingface.co") && e.contains(" 50%") && e.contains("done"), "{e}");
    let out = pitwall(h.path(), &["pull", "acme/tiny/sub/tiny-q4.gguf", "--no-wait", "--port", &port]);
    assert_eq!(text(&out.stdout), "dl-1\n");
}

#[test]
fn pull_reports_a_failed_download() {
    let dl = json!({"id": "dl-1", "kind": "model", "name": "x.gguf", "host": "huggingface.co", "bytesDone": 1, "bytesTotal": 10, "speedBps": 0,
                    "state": "failed", "error": "disk full", "startedAt": 1});
    let (a, b) = (dl.clone(), dl);
    let f = fake(vec![
        route(rpc("download_model"), move |_| json_reply(200, a.clone())),
        route(rpc("downloads"), move |_| json_reply(200, json!([b.clone()]))),
    ]);
    let h = home();
    let out = pitwall(h.path(), &["pull", "o/r/x.gguf", "--port", &f.port.to_string()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("disk full") && text(&out.stderr).contains("resume"));
}

#[test]
fn bench_prints_speeds() {
    let f = fake(vec![route(rpc("run_benchmark"), |b| {
        json_reply(
            200,
            json!({"modelId": b["modelId"], "build": "vulkan", "at": 1, "promptTps": 410.0, "genTps": 52.3, "ttftMs": 180, "contextTokens": 4096}),
        )
    })]);
    let h = home();
    let out = pitwall(h.path(), &["bench", "tiny", "--port", &f.port.to_string()]);
    assert!(text(&out.stdout).contains("52.3 tokens/s") && text(&out.stdout).contains("180 ms"));
}

const SSE: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\ndata: [DONE]\n\n";

#[test]
fn chat_streams_to_stdout_with_the_app_key() {
    let f = fake(vec![(
        "/v1/chat/completions",
        Box::new(|b: &Value| {
            if b["model"] == "ghost" {
                json_reply(404, json!({"error": {"message": "no such model"}}))
            } else {
                (200, SSE.to_string(), "text/event-stream")
            }
        }) as Handler,
    )]);
    let h = home();
    let port = f.port.to_string();
    let out = pitwall(
        h.path(),
        &[
            "chat",
            "fast",
            "say",
            "hi",
            "--system",
            "Be brief",
            "--max-tokens",
            "9",
            "--port",
            &port,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "Hello world\n");
    let r = &f.requests()[0];
    assert_eq!(r.1, format!("Bearer {APP_KEY}"));
    let body: Value = serde_json::from_str(&r.2).unwrap();
    assert_eq!((body["model"].as_str(), body["max_tokens"].as_u64()), (Some("fast"), Some(9)));
    assert_eq!(body["messages"][1]["content"], "say hi");
    // errors
    let out = pitwall(h.path(), &["chat", "ghost", "hi", "--port", &port]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("pitwall models"));
    // stdin is the prompt when none is given
    let mut child = bare(&["chat", "fast", "--port", &port])
        .env("PITWALL_HOME", h.path())
        .env("PITWALL_API_KEY", APP_KEY)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"from stdin").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(text(&out.stdout), "Hello world\n");
    assert!(f.requests().last().unwrap().2.contains("from stdin"));
    // an empty stdin is a usage error; a missing key is explained
    let out = pitwall(h.path(), &["chat", "fast", "--port", &port]);
    assert_eq!(out.status.code(), Some(2));
    let out = bare(&["chat", "fast", "hi", "--port", &port])
        .env("PITWALL_HOME", h.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("PITWALL_API_KEY"));
}

#[test]
fn a_call_that_never_answers_times_out() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port().to_string();
    let t = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut b = [0u8; 4096];
        let _ = s.read(&mut b);
        // Hold the connection without answering until the client gives up and closes it (no fixed wait).
        while s.read(&mut b).is_ok_and(|n| n > 0) {}
    });
    let h = home();
    let out = pitwall(h.path(), &["status", "--port", &port, "--timeout", "1"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("did not answer within 1 s"));
    t.join().unwrap();
}

/// A loopback port that is bound and held but not listening: connecting is refused (Linux and Windows; macOS drops the
/// attempt instead, so the test below does not run there).
#[cfg(not(target_os = "macos"))]
fn refused_port() -> (socket2::Socket, u16) {
    let s = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None).unwrap();
    s.bind(&std::net::SocketAddr::from(([127, 0, 0, 1], 0)).into()).unwrap();
    let port = s.local_addr().unwrap().as_socket().unwrap().port();
    (s, port)
}

#[cfg(not(target_os = "macos"))]
#[test]
fn nothing_listening_is_exit_3_with_the_next_step() {
    let (_held, port) = refused_port();
    let h = home();
    let out = pitwall(h.path(), &["status", "--port", &port.to_string()]);
    assert_eq!(out.status.code(), Some(3));
    assert!(text(&out.stderr).contains("Start the Pitwall app"), "{}", text(&out.stderr));
}
