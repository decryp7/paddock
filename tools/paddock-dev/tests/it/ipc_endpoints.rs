//! C102 (ADR 0072): the endpoint register check. Its positive control is the fixture tree `tests/fixtures/ipc-endpoints`
//! (ER-33): one production-style site per listen primitive, the skipped forms and the look-alikes. The scan must report exactly
//! the expected sites, and removing any fixture file must change that list, so a call the parser stops seeing turns this red.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::ipc_endpoints::{
    FAULTS, PRIMITIVES, RATCHET, REGISTER, RatchetEntry, SCENARIOS, Site, blocked_by, check, parse_ratchet, scan, tag_matches,
};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ipc-endpoints")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// What the scan must find in the fixture tree: `(file, call)`, one per site.
fn expected() -> Vec<(&'static str, &'static str)> {
    let mut v = vec![
        ("accepted.rs", "paddock_ipc::Accepted::serve_without_handshake"),
        ("aliased.rs", "std::net::TcpListener::bind"),
        ("aliased.rs", "std::net::TcpListener::bind"),
        ("glob_import.rs", "tokio::net::TcpListener::bind"),
        ("mcp_loopback.rs", "paddock_mcp::serve_loopback"),
        ("paddock_host.rs", "paddock_ipc::Host::listen*"),
        ("paddock_host.rs", "paddock_ipc::Host::listen*"),
        ("paddock_host.rs", "paddock_ipc::Host::listen*"),
        ("paddock_link.rs", "paddock_ipc::link::LinkServer::start"),
        ("paddock_listener.rs", "paddock_ipc::endpoint::Listener::bind*"),
        ("paddock_listener.rs", "paddock_ipc::endpoint::Listener::bind*"),
        ("paddock_listener.rs", "paddock_ipc::endpoint::Listener::bind*"),
        ("paddock_listener.rs", "paddock_ipc::endpoint::Listener::bind*"),
        ("socket2_listen.rs", "socket2::Socket::listen"),
        ("std_tcp.rs", "std::net::TcpListener::bind"),
        ("std_unix.rs", "std::os::unix::net::UnixListener::bind"),
        ("tokio_listeners.rs", "tokio::net::TcpListener::bind"),
        ("tokio_listeners.rs", "tokio::net::TcpListener::from_std"),
        ("tokio_listeners.rs", "tokio::net::UnixListener::bind"),
        ("tokio_listeners.rs", "tokio::net::UnixListener::from_std"),
        ("tokio_pipe.rs", "tokio::net::windows::named_pipe::ServerOptions::create"),
        (
            "tokio_pipe.rs",
            "tokio::net::windows::named_pipe::ServerOptions::create_with_security_attributes_raw",
        ),
        ("win_pipe.rs", "CreateNamedPipeW"),
    ];
    v.sort_unstable();
    v
}

fn found(sites: &[Site]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = sites.iter().map(|s| (s.file.clone(), s.call.clone())).collect();
    v.sort_unstable();
    v
}

fn expected_owned() -> Vec<(String, String)> {
    expected().into_iter().map(|(f, c)| (f.to_string(), c.to_string())).collect()
}

#[test]
fn the_scan_reports_every_fixture_site_by_its_call_and_nothing_else() {
    let sites = scan(&fixtures()).unwrap();
    assert_eq!(found(&sites), expected_owned(), "sites: {sites:#?}");
    // Every primitive of the check has a fixture site, so adding a primitive without a control fails here.
    for p in PRIMITIVES {
        assert!(sites.iter().any(|s| s.call == p), "no fixture site for the primitive {p}");
    }
}

#[test]
fn skipped_forms_and_look_alikes_report_nothing() {
    let sites = scan(&fixtures()).unwrap();
    for quiet in [
        "skipped_test_forms.rs",
        "skipped_test_forms/declared_elsewhere.rs",
        "not_listeners.rs",
    ] {
        assert!(sites.iter().all(|s| s.file != quiet), "{quiet} must report nothing: {sites:#?}");
    }
}

fn copy_tree(from: &Path, to: &Path, without: &str) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let (src, name) = (entry.path(), entry.file_name().to_string_lossy().to_string());
        if src.is_dir() {
            std::fs::create_dir_all(to.join(&name)).unwrap();
            copy_tree(&src, &to.join(&name), without);
        } else if name != without {
            std::fs::copy(&src, to.join(&name)).unwrap();
        }
    }
}

#[test]
fn removing_any_fixture_file_makes_the_scan_differ() {
    let mut files: Vec<&str> = expected().iter().map(|(f, _)| *f).collect();
    files.dedup();
    assert_eq!(files.len(), 13, "the fixture files with sites");
    for file in files {
        let dir = tempfile::tempdir().unwrap();
        copy_tree(&fixtures(), dir.path(), file);
        let sites = scan(dir.path()).unwrap();
        assert_ne!(
            found(&sites),
            expected_owned(),
            "without {file} the scan must differ from the expected list"
        );
        assert!(sites.iter().all(|s| s.file != file), "{file} was removed");
    }
}

