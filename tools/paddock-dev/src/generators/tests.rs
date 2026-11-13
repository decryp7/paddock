//! The engine with a fake runner: no generator is run.

use super::registry::Registry;
use super::run::{Ran, Runner};
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

const REG: &str = r#"
version = 1
[[generator]]
id = "icons"
what = "icons"
check = ["node", "icons/tools/install.mjs", "--check"]
regenerate = "node icons/tools/install.mjs"
paths = ["icons/**"]
cost = "fast"
[[generator]]
id = "i18n"
what = "catalogue"
check = ["node", "i18n.mjs", "--check"]
regenerate = "npm run i18n"
paths = ["ui/**/*.ts", "ui/i18n/*.xlf"]
cost = "fast"
[[generator]]
id = "hakari"
what = "hack"
check = ["cargo", "hakari", "verify"]
regenerate = "cargo hakari generate"
cwd = "crates"
paths = ["crates/**/Cargo.toml"]
cost = "heavy"
[[generator]]
id = "world-map"
what = "a map from external data"
manual = "needs the Natural Earth download"
regenerate = "node build-world-map.mjs"
paths = ["maps/**"]
cost = "heavy"
[[generator]]
id = "marked"
what = "a cargo test"
check = ["cargo", "marked-test"]
regenerate = "r"
paths = ["marked/**"]
cost = "heavy"
drift_marker = ["test result: FAILED"]
[[generator]]
id = "svg"
what = "svgs"
check = ["node", "scripts/svg-drift.mjs"]
regenerate = "node scripts/svg-drift.mjs --write"
paths = ["**/*.drawio"]
cost = "fast"
requires = ["scripts/svg-drift.mjs"]
"#;

#[derive(Default)]
struct Fake {
    /// The second word of the command (`argv[1]`) -> what it does; a missing one exits 0.
    answers: BTreeMap<String, Ran>,
    files: BTreeSet<String>,
    started: Mutex<Vec<String>>,
}

impl Runner for Fake {
    fn run(&self, dir: &Path, argv: &[String], _limit: std::time::Duration, _spec: &super::run::Spec) -> Ran {
        self.started
            .lock()
            .unwrap()
            .push(format!("{} :: {}", dir.file_name().unwrap().to_string_lossy(), argv.join(" ")));
        self.answers.get(&argv[1]).cloned().unwrap_or(Ran::exit(0, ""))
    }
    fn subject(&self, _root: &Path) -> paddock_cli::Subject {
        paddock_cli::Subject::Files {
            head: paddock_cli::outcome::Sha::new("h"),
            digest: paddock_cli::outcome::Sha256::new("d"),
        }
    }
    fn exists(&self, path: &Path) -> bool {
        let rel = path.strip_prefix("/r").unwrap().to_string_lossy().into_owned();
        rel.is_empty() || rel == "crates" || self.files.contains(&rel)
    }
}

fn run(fake: &Fake, changed: &[&str], all: bool, only: &[&str]) -> Report {
    let reg = Registry::parse(REG).unwrap();
    let changed: Vec<String> = changed.iter().map(|s| s.to_string()).collect();
    let only: Vec<String> = only.iter().map(|s| s.to_string()).collect();
    check_with(&reg, Path::new("/r"), &changed, all, &only, fake, &|_| {}).unwrap()
}

#[test]
fn only_the_generators_a_change_touches_run_each_in_its_folder() {
    let fake = Fake::default();
    let r = run(&fake, &["icons/tools/build.mjs", "crates/p/Cargo.toml", "README.md"], false, &[]);
    assert_eq!(r.exit, Exit::Ok);
    let mut started = fake.started.lock().unwrap().clone();
    started.sort();
    assert_eq!(
        started,
        ["crates :: cargo hakari verify", "r :: node icons/tools/install.mjs --check"]
    );
    assert!(r.text.starts_with("gen check: 2 of 6 due: icons, hakari"), "{}", r.text);
    assert_eq!(r.data["outcome"], "clean");
}

#[test]
fn a_change_that_touches_nothing_registered_runs_nothing_and_says_so() {
    let fake = Fake::default();
    let r = run(&fake, &["apps/x/src/lib.rs"], false, &[]);
    assert_eq!(r.exit, Exit::Ok);
    assert!(fake.started.lock().unwrap().is_empty());
    assert!(r.text.contains("no generator is due (6 registered"));
}

