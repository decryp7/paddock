//! `./dev locks check|sync`: the plan from tracked files, the order and the findings with a fake runner, and the real cargo and
//! npm on temporary workspaces (no dependencies from a registry, so nothing needs the network).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_cli::Exit;
use paddock_dev::locks::{Kind, Ran, Runner, check_all, plan, skipped, sync_all};
use paddock_testkit::TestCommand;
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

fn files(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn the_plan_names_every_lockfile_that_has_its_manifest_and_the_hack_config() {
    let t = plan(&files(&[
        "Cargo.lock", // no Cargo.toml beside it: not a workspace, left out
        "package-lock.json",
        "crates/Cargo.toml",
        "crates/Cargo.lock",
        "crates/.config/hakari.toml",
        "tools/paddock-dev/tests/golden/fixture/apps/x/Cargo.toml", // test data, not a workspace
        "tools/paddock-dev/tests/golden/fixture/apps/x/Cargo.lock",
        "apps/dyno/tests/Cargo.toml", // a folder named tests is not test data: it is checked
        "apps/dyno/tests/Cargo.lock",
        "apps/dyno/app/src-tauri/Cargo.toml",
        "apps/dyno/app/src-tauri/Cargo.lock",
    ]));
    let kinds: Vec<(Kind, String)> = t.iter().map(|t| (t.kind, t.file.clone())).collect();
    assert!(kinds.contains(&(Kind::Npm, "package-lock.json".into())));
    assert!(kinds.contains(&(Kind::Cargo, "crates/Cargo.lock".into())));
    assert!(kinds.contains(&(Kind::Cargo, "apps/dyno/app/src-tauri/Cargo.lock".into())));
    assert!(kinds.contains(&(Kind::Hakari, "crates/.config/hakari.toml".into())));
    assert!(!kinds.contains(&(Kind::Cargo, "Cargo.lock".into())), "{kinds:?}");
    assert!(kinds.iter().all(|(_, f)| !f.contains("/fixture/")), "{kinds:?}");
    assert!(kinds.contains(&(Kind::Cargo, "apps/dyno/tests/Cargo.lock".into())), "{kinds:?}");
    let hack = t.iter().find(|t| t.kind == Kind::Hakari).unwrap();
    assert_eq!(hack.dir, Path::new("crates"), "hakari runs in the workspace, not in .config");
    assert_eq!(hack.check.len(), 3, "generate --diff, manage-deps --dry-run, verify (what CI runs)");
    assert_eq!(
        hack.fix_text(),
        "cd crates && cargo hakari manage-deps --yes && cargo hakari generate"
    );
}

/// Fails the commands whose text contains one of `fail`; records every command it was asked to run.
struct Fake {
    fail: Vec<&'static str>,
    seen: Mutex<Vec<String>>,
}

impl Runner for Fake {
    fn run(&self, dir: &Path, argv: &[String]) -> Ran {
        let line = format!("{} :: {}", dir.file_name().unwrap().to_string_lossy(), argv.join(" "));
        self.seen.lock().unwrap().push(line.clone());
        let ok = !self.fail.iter().any(|f| line.contains(f));
        Ran {
            ok,
            tail: if ok { String::new() } else { "the hack is out of date".into() },
        }
    }
}

#[test]
fn a_stale_hack_is_one_finding_that_names_the_failed_command_and_the_fix() {
    let targets = plan(&files(&["crates/Cargo.toml", "crates/Cargo.lock", "crates/.config/hakari.toml"]));
    let fake = Fake {
        fail: vec!["hakari generate --diff"],
        seen: Mutex::new(vec![]),
    };
    let stale = check_all(&fake, Path::new("/r"), &targets);
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].target.kind, Kind::Hakari);
    assert_eq!(stale[0].command, "cargo hakari generate --diff");
    // The remaining hakari checks do not run once one has said stale, and the lockfile was still checked.
    let seen = fake.seen.lock().unwrap().join("\n");
    assert!(seen.contains("cargo metadata --locked"), "{seen}");
    assert!(!seen.contains("hakari verify"), "{seen}");
}

#[test]
fn sync_regenerates_the_hack_before_the_lockfiles_and_npm_last() {
    let targets = plan(&files(&[
        "package-lock.json",
        "crates/Cargo.toml",
        "crates/Cargo.lock",
        "crates/.config/hakari.toml",
    ]));
    let fake = Fake {
        fail: vec![],
        seen: Mutex::new(vec![]),
    };
    assert!(sync_all(&fake, Path::new("/r"), &targets).is_empty());
    let seen = fake.seen.lock().unwrap().clone();
    let at = |needle: &str| {
        seen.iter()
            .position(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("{needle} not run: {seen:?}"))
    };
    assert!(at("hakari manage-deps --yes") < at("hakari generate"));
    assert!(at("hakari generate") < at("cargo update --workspace"));
    assert!(at("cargo update --workspace") < at("npm install --package-lock-only"));
}

#[test]
fn a_failing_sync_step_is_reported_and_stops_that_target_only() {
    let targets = plan(&files(&["crates/Cargo.toml", "crates/Cargo.lock", "crates/.config/hakari.toml"]));
    let fake = Fake {
        fail: vec!["manage-deps"],
        seen: Mutex::new(vec![]),
    };
    let failed = sync_all(&fake, Path::new("/r"), &targets);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].command, "cargo hakari manage-deps --yes");
    let seen = fake.seen.lock().unwrap().join("\n");
    assert!(!seen.contains("hakari generate"), "{seen}");
    assert!(seen.contains("cargo update --workspace"), "the lockfile target still ran: {seen}");
}