#[test]
fn a_file_that_does_not_parse_is_an_error_not_a_silent_gap() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("broken.rs"), "fn main( {").unwrap();
    let e = scan(dir.path()).unwrap_err();
    assert!(e.contains("broken.rs") && e.contains("do not parse"), "{e}");
}

/// A repository in a folder: `files` are written first, then the register and the ratchet.
fn repo(files: &[(&str, &str)], register: &str, ratchet: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    std::fs::create_dir_all(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join(REGISTER), register).unwrap();
    std::fs::write(dir.path().join(RATCHET), ratchet).unwrap();
    dir
}

const SERVER: &str = "use std::net::TcpListener;\npub fn serve() { let _ = TcpListener::bind(\"127.0.0.1:0\"); }\n";
const TEST_FILE: &str = "#[test]\nfn the_ending_test() { let _ = \"ep\"; }\n";
const GOOD_REGISTER: &str = r#"
[[endpoint]]
id = "ep"
file = "src/server.rs"
calls = ["std::net::TcpListener::bind"]
sites = 1
ending_test = "tests/endings.rs::the_ending_test"
"#;

fn run(files: &[(&str, &str)], register: &str, ratchet: &str) -> Vec<String> {
    let dir = repo(files, register, ratchet);
    let (_, _, _, found) = check(dir.path()).unwrap();
    found
}

#[test]
fn a_registered_site_with_its_ending_test_is_clean() {
    let found = run(&[("src/server.rs", SERVER), ("tests/endings.rs", TEST_FILE)], GOOD_REGISTER, "");
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_new_listen_site_in_neither_section_fails_with_its_next_step() {
    let found = run(
        &[("src/server.rs", SERVER), ("src/other.rs", SERVER), ("tests/endings.rs", TEST_FILE)],
        GOOD_REGISTER,
        "",
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("src/other.rs:2") && found[0].contains("[[endpoint]]") && found[0].contains("[[out_of_scope]]"),
        "{}",
        found[0]
    );
}

#[test]
fn a_second_listen_call_in_a_registered_file_must_be_reviewed() {
    let two = format!("{SERVER}pub fn again() {{ let _ = std::net::TcpListener::bind(\"127.0.0.1:0\"); }}\n");
    let found = run(&[("src/server.rs", &two), ("tests/endings.rs", TEST_FILE)], GOOD_REGISTER, "");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("2") && found[0].contains("needs its own review"), "{}", found[0]);
}

#[test]
fn an_ending_test_that_does_not_exist_or_never_names_the_endpoint_fails() {
    let missing = run(&[("src/server.rs", SERVER)], GOOD_REGISTER, "");
    assert!(
        missing
            .iter()
            .any(|f| f.contains("tests/endings.rs") && f.contains("does not exist")),
        "{missing:?}"
    );
    let no_fn = run(&[("src/server.rs", SERVER), ("tests/endings.rs", "// \"ep\"\n")], GOOD_REGISTER, "");
    assert!(no_fn.iter().any(|f| f.contains("no `fn the_ending_test`")), "{no_fn:?}");
    let unnamed = run(
        &[
            ("src/server.rs", SERVER),
            ("tests/endings.rs", "#[test]\nfn the_ending_test() {}\n"),
        ],
        GOOD_REGISTER,
        "",
    );
    assert!(unnamed.iter().any(|f| f.contains("never names the endpoint id")), "{unnamed:?}");
}

#[test]
fn out_of_scope_needs_a_reason_an_owner_and_a_site() {
    let register = r#"
[[out_of_scope]]
path = "src/server.rs"
reason = "a fake"
owner = "me"

[[out_of_scope]]
path = "src/gone.rs"
reason = "stale"
owner = "me"

[[out_of_scope]]
path = "src/server.rs"
reason = " "
owner = ""
"#;
    let found = run(&[("src/server.rs", SERVER)], register, "");
    assert!(
        found
            .iter()
            .any(|f| f.contains("src/gone.rs") && f.contains("matches no listen site")),
        "{found:?}"
    );
    assert!(found.iter().any(|f| f.contains("needs path, reason and owner")), "{found:?}");
}

#[test]
fn the_ratchet_is_checked_for_shape() {
    let ratchet = r#"
[[ratchet]]
instance = "I11"
endpoint = "ep"
scenario = "listener-stop-with-client-in-backlog"
fault = "no-reason"
blocks_tag = "dyno-v*"

[[ratchet]]
instance = "I1"
endpoint = "ghost"
scenario = "nonsense"
fault = "no-reason"
blocks_tag = "none"
os = "beos"

[[ratchet]]
instance = "WP-7"
endpoint = "ep"
scenario = "*"
fault = "no-end-line"
blocks_tag = "none"
"#;
    let found = run(
        &[("src/server.rs", SERVER), ("tests/endings.rs", TEST_FILE)],
        GOOD_REGISTER,
        ratchet,
    );
    let text = found.join("\n");
    assert!(text.contains("the endpoint is not in"), "{text}");
    assert!(text.contains("unknown scenario"), "{text}");
    assert!(text.contains("only a logging gap may say `none`"), "{text}");
    assert!(text.contains("unknown os `beos`"), "{text}");
    assert_eq!(found.len(), 4, "the good entries produce nothing: {text}");
}