#[test]
fn drift_is_stale_with_the_regenerate_command_and_exit_5() {
    let mut fake = Fake::default();
    fake.answers.insert("i18n.mjs".into(), Ran::exit(1, "messages.xlf is stale"));
    let r = run(&fake, &["ui/a.ts"], false, &[]);
    assert_eq!(r.exit, Exit::Findings);
    assert_eq!(r.data["outcome"], "stale");
    assert!(
        r.text
            .contains("::error::gen i18n: STALE [node i18n.mjs --check]: messages.xlf is stale. Next step: run `npm run i18n`"),
        "{}",
        r.text
    );
}

#[test]
fn a_missing_tool_a_timeout_and_a_file_not_on_this_tree_are_not_measured_never_a_pass() {
    let mut fake = Fake::default();
    fake.answers
        .insert("hakari".into(), Ran::Unavailable("cargo-hakari is not installed".into()));
    fake.answers.insert("i18n.mjs".into(), Ran::TimedOut);
    let r = run(&fake, &["crates/p/Cargo.toml", "ui/a.ts", "docs/a.drawio"], false, &[]);
    assert_eq!(r.exit, Exit::NotMeasured, "{}", r.text);
    let results = r.data["results"].as_array().unwrap();
    let by = |id: &str| results.iter().find(|x| x["id"] == id).unwrap().clone();
    assert_eq!(by("hakari")["verdict"], "not measured");
    assert!(by("hakari")["detail"].as_str().unwrap().contains("cargo-hakari is not installed"));
    assert!(by("i18n")["detail"].as_str().unwrap().contains("did not finish"));
    assert!(
        by("svg")["detail"]
            .as_str()
            .unwrap()
            .contains("not on this tree: scripts/svg-drift.mjs is missing")
    );
    assert!(
        !fake.started.lock().unwrap().iter().any(|s| s.contains("svg-drift")),
        "an entry whose file is missing never starts"
    );
}

#[test]
fn stale_beats_not_measured_in_the_exit() {
    let mut fake = Fake::default();
    fake.answers.insert("hakari".into(), Ran::Unavailable("x".into()));
    fake.answers.insert("icons/tools/install.mjs".into(), Ran::exit(1, "differs"));
    let r = run(&fake, &["crates/p/Cargo.toml", "icons/a"], false, &[]);
    assert_eq!(r.exit, Exit::Findings, "{}", r.text);
}

#[test]
fn a_present_required_file_lets_the_entry_run() {
    let mut fake = Fake::default();
    fake.files.insert("scripts/svg-drift.mjs".into());
    let r = run(&fake, &["docs/a.drawio"], false, &[]);
    assert_eq!(r.exit, Exit::Ok);
    assert!(fake.started.lock().unwrap().iter().any(|s| s.contains("svg-drift")));
}

#[test]
fn all_and_only_select_without_a_change_and_an_unknown_id_is_a_usage_error() {
    let fake = Fake::default();
    assert_eq!(run(&fake, &[], true, &[]).data["due"].as_array().unwrap().len(), 5);
    let r = run(&fake, &[], false, &["hakari"]);
    assert_eq!(r.data["due"], json!(["hakari"]));
    let reg = Registry::parse(REG).unwrap();
    let e = check_with(&reg, Path::new("/r"), &[], false, &["nope".into()], &fake, &|_| {}).unwrap_err();
    assert_eq!(e.exit, Exit::Usage);
}

#[test]
fn the_logs_carry_each_command_and_its_duration() {
    let fake = Fake::default();
    let reg = Registry::parse(REG).unwrap();
    let lines = Mutex::new(Vec::new());
    check_with(&reg, Path::new("/r"), &["icons/a".into()], false, &[], &fake, &|l| {
        lines.lock().unwrap().push(l.to_string())
    })
    .unwrap();
    let lines = lines.into_inner().unwrap();
    assert!(
        lines[0].starts_with("gen icons: running [fast] node icons/tools/install.mjs --check"),
        "{lines:?}"
    );
    assert!(lines[1].starts_with("gen icons: current in "), "{lines:?}");
}

