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
    fn run(&self, dir: &Path, argv: &[String]) -> Ran {
        self.started
            .lock()
            .unwrap()
            .push(format!("{} :: {}", dir.file_name().unwrap().to_string_lossy(), argv.join(" ")));
        self.answers.get(&argv[1]).cloned().unwrap_or(Ran::Exited {
            ok: true,
            tail: String::new(),
        })
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
    assert!(r.text.starts_with("gen check: 2 of 5 due: icons, hakari"), "{}", r.text);
    assert_eq!(r.data["outcome"], "clean");
}

#[test]
fn a_change_that_touches_nothing_registered_runs_nothing_and_says_so() {
    let fake = Fake::default();
    let r = run(&fake, &["apps/x/src/lib.rs"], false, &[]);
    assert_eq!(r.exit, Exit::Ok);
    assert!(fake.started.lock().unwrap().is_empty());
    assert!(r.text.contains("no generator is due (5 registered"));
}

#[test]
fn drift_is_stale_with_the_regenerate_command_and_exit_5() {
    let mut fake = Fake::default();
    fake.answers.insert(
        "i18n.mjs".into(),
        Ran::Exited {
            ok: false,
            tail: "messages.xlf is stale".into(),
        },
    );
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
    fake.answers.insert(
        "icons/tools/install.mjs".into(),
        Ran::Exited {
            ok: false,
            tail: "differs".into(),
        },
    );
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
    assert_eq!(run(&fake, &[], true, &[]).data["due"].as_array().unwrap().len(), 4);
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
