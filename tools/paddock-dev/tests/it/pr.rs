//! `./dev pr` end to end: the real binary and curl against a fake Gitea on 127.0.0.1 (never the real server), in a throw-away
//! repository. Checks what reaches the server (method, path, token header, JSON body) and that the token is printed nowhere.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};

const TOKEN: &str = "tok-SECRET-4711";

#[derive(Default)]
struct State {
    labels: Vec<Value>,
    pulls: Vec<Value>,
    /// `(method, path, authorization, body)`.
    seen: Vec<(String, String, String, Value)>,
}

fn route(s: &mut State, method: &str, path: &str, body: &Value) -> (u16, Value) {
    let (p, query) = path.split_once('?').unwrap_or((path, ""));
    let p = p.strip_prefix("/api/v1/repos/o/r").unwrap_or("");
    let page = query.split('&').find_map(|q| q.strip_prefix("page=")).unwrap_or("1");
    let first = |v: &Vec<Value>| if page == "1" { Value::Array(v.clone()) } else { json!([]) };
    match (method, p) {
        ("GET", "") => (200, json!({"default_branch": "main", "autodetect_manual_merge": false})),
        ("GET", "/labels") => (200, first(&s.labels)),
        ("POST", "/labels") => {
            let mut l = body.clone();
            l["id"] = json!(s.labels.len() + 1);
            s.labels.push(l.clone());
            (201, l)
        }
        ("GET", "/pulls") => {
            let state = query.split('&').find_map(|q| q.strip_prefix("state=")).unwrap_or("open");
            let v: Vec<Value> = s.pulls.iter().filter(|x| state == "all" || x["state"] == state).cloned().collect();
            (200, first(&v))
        }
        ("POST", "/pulls") => {
            let n = s.pulls.len() + 1;
            let ids: Vec<Value> = body["labels"].as_array().unwrap().clone();
            let labels: Vec<Value> = s.labels.iter().filter(|l| ids.contains(&l["id"])).cloned().collect();
            let pr = json!({"number": n, "title": body["title"], "state": "open", "draft": true, "merged": false,
                "head": {"ref": body["head"], "sha": "abcdef0123456789"}, "base": {"ref": body["base"]}, "labels": labels,
                "html_url": format!("http://stub/o/r/pulls/{n}")});
            s.pulls.push(pr.clone());
            (201, pr)
        }
        ("PUT", p) if p.starts_with("/issues/") && p.ends_with("/labels") => {
            let n: u64 = p.trim_start_matches("/issues/").trim_end_matches("/labels").parse().unwrap();
            let ids: Vec<Value> = body["labels"].as_array().unwrap().clone();
            let labels: Vec<Value> = s.labels.iter().filter(|l| ids.contains(&l["id"])).cloned().collect();
            let pr = s.pulls.iter_mut().find(|x| x["number"] == n).unwrap();
            pr["labels"] = Value::Array(labels.clone());
            (200, Value::Array(labels))
        }
        ("GET", "/actions/runs") => (
            200,
            json!({"total_count": 1, "workflow_runs": [{"id": 31, "path": "ci.yml@refs/heads/feat/x", "status": "completed", "conclusion": "success"}]}),
        ),
        _ => (404, json!({"message": "not found"})),
    }
}