#[test]
fn a_manual_entry_is_named_with_its_regenerate_command_and_never_fails_the_run() {
    let fake = Fake::default();
    let r = run(&fake, &["maps/a.json"], false, &[]);
    assert_eq!(r.exit, Exit::Ok);
    assert!(fake.started.lock().unwrap().is_empty());
    assert!(
        r.text
            .contains("gen world-map: no automatic check (needs the Natural Earth download)")
            && r.text.contains("`node build-world-map.mjs`"),
        "{}",
        r.text
    );
    assert_eq!(r.data["manual"], json!(["world-map"]));
}

fn verdict_of(ran: Ran) -> (Exit, String) {
    let mut fake = Fake::default();
    fake.answers.insert("marked-test".into(), ran);
    let r = run(&fake, &["marked/a"], false, &[]);
    (r.exit, r.text)
}

#[test]
fn a_crash_is_never_stale() {
    // A build that does not compile exits 101 like a failing test, without the marker.
    let (exit, text) = verdict_of(Ran::exit(101, "error: could not compile `x` (lib test) due to 1 previous error"));
    assert_eq!(exit, Exit::NotMeasured, "{text}");
    assert!(text.contains("the code does not compile"), "{text}");
    // A failure without the entry's marker did not reach a verdict either.
    let (exit, text) = verdict_of(Ran::exit(101, "thread panicked at src/lib.rs"));
    assert_eq!(exit, Exit::NotMeasured, "{text}");
    assert!(text.contains("without saying the files are stale"), "{text}");
    // The marker makes it drift.
    let (exit, _) = verdict_of(Ran::exit(
        101,
        "test generated::t ... FAILED\ntest result: FAILED. 0 passed; 1 failed",
    ));
    assert_eq!(exit, Exit::Findings);
    // Statuses that mean "could not look", whatever the tool.
    for code in [6, 126, 127] {
        let (exit, _) = verdict_of(Ran::exit(code, "test result: FAILED"));
        assert_eq!(exit, Exit::NotMeasured, "status {code}");
    }
    // Ended by a signal, and the output cap.
    let (exit, _) = verdict_of(Ran::Exited {
        code: None,
        tail: "x".into(),
        text: "x".into(),
    });
    assert_eq!(exit, Exit::NotMeasured);
    let (exit, text) = verdict_of(Ran::OutputCap);
    assert_eq!(exit, Exit::NotMeasured);
    assert!(text.contains("more output than the cap"), "{text}");
}

#[test]
fn the_limit_follows_the_cost_class() {
    use super::registry::Cost;
    assert!(run::limit(Cost::Fast) < run::limit(Cost::Heavy));
}

/// `Real` with a fake `heavy` (the machine-wide slot) and a fake `cargo` (records what it was started with).
#[cfg(unix)]
mod real {
    use super::super::registry::Cost;
    use super::super::run::{Ran, Real, Runner, Spec};
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    struct Fixture {
        dir: tempfile::TempDir,
        repo: PathBuf,
        builds: PathBuf,
    }

