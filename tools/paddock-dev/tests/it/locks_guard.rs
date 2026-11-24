//! `./dev locks guard` (the shared-crate dependency guard): its positive control (ER-33) is a fixture repository with a shared crate
//! workspace and an app workspace, built with the real cargo on path-only crates (no registry). Adding a dependency that forces a
//! feature on a watched crate turns the guard red in (b) and (c); a stale workspace hack turns (a) red; the wanted-change path
//! (`--write`) turns it green again. Removing the pin file or the watch list shows the control cannot pass by watching nothing.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_cli::Exit;
use paddock_dev::locks::{Ran, Runner};
use paddock_dev::locks_guard::{self, Mode, PINS_FILE, Part, RealMetadata, parse_pins};
use std::path::Path;
use std::process::Command;

#[allow(clippy::disallowed_methods)] // git and cargo on a scratch repository
fn sh(dir: &Path, program: &str, args: &[&str]) {
    let o = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("RUSTUP_AUTO_INSTALL", "0")
        .output()
        .unwrap();
    assert!(o.status.success(), "{program} {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn lib(root: &Path, krate: &str) {
    put(root, &format!("{krate}/src/lib.rs"), "");
}

/// A fake runner: passes everything, except the commands whose text contains one of `fail`.
struct Fake(Vec<&'static str>);

impl Runner for Fake {
    fn run(&self, dir: &Path, argv: &[String]) -> Ran {
        let line = format!("{} :: {}", dir.display(), argv.join(" "));
        let ok = !self.0.iter().any(|f| line.contains(f));
        Ran {
            ok,
            tail: if ok { String::new() } else { "the hack is out of date".into() },
        }
    }
}

/// `crates/` (shared crates `jsonlike` with a feature `ordered`, and `shared` that uses it) and `apps/x` (an app that uses both),
/// each its own workspace with its own lockfile, as in Paddock. The pins file watches `jsonlike`.
fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    put(
        r,
        "crates/Cargo.toml",
        "[workspace]\nmembers = [\"jsonlike\", \"shared\"]\nresolver = \"2\"\n",
    );
    put(
        r,
        "crates/jsonlike/Cargo.toml",
        "[package]\nname = \"jsonlike\"\nversion = \"1.0.0\"\nedition = \"2021\"\n[features]\ndefault = []\nordered = []\n",
    );
    lib(&r.join("crates"), "jsonlike");
    put(
        r,
        "crates/shared/Cargo.toml",
        "[package]\nname = \"shared\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\njsonlike = { path = \"../jsonlike\" }\n",
    );
    lib(&r.join("crates"), "shared");
    put(
        r,
        "apps/x/Cargo.toml",
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\nshared = { path = \"../../crates/shared\" }\n",
    );
    put(r, "apps/x/src/lib.rs", "");
    put(r, "crates/.config/hakari.toml", "hakari-package = \"paddock-workspace-hack\"\n");
    put(r, PINS_FILE, "watch = [\"jsonlike\"]\n");
    sh(r, "git", &["init", "-q"]);
    lock_all(r);
    d
}

fn lock_all(r: &Path) {
    for ws in ["crates", "apps/x"] {
        sh(&r.join(ws), "cargo", &["generate-lockfile", "--offline"]);
    }
    sh(r, "git", &["add", "-A"]);
}

/// Adds the shared crate `forcer`, which depends on `jsonlike` with its `ordered` feature: what oxc_resolver did to serde_json.
fn add_the_forcing_dependency(r: &Path) {
    put(
        r,
        "crates/forcer/Cargo.toml",
        "[package]\nname = \"forcer\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\njsonlike = { path = \"../jsonlike\", features = [\"ordered\"] }\n",
    );
    lib(&r.join("crates"), "forcer");
    let shared = std::fs::read_to_string(r.join("crates/shared/Cargo.toml")).unwrap();
    std::fs::write(
        r.join("crates/shared/Cargo.toml"),
        format!("{shared}forcer = {{ path = \"../forcer\" }}\n"),
    )
    .unwrap();
    let ws = std::fs::read_to_string(r.join("crates/Cargo.toml")).unwrap();
    std::fs::write(r.join("crates/Cargo.toml"), ws.replace("\"shared\"]", "\"shared\", \"forcer\"]")).unwrap();
}

fn measure(r: &Path, runner: &dyn Runner) -> Vec<locks_guard::Finding> {
    let files: Vec<String> = [
        "crates/Cargo.toml",
        "crates/Cargo.lock",
        "crates/.config/hakari.toml",
        "apps/x/Cargo.toml",
        "apps/x/Cargo.lock",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    let pins = std::fs::read_to_string(r.join(PINS_FILE))
        .map_err(|e| e.to_string())
        .and_then(|t| parse_pins(&t));
    locks_guard::measure(runner, &RealMetadata, r, &files, &pins).findings
}

fn run(r: &Path, runner: &dyn Runner, write: bool, mode: Mode) -> paddock_cli::Report {
    locks_guard::run(runner, &RealMetadata, r, write, mode).unwrap()
}

#[test]
fn graph_a_dependency_that_forces_a_feature_on_a_watched_crate_turns_the_guard_red_and_the_wanted_path_turns_it_green() {
    let d = repo();
    let r = d.path();
    // The tree as committed: pin what Cargo resolves, then nothing is found.
    let written = run(r, &Fake(vec![]), true, Mode::Gate);
    assert_eq!(written.exit, Exit::Ok, "{}", written.text);
    let pins = parse_pins(&std::fs::read_to_string(r.join(PINS_FILE)).unwrap()).unwrap();
    assert_eq!(pins.features["jsonlike"].len(), 2, "both workspaces build jsonlike: {pins:?}");
    sh(r, "git", &["add", "-A"]);
    assert!(measure(r, &Fake(vec![])).is_empty(), "clean tree, clean guard");
    assert_eq!(run(r, &Fake(vec![]), false, Mode::Gate).exit, Exit::Ok);

    // The control: the forcing dependency, with the lockfiles regenerated (so only (b) is red).
    add_the_forcing_dependency(r);
    lock_all(r);
    let found = measure(r, &Fake(vec![]));
    let b: Vec<&locks_guard::Finding> = found.iter().filter(|f| f.part == Part::Features).collect();
    assert_eq!(b.len(), 2, "both workspaces: {found:?}");
    assert!(
        b.iter().all(|f| f.message.contains("+ordered") && f.message.contains("jsonlike")),
        "{b:?}"
    );
    assert!(
        b.iter().any(|f| f.file == "apps/x/Cargo.toml") && b.iter().any(|f| f.file == "crates/Cargo.toml"),
        "{b:?}"
    );
    let gate = run(r, &Fake(vec![]), false, Mode::Gate);
    assert_eq!(gate.exit, Exit::Findings, "{}", gate.text);
    assert!(
        gate.text.contains("::error file=apps/x/Cargo.toml::jsonlike in apps/x"),
        "{}",
        gate.text
    );
    // Report-only (the mode it starts in): the same findings, as warnings, and exit 0 (ER-41).
    let soft = run(r, &Fake(vec![]), false, Mode::ReportOnly);
    assert_eq!(soft.exit, Exit::Ok);
    assert!(
        soft.text.contains("::warning file=apps/x/Cargo.toml::") && !soft.text.contains("::error"),
        "{}",
        soft.text
    );

    // The wanted-change path: regenerate the pins, and the guard is green again.
    assert_eq!(run(r, &Fake(vec![]), true, Mode::Gate).exit, Exit::Ok);
    assert!(measure(r, &Fake(vec![])).is_empty());
}

#[test]
fn graph_a_lockfile_the_change_did_not_regenerate_is_found_by_part_c() {
    let d = repo();
    let r = d.path();
    run(r, &Fake(vec![]), true, Mode::Gate);
    add_the_forcing_dependency(r); // manifests changed, no `cargo generate-lockfile`
    let found = measure(r, &Fake(vec![]));
    let locks: Vec<&str> = found.iter().filter(|f| f.part == Part::Lock).map(|f| f.file.as_str()).collect();
    assert_eq!(locks, ["apps/x/Cargo.lock", "crates/Cargo.lock"].map(|s| s), "{found:?}");
    assert!(
        found.iter().any(|f| f.message.contains("cargo update --workspace")),
        "the next step is named: {found:?}"
    );
}

#[test]
fn graph_a_stale_workspace_hack_is_found_by_part_a() {
    let d = repo();
    let r = d.path();
    run(r, &Fake(vec![]), true, Mode::Gate);
    sh(r, "git", &["add", "-A"]);
    let found = measure(r, &Fake(vec!["hakari generate --diff"]));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].part, Part::Hack);
    assert_eq!(found[0].file, "crates/.config/hakari.toml");
    assert!(found[0].message.contains("cargo hakari generate"), "{found:?}");
}

#[test]
fn graph_the_control_cannot_pass_by_watching_nothing_or_without_a_pins_file() {
    let d = repo();
    let r = d.path();
    run(r, &Fake(vec![]), true, Mode::Gate);
    add_the_forcing_dependency(r);
    lock_all(r);
    // An empty watch list sees no drift: the pins file is the thing the guard reads, so a change that empties it is itself in the diff.
    std::fs::write(r.join(PINS_FILE), "watch = []\n").unwrap();
    assert!(measure(r, &Fake(vec![])).is_empty());
    // No pins file: not measured, never a pass.
    std::fs::remove_file(r.join(PINS_FILE)).unwrap();
    let found = measure(r, &Fake(vec![]));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].part, Part::Pins);
    assert!(found[0].message.starts_with("not measured"), "{found:?}");
}

#[test]
fn the_real_tree_has_a_pins_file_that_parses_and_watches_serde_json() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    let pins = parse_pins(&std::fs::read_to_string(root.join(PINS_FILE)).unwrap()).unwrap();
    assert!(pins.watch.iter().any(|w| w == "serde_json"), "{pins:?}");
    assert!(pins.features["serde_json"].contains_key("crates"), "{pins:?}");
}