/// Starts the fake server; returns its root URL and state.
fn serve() -> (String, Arc<Mutex<State>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Mutex::new(State::default()));
    let st = state.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { continue };
            let mut reader = BufReader::new(conn.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                continue;
            }
            let mut parts = line.split_whitespace();
            let (method, path) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
            let (mut len, mut auth, mut expect) = (0usize, String::new(), false);
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                let h = h.trim_end();
                if h.is_empty() {
                    break;
                }
                let (k, v) = h.split_once(':').unwrap_or((h, ""));
                match k.to_ascii_lowercase().as_str() {
                    "content-length" => len = v.trim().parse().unwrap(),
                    "authorization" => auth = v.trim().to_string(),
                    "expect" => expect = true,
                    _ => {}
                }
            }
            if expect {
                conn.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
            }
            let mut body = vec![0u8; len];
            reader.read_exact(&mut body).unwrap();
            let body: Value = if len == 0 {
                Value::Null
            } else {
                serde_json::from_slice(&body).expect("the client sends JSON")
            };
            let (code, answer) = {
                let mut s = st.lock().unwrap();
                s.seen.push((method.clone(), path.clone(), auth, body.clone()));
                route(&mut s, &method, &path, &body)
            };
            let text = answer.to_string();
            let _ = write!(
                conn,
                "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            );
        }
    });
    (url, state)
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = paddock_proc::command("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// `<tmp>/work`: a repository with `.gitea/labels.toml`, app dyno, main and `feat/x` two commits ahead, both pushed to the
/// bare `<tmp>/origin.git`; its `origin/feat/x` is then made stale, so only a fetch shows the branch's commits.
fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let origin = d.path().join("origin.git");
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare"]);
    let r = &d.path().join("work");
    std::fs::create_dir_all(r.join(".gitea")).unwrap();
    std::fs::create_dir_all(r.join("apps/dyno")).unwrap();
    std::fs::write(r.join("apps/dyno/paddock.ci.json"), "{}").unwrap();
    let real = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.gitea/labels.toml")).unwrap();
    // A description with quotes and a backslash: the body travels through curl's config quoting.
    let extra = "\n[[label]]\nname = \"quoted\"\ncolor = \"#123456\"\ndescription = 'say \"hi\" \\ back'\n";
    std::fs::write(r.join(".gitea/labels.toml"), real + extra).unwrap();
    git(r, &["init", "-q", "-b", "main"]);
    git(r, &["add", "-A"]);
    git(r, &["commit", "-q", "-m", "chore: base"]);
    git(r, &["checkout", "-q", "-b", "feat/x"]);
    for s in ["feat: one", "fix: two \"quoted\""] {
        git(r, &["commit", "-q", "--allow-empty", "-m", s]);
    }
    git(r, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(r, &["push", "-q", "origin", "main", "feat/x"]);
    git(r, &["update-ref", "refs/remotes/origin/feat/x", "main"]);
    d
}

fn dev(root: &Path, server: &str, args: &[&str]) -> (i32, String, String) {
    // The common flags go right after `pr <command>`: `pr label` takes every later word as a change.
    let at = args.iter().position(|a| *a == "pr").unwrap() + 2;
    let out = paddock_proc::command(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(&args[..at])
        .args(["--server", server, "--repo", "o/r", "--root"])
        .arg(root)
        .args(&args[at..])
        .env("GITEA_TOKEN", TOKEN)
        // The fake server is on 127.0.0.1: never through a proxy of the machine running the test.
        .env("NO_PROXY", "*")
        .env("no_proxy", "*")
        .output()
        .unwrap();
    let (o, e) = (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    );
    assert!(!o.contains(TOKEN) && !e.contains(TOKEN), "the token was printed: {o}{e}");
    (out.status.code().unwrap(), o, e)
}

#[test]
fn sync_open_label_and_list_against_a_local_fake_gitea() {
    let (server, state) = serve();
    let d = repo();
    let root = &d.path().join("work");

    let (code, out, err) = dev(root, &server, &["--json", "pr", "sync"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        err.contains("autodetect manual merge"),
        "sync warns while the setting is off: {err}"
    );
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert!(v["created"].as_array().unwrap().iter().any(|l| l == "app/dyno"), "{v}");
    {
        let s = state.lock().unwrap();
        let q = s.labels.iter().find(|l| l["name"] == "quoted").unwrap();
        assert_eq!(q["description"], "say \"hi\" \\ back", "the body survives curl's config quoting");
        assert!(
            s.seen.iter().all(|(_, _, auth, _)| auth == &format!("token {TOKEN}")),
            "every request carries the token header"
        );
    }

    let (code, out, err) = dev(root, &server, &["--json", "pr", "open", "feat/x", "--labels", "owes/windows-tests"]);
    assert_eq!(code, 0, "{out}{err}");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["labels"], json!(["state/built", "owes/windows-tests"]));
    {
        let s = state.lock().unwrap();
        let (_, _, _, body) = s
            .seen
            .iter()
            .rev()
            .find(|(m, p, _, _)| m == "POST" && p.ends_with("/pulls"))
            .unwrap();
        assert_eq!(
            (body["head"].as_str(), body["base"].as_str(), body["title"].as_str()),
            (Some("feat/x"), Some("main"), Some("WIP: feat/x"))
        );
        let text = body["body"].as_str().unwrap();
        assert!(text.contains(" feat: one\n") && text.contains(" fix: two \"quoted\"\n"), "{text}");
    }

    let (code, out, err) = dev(root, &server, &["pr", "open", "feat/x"]);
    assert_eq!(code, 1, "a second pull request is refused: {out}{err}");
    assert!(err.contains("already has an open pull request: #1"), "{err}");

    let (code, out, err) = dev(root, &server, &["pr", "label", "feat/x", "+state/approved", "-owes/windows-tests"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("state/built,owes/windows-tests -> state/approved"), "{out}");

    let (code, out, err) = dev(root, &server, &["--json", "pr", "list", "--state", "approved"]);
    assert_eq!(code, 0, "{out}{err}");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["pulls"][0]["branch"], "feat/x");
    assert_eq!(v["pulls"][0]["ci"], "ci 31 success");
}

#[test]
fn no_token_is_a_usage_error_that_says_what_to_do() {
    let out = paddock_proc::command(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["pr", "list", "--server", "http://127.0.0.1:9"])
        .env_remove("GITEA_TOKEN")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("GITEA_TOKEN is not set"));
}