// ---- the real tools on temporary workspaces -------------------------------------------------------------------------------

#[allow(clippy::disallowed_methods)] // git on a scratch repository
fn run(dir: &Path, program: &str, args: &[&str]) {
    let s = Command::new(program).args(args).current_dir(dir).output().unwrap();
    assert!(s.status.success(), "{program} {args:?}: {}", String::from_utf8_lossy(&s.stderr));
}

/// `./dev --json locks <sub>` in `root`, with cargo and npm kept away from the user's homes.
#[allow(clippy::disallowed_methods)] // the binary under test
fn dev(root: &Path, home: &Path, sub: &str) -> (i32, Value, String) {
    let out = TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["--json", "locks", sub, "--root"])
        .arg(root)
        .env("CARGO_HOME", home.join("cargo"))
        .env("npm_config_cache", home.join("npm"))
        .env("npm_config_audit", "false")
        .env("npm_config_fund", "false")
        .env("npm_config_update_notifier", "false")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    (
        out.status.code().unwrap(),
        serde_json::from_str(text.trim()).unwrap_or(Value::Null),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A repository with one cargo workspace (no dependencies) whose lock names the wrong version, and an npm workspace whose
/// lock lacks its only (local) dependency.
fn stale_repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    std::fs::create_dir_all(r.join("ws/src")).unwrap();
    std::fs::write(
        r.join("ws/Cargo.toml"),
        "[package]\nname = \"ws\"\nversion = \"0.2.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(r.join("ws/src/lib.rs"), "").unwrap();
    std::fs::write(
        r.join("ws/Cargo.lock"),
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"ws\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(r.join("local")).unwrap();
    std::fs::write(r.join("local/package.json"), "{\"name\":\"local\",\"version\":\"1.0.0\"}").unwrap();
    std::fs::write(
        r.join("package.json"),
        "{\"name\":\"root\",\"version\":\"1.0.0\",\"dependencies\":{\"local\":\"file:./local\"}}",
    )
    .unwrap();
    std::fs::write(r.join("package-lock.json"), "{\"name\":\"root\",\"version\":\"1.0.0\",\"lockfileVersion\":3,\"requires\":true,\"packages\":{\"\":{\"name\":\"root\",\"version\":\"1.0.0\"}}}").unwrap();
    run(r, "git", &["init", "-q"]);
    run(r, "git", &["add", "ws", "local", "package.json", "package-lock.json"]);
    d
}

#[test]
fn stale_cargo_and_npm_lockfiles_are_found_one_finding_each_and_sync_fixes_them() {
    let repo = stale_repo();
    let home = tempfile::tempdir().unwrap();
    let (code, v, err) = dev(repo.path(), home.path(), "check");
    assert_eq!(code, Exit::Findings as i32, "{v} {err}");
    let text = v["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("::error file=ws/Cargo.lock::"), "{text}");
    assert!(text.contains("::error file=package-lock.json::"), "{text}");
    assert!(text.contains("./dev locks sync"), "{text}");
    assert_eq!(text.matches("::error").count(), 2, "{text}");
    assert!(
        !text.contains("A complete log of this run"),
        "npm's log-path line hides the cause: {text}"
    );
    assert!(
        text.contains("in sync") || text.contains("Missing:"),
        "the finding names why npm refused: {text}"
    );

    let (code, v, err) = dev(repo.path(), home.path(), "sync");
    assert_eq!(code, 0, "{v} {err}");
    let lock = std::fs::read_to_string(repo.path().join("ws/Cargo.lock")).unwrap();
    assert!(lock.contains("version = \"0.2.0\""), "{lock}");

    let (code, v, err) = dev(repo.path(), home.path(), "check");
    assert_eq!(code, 0, "{v} {err}");
}

#[test]
fn a_folder_without_lockfiles_is_an_error_that_says_what_to_do() {
    let d = tempfile::tempdir().unwrap();
    run(d.path(), "git", &["init", "-q"]);
    let home = tempfile::tempdir().unwrap();
    let (code, v, err) = dev(d.path(), home.path(), "check");
    assert_ne!(code, 0, "{v}");
    assert!(format!("{v}{err}").contains("No lockfile found"), "{v} {err}");
}

#[test]
#[allow(clippy::disallowed_methods)] // git on this repository
fn the_real_tracked_list_skips_only_the_golden_fixture_and_every_other_lockfile_is_a_target() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    let out = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["ls-files", "--", "*Cargo.lock", "*Cargo.toml", "*package-lock.json", "*hakari.toml"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let tracked: Vec<String> = String::from_utf8(out.stdout).unwrap().lines().map(str::to_string).collect();
    let targets = plan(&tracked);
    let skipped = skipped(&tracked);
    assert!(
        !skipped.is_empty() && skipped.iter().all(|f| f.starts_with("tools/paddock-dev/tests/golden/fixture/")),
        "{skipped:?}"
    );
    let locks: Vec<&String> = tracked.iter().filter(|f| f.ends_with("Cargo.lock")).collect();
    for l in &locks {
        let is_target = targets.iter().any(|t| &t.file == *l);
        assert_eq!(
            is_target,
            !skipped.contains(l),
            "{l}: every tracked Cargo.lock is a target unless it was reported as skipped"
        );
    }
    assert!(targets.iter().any(|t| t.kind == Kind::Npm && t.file == "package-lock.json"));
    assert!(
        targets.iter().any(|t| t.kind == Kind::Hakari),
        "the crates workspace has a hack config"
    );
}