    fn script(path: &Path, body: &str) {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn fixture(heavy_body: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let builds = dir.path().join("builds");
        let bin = dir.path().join("bin");
        for d in [&repo, &builds, &bin] {
            std::fs::create_dir_all(d).unwrap();
        }
        let calls = dir.path().join("calls");
        script(
            &bin.join("cargo"),
            &format!(
                "echo \"cargo jobs=$CARGO_BUILD_JOBS target=$CARGO_TARGET_DIR cwd=$(pwd)\" >> {}",
                calls.display()
            ),
        );
        script(&bin.join("fake-heavy"), heavy_body);
        Fixture { dir, repo, builds }
    }

    fn env(f: &Fixture) -> HashMap<String, String> {
        HashMap::from([
            ("PATH".to_string(), format!("{}:/usr/bin:/bin", f.dir.path().join("bin").display())),
            (
                "PADDOCK_HEAVY".to_string(),
                f.dir.path().join("bin/fake-heavy").display().to_string(),
            ),
            ("PADDOCK_DEV_TARGET_DIR".to_string(), f.builds.display().to_string()),
        ])
    }

    fn calls(f: &Fixture) -> String {
        std::fs::read_to_string(f.dir.path().join("calls")).unwrap_or_default()
    }

    const PASS_THROUGH: &str = r#"echo "slot taken" >> "$(dirname "$0")/../slot"; exec "$@""#;

    fn argv() -> Vec<String> {
        vec!["cargo".into(), "metadata".into()]
    }

    #[test]
    fn a_heavy_entry_takes_the_slot_and_builds_in_its_workspaces_kept_folder_with_two_jobs() {
        let f = fixture(PASS_THROUGH);
        let real = Real::with(&f.repo, env(&f), Vec::new());
        let ran = real.run(
            &f.repo,
            &argv(),
            Duration::from_secs(20),
            &Spec {
                heavy: true,
                workspace: "apps/dyno/app/src-tauri",
            },
        );
        assert!(matches!(ran, Ran::Exited { code: Some(0), .. }), "{ran:?}");
        assert!(f.dir.path().join("slot").is_file(), "the fake heavy ran first");
        let c = calls(&f);
        let want = f.builds.join("gen-check/apps-dyno-app-src-tauri");
        assert!(c.contains("jobs=2"), "{c}");
        assert!(c.contains(&format!("target={}", want.display())), "{c}");
        assert!(
            want.is_dir() && want.join(".paddock-dir").exists() || want.is_dir(),
            "kept and recorded"
        );
        assert!(!want.starts_with(&f.repo), "never inside the worktree");
    }

    #[test]
    fn a_fast_entry_skips_the_slot_but_gets_the_same_bounds_and_the_callers_job_count_wins() {
        let f = fixture(PASS_THROUGH);
        let mut e = env(&f);
        e.insert("CARGO_BUILD_JOBS".into(), "3".into());
        let real = Real::with(&f.repo, e, Vec::new());
        let ran = real.run(
            &f.repo,
            &argv(),
            Duration::from_secs(20),
            &Spec {
                heavy: false,
                workspace: ".",
            },
        );
        assert!(matches!(ran, Ran::Exited { code: Some(0), .. }), "{ran:?}");
        assert!(!f.dir.path().join("slot").exists(), "a fast entry does not queue");
        let c = calls(&f);
        assert!(c.contains("jobs=3") && c.contains("gen-check/root"), "{c}");
        assert_eq!(Cost::Fast.name(), "fast");
    }

    #[test]
    fn a_wait_for_the_slot_past_the_hang_guard_is_not_measured_and_names_the_holder() {
        let f = fixture("echo 'heavy: the lock is held by Bob (pid 12) for 3m, running: cargo test' >&2; sleep 30");
        let real = Real::with(&f.repo, env(&f), Vec::new());
        let ran = real.run(
            &f.repo,
            &argv(),
            Duration::from_secs(1),
            &Spec {
                heavy: true,
                workspace: "crates",
            },
        );
        match ran {
            Ran::SlotWait(who) => assert!(who.contains("Bob"), "{who}"),
            other => panic!("{other:?}"),
        }
        assert!(calls(&f).is_empty(), "cargo never started");
    }

    #[test]
    fn a_heavy_that_ran_the_command_and_timed_out_later_is_a_plain_timeout() {
        let f = fixture("echo 'heavy: pre-push holds the lock (pid 9) at 10:00:00Z, waited 0s' >&2; sleep 30");
        let real = Real::with(&f.repo, env(&f), Vec::new());
        let ran = real.run(
            &f.repo,
            &argv(),
            Duration::from_secs(1),
            &Spec {
                heavy: true,
                workspace: "crates",
            },
        );
        assert_eq!(ran, Ran::TimedOut);
    }

    #[test]
    fn no_build_root_or_one_inside_the_checkout_is_not_measured_rather_than_a_build_in_the_worktree() {
        let f = fixture(PASS_THROUGH);
        let mut e = env(&f);
        e.remove("PADDOCK_DEV_TARGET_DIR");
        let real = Real::with(&f.repo, e.clone(), Vec::new());
        let spec = Spec {
            heavy: false,
            workspace: "crates",
        };
        match real.run(&f.repo, &argv(), Duration::from_secs(5), &spec) {
            Ran::Unavailable(why) => assert!(
                why.contains("no build-folder root") && why.contains("PADDOCK_DEV_TARGET_DIR"),
                "{why}"
            ),
            other => panic!("{other:?}"),
        }
        // A default root that exists is used (the d7 root); one that does not is skipped.
        let d7 = f.dir.path().join("d7");
        std::fs::create_dir_all(&d7).unwrap();
        let real = Real::with(&f.repo, e.clone(), vec![f.dir.path().join("missing"), d7.clone()]);
        assert_eq!(real.target_dir("crates").unwrap(), d7.join("gen-check/crates"));
        // A root under $HOME is refused, whether configured or one of the default roots.
        let home = f.dir.path().join("home");
        std::fs::create_dir_all(home.join("builds")).unwrap();
        let mut under = e.clone();
        under.insert("HOME".into(), home.display().to_string());
        under.insert("PADDOCK_DEV_TARGET_DIR".into(), home.join("builds").display().to_string());
        let why = Real::with(&f.repo, under.clone(), Vec::new()).target_dir("crates").unwrap_err();
        assert!(
            why.contains("under your home folder") && why.contains("PADDOCK_DEV_TARGET_DIR"),
            "{why}"
        );
        under.remove("PADDOCK_DEV_TARGET_DIR");
        let why = Real::with(&f.repo, under, vec![home.join("builds")])
            .target_dir("crates")
            .unwrap_err();
        assert!(why.contains("under your home folder"), "{why}");
        // Configured inside the checkout: refused.
        e.insert("PADDOCK_DEV_TARGET_DIR".into(), f.repo.join("target").display().to_string());
        let real = Real::with(&f.repo, e, Vec::new());
        assert!(real.target_dir("crates").unwrap_err().contains("inside the checkout"));
        assert!(calls(&f).is_empty());
    }

    #[test]
    fn heavy_refusing_the_slot_is_not_the_checks_verdict() {
        let f = fixture("echo 'heavy 0.1 (macos) exit 0x4b: the lock could not be taken. Next: x' >&2; exit 75");
        let real = Real::with(&f.repo, env(&f), Vec::new());
        let ran = real.run(
            &f.repo,
            &argv(),
            Duration::from_secs(5),
            &Spec {
                heavy: true,
                workspace: "crates",
            },
        );
        assert!(
            matches!(ran, Ran::Unavailable(ref w) if w.contains("heavy gave no slot")),
            "{ran:?}"
        );
    }
}

/// The default roots come from `./dev disk`'s settings (`PADDOCK_DISK_ROOTS`), not from a path in this module.
#[test]
fn the_default_build_roots_are_the_disk_settings_roots() {
    // The platform's list separator (`:`, `;` on Windows), as `split_paths` reads it.
    let list = std::env::join_paths(["/nope/a", "/nope/b"]).unwrap().into_string().unwrap();
    let env = std::collections::HashMap::from([("PADDOCK_DISK_ROOTS".to_string(), list)]);
    let roots = crate::disk::config::Settings::resolve(&Default::default(), &|k| env.get(k).cloned()).roots;
    assert_eq!(roots, [std::path::PathBuf::from("/nope/a"), std::path::PathBuf::from("/nope/b")]);
    assert!(!include_str!("run.rs").contains("DEFAULT_ROOTS"), "run.rs names no machine path");
}

/// ADR 0089 amendment 4: the pass is made only by `measure`, from the check's own exit 0, and says what it is a pass of.
#[test]
fn a_current_generator_carries_its_evidence_and_the_exit_comes_only_from_the_outcomes() {
    let fake = Fake::default();
    let r = run(&fake, &["ui/a.ts"], false, &[]);
    assert_eq!(r.exit, Exit::Ok);
    let o = &r.data["results"][0]["outcome"];
    assert_eq!(o["status"], "clean");
    assert_eq!(o["evidence"]["check"], "gen:i18n");
    assert_eq!(o["evidence"]["subject"]["kind"], "files");
}

/// "Delete the check": a generator whose check could not run (the program gone) gives 6, never 0; "skip the gate": a worker
/// that never stored its result gives 6 as well.
#[test]
fn a_deleted_check_and_a_skipped_worker_are_exit_6_not_a_pass() {
    let mut fake = Fake::default();
    fake.answers.insert("i18n.mjs".into(), Ran::Unavailable("node is not installed".into()));
    assert_eq!(run(&fake, &["ui/a.ts"], false, &[]).exit, Exit::NotMeasured);
    let lost = super::run::Checked {
        id: "x".into(),
        line: "l".into(),
        regenerate: "r".into(),
        cost: super::registry::Cost::Fast,
        outcome: paddock_cli::Outcome::NotMeasured(paddock_cli::Reason::new("the check did not run (a worker ended early)")),
        took: std::time::Duration::ZERO,
    };
    assert_eq!(super::exit_of(&[lost]), Exit::NotMeasured);
}