#[test]
fn a_tag_is_refused_while_an_entry_that_blocks_it_remains() {
    let ratchet = parse_ratchet(
        r#"
[[ratchet]]
instance = "I1"
endpoint = "ipc-host"
scenario = "stop-with-open-connection"
fault = "no-reason"
blocks_tag = "dyno-v*"

[[ratchet]]
instance = "I24"
endpoint = "pitcrew-service"
scenario = "malformed-first-frame"
fault = "no-reason"
blocks_tag = "pitcrew-v*"

[[ratchet]]
instance = "WP-7"
endpoint = "ipc-host"
scenario = "*"
fault = "no-end-line"
blocks_tag = "none"
"#,
    )
    .unwrap();
    let dyno = blocked_by(&ratchet, "dyno-v1.2.0");
    assert_eq!(dyno.iter().map(|e| e.instance.as_str()).collect::<Vec<_>>(), ["I1"]);
    let pitcrew = blocked_by(&ratchet, "pitcrew-v1.1.0");
    assert_eq!(pitcrew.iter().map(|e| e.instance.as_str()).collect::<Vec<_>>(), ["I24"]);
    assert!(blocked_by(&ratchet, "pitwall-v1.1.0").is_empty(), "other apps are not blocked");
    assert!(
        blocked_by(&[] as &[RatchetEntry], "dyno-v1.2.0").is_empty(),
        "an empty ratchet blocks nothing"
    );
    assert!(tag_matches("dyno-v*", "dyno-v1.1.0") && !tag_matches("dyno-v*", "dyno"), "the glob");
    assert!(tag_matches("a*b*c", "aXbYc") && !tag_matches("a*b*c", "aXcYb"));
}

#[test]
fn the_cli_refuses_a_blocked_tag_and_accepts_the_others() {
    let ratchet = r#"
[[ratchet]]
instance = "I1"
endpoint = "ep"
scenario = "stop-with-open-connection"
fault = "no-reason"
blocks_tag = "dyno-v*"
"#;
    let dir = repo(
        &[("src/server.rs", SERVER), ("tests/endings.rs", TEST_FILE)],
        GOOD_REGISTER,
        ratchet,
    );
    let dev = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
            .args(["check", "ipc-endpoints", "--root"])
            .arg(dir.path())
            .args(args)
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let (code, out, _) = dev(&[]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("1 listen site"), "{out}");
    let (code, _, err) = dev(&["--tag", "pitcrew-v1.1.0"]);
    assert_eq!(code, 0, "{err}");
    let (code, _, err) = dev(&["--tag", "dyno-v1.2.0"]);
    assert_ne!(code, 0);
    assert!(
        err.contains("::error::C102 the tag dyno-v1.2.0 is refused") && err.contains("I1"),
        "{err}"
    );
}

/// The scenario and fault names the check accepts are the harness's own.
#[test]
fn the_check_and_the_harness_agree_on_names() {
    let source = std::fs::read_to_string(repo_root().join("crates/paddock-ipc/src/testing/endings/mod.rs")).unwrap();
    let ratchet_src = parse_ratchet(&std::fs::read_to_string(repo_root().join(RATCHET)).unwrap()).unwrap();
    for name in SCENARIOS.iter().chain(FAULTS.iter()) {
        assert!(
            source.contains(&format!("\"{name}\"")),
            "the check knows `{name}`, which the harness does not report"
        );
    }
    for e in ratchet_src.iter().filter(|e| e.scenario != "*") {
        assert!(
            source.contains(&format!("\"{}\"", e.scenario)),
            "the harness has no scenario {}",
            e.scenario
        );
    }
    for e in &ratchet_src {
        assert!(
            source.contains(&format!("\"{}\"", e.fault)),
            "the harness has no fault kind {}",
            e.fault
        );
    }
}

/// The repository's own register and ratchet, scanned against the repository's own sources: the push check in a test, so that
/// CI runs it on every OS. It needs the whole tree (a sparse checkout without `apps/` cannot hold the register's files).
#[test]
fn the_repository_register_is_complete_and_the_ratchet_is_well_formed() {
    let (sites, register, _ratchet, found) = check(&repo_root()).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        found.is_empty(),
        "C102 findings (run `./dev check ipc-endpoints`):\n{}",
        found.join("\n")
    );
    assert!(
        !sites.is_empty() && !register.endpoints.is_empty(),
        "the scan found the repository's listeners"
    );
}
