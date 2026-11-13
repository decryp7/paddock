//! The Windows twin of `ci_isolation.rs`: the commands that need no slot holder (`path`, `port`, `source-key`) as a real process
//! on a Windows host, where the Unix file does not run (docs/plans/ci-in-rust.md section 7, ER-9). The CI home is a scratch
//! folder (`PADDOCK_CI_HOME`), never the runner's own. A drive path, a Windows path with backslashes and a Git-bash path name one
//! folder and so one key.
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::{Command, Output};

#[allow(clippy::disallowed_methods)]
fn run(home: &Path, tables: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["ci", "isolation"])
        .args(args)
        .env_clear()
        .env("SystemRoot", std::env::var("SystemRoot").unwrap_or_default())
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("RUNNER_OS", "Windows")
        .env("PADDOCK_CI_HOME", home)
        .env("PADDOCK_CI_TABLES", tables)
        .env("PADDOCK_CI_SLOT", "s2")
        .env("PADDOCK_CI_SLOTS", "3")
        .output()
        .unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).replace("\r\n", "\n")
}

fn scratch() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let home = paddock_env::winpath::real(d.path()).unwrap().join("ci");
    let tables = d.path().join("tables");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&tables).unwrap();
    std::fs::write(
        tables.join("kept-folders.txt"),
        "target target-{name}-{src}-{slot} slot\ntools tools shared\n",
    )
    .unwrap();
    std::fs::write(tables.join("ports.txt"), "sccache 4227 1 1\n").unwrap();
    (d, home, tables)
}

#[test]
fn a_slots_port_and_kept_folder_on_a_windows_host() {
    let (_d, home, tables) = scratch();
    let port = run(&home, &tables, &["port", "sccache"]);
    assert!(port.status.success(), "{}", text(&port.stderr));
    assert_eq!(text(&port.stdout).trim(), "4228", "slot s2 takes the second port of the block");
    let tools = run(&home, &tables, &["path", "tools"]);
    assert!(tools.status.success(), "{}", text(&tools.stderr));
    let shown = paddock_testkit::path::text(Path::new(text(&tools.stdout).trim()));
    assert!(shown.ends_with("/ci/tools"), "{shown}");
    assert!(Path::new(shown.as_str()).is_dir());
}

#[test]
fn one_folder_has_one_key_however_it_is_spelled() {
    let (d, _home, _tables) = scratch();
    let (home, tables) = (d.path().join("ci"), d.path().join("tables"));
    let work = paddock_env::winpath::real(d.path()).unwrap().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let plain = work.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let forward = paddock_testkit::path::text(&work);
    let (drive, rest) = forward.split_at(2);
    let git_bash = format!("/{}{rest}", drive[..1].to_lowercase());
    let key = |p: &str| {
        let r = run(&home, &tables, &["source-key", p]);
        assert!(r.status.success(), "{p}: {}", text(&r.stderr));
        text(&r.stdout).trim().to_string()
    };
    let k = key(&plain);
    assert_eq!(k.len(), 10, "{k}");
    assert_eq!(key(&forward), k, "forward slashes");
    assert_eq!(key(&git_bash), k, "a Git-bash spelling ({git_bash})");
}
