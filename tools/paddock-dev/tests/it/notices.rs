//! `dev notices` reproduces what the generator it ports wrote: the files under `tests/golden/notices/` were recorded by
//! `record.mjs` from `conformance/src/notices` on the two fixture trees in `fixtures.json` (invented data; the trees are built here in
//! scratch folders). The test needs no node, so it outlives the generator (ADR 0022 §3.6). Plus a live oracle against the generator
//! while it exists.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::notices::{
    Args, FILE_NAME,
    exec::{ToolRun, Tools},
    generate, resolve, resolve_path,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/notices")
}

/// Never called: the fixtures give `--rust-json`, so no program may run.
struct NoTools;
impl Tools for NoTools {
    fn run(&self, program: &str, _: &[&str], _: &Path, _: Duration) -> Result<ToolRun, String> {
        panic!("{program} must not run in a golden test")
    }
}

fn fixtures() -> Value {
    serde_json::from_str(&fs::read_to_string(golden_dir().join("fixtures.json")).unwrap()).unwrap()
}

fn build(root: &Path, scenario: &Value) {
    for (path, content) in scenario["files"].as_object().unwrap() {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content.as_str().unwrap()).unwrap();
    }
}

/// Parses the recorded generator arguments (`--app`, `--config`, `--root`, `--npm`, `--out`, `--rust-json`) into ours.
fn args_of(scenario: &Value, root: &Path) -> Args {
    let list: Vec<String> = scenario["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().replace("<root>", &root.display().to_string()))
        .collect();
    let mut a = Args::default();
    let mut it = list.into_iter();
    while let Some(flag) = it.next() {
        let value = it.next().unwrap();
        match flag.as_str() {
            "--app" => a.name = Some(value),
            "--config" => a.config = Some(value.into()),
            "--root" => a.root = Some(value.into()),
            "--npm" => a.npm = Some(value),
            "--out" => a.out.push(value),
            "--rust-json" => a.rust_json.push(value),
            other => panic!("unknown recorded flag {other}"),
        }
    }
    a
}

/// Our messages in the recorded form: `out: ` for the generator's log, `err: ` for its complaints, the root as `<root>`.
fn messages(outcome: &paddock_dev::notices::Outcome, root: &Path) -> String {
    let mut s = String::new();
    for m in outcome.messages() {
        s.push_str(if m.complaint { "err: " } else { "out: " });
        s.push_str(&m.text.replace(&root.display().to_string(), "<root>"));
        s.push('\n');
    }
    s
}

fn run_scenario(name: &str) -> (paddock_dev::notices::Outcome, PathBuf, tempfile::TempDir) {
    let all = fixtures();
    let scenario = &all[name];
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    build(&root, scenario);
    let args = args_of(scenario, &root);
    let config = args.config.clone().map(|c| {
        let file = resolve_path(&root, &c);
        let doc: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        (file, doc)
    });
    let plan = resolve(&args, &root, config.as_ref().map(|(f, d)| (f.as_path(), d)), &|p| p.is_file()).unwrap();
    let outcome = generate(&plan, &root, false, false, &NoTools).unwrap();
    (outcome, root, dir)
}

/// A recorded file with LF endings: a Windows checkout (`text=auto`) may have turned them into CRLF.
fn expected(name: &str, ext: &str) -> String {
    fs::read_to_string(golden_dir().join("expected").join(format!("{name}.{ext}")))
        .unwrap()
        .replace("\r\n", "\n")
}

fn assert_scenario(name: &str) {
    let (outcome, root, _dir) = run_scenario(name);
    assert_eq!(
        outcome.markdown,
        expected(name, "md"),
        "{name}: the notices file differs from the generator's"
    );
    assert!(!outcome.markdown.contains('\r'), "LF in embedded licences");
    assert_eq!(messages(&outcome, &root), expected(name, "messages.txt"), "{name}: messages");
    let exit = if outcome.has_findings() { 1 } else { 0 };
    assert_eq!(format!("{exit}\n"), expected(name, "exit"), "{name}: exit status");
    for (file, _) in &outcome.files {
        assert_eq!(
            fs::read(file).unwrap(),
            expected(name, "md").as_bytes(),
            "{name}: the written file, byte for byte"
        );
        assert_eq!(file.file_name().unwrap(), FILE_NAME);
    }
}

#[test]
fn recorded_workspace_scenario_matches_the_generator() {
    assert_scenario("workspace");
}

#[test]
fn recorded_flat_scenario_with_refused_licences_matches_the_generator() {
    assert_scenario("flat");
}

#[test]
fn live_the_generator_itself_still_agrees_on_the_fixtures() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    let script = repo.join("conformance/bin/paddock-notices.mjs");
    if !script.is_file() || std::process::Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: node or conformance/bin/paddock-notices.mjs is missing");
        return;
    }
    let all = fixtures();
    for name in ["workspace", "flat"] {
        let scenario = &all[name];
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        build(&root, scenario);
        let argv: Vec<String> = scenario["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap().replace("<root>", &root.display().to_string()))
            .collect();
        let out = paddock_proc_free(&script, &argv);
        assert!(out.0 <= 1, "{name}: the generator failed: {}", out.1);
        let dir_out = if name == "workspace" { "apps/x/app/resources" } else { "out" };
        let theirs = fs::read_to_string(root.join(dir_out).join(FILE_NAME)).unwrap();
        let (ours, _, _d) = run_scenario(name);
        assert_eq!(ours.markdown, theirs, "{name}");
    }
}

/// Runs the generator with node (a test of this tool may start node; the clippy rule against `Command::new` is for production code).
#[allow(clippy::disallowed_methods)]
fn paddock_proc_free(script: &Path, argv: &[String]) -> (i32, String) {
    let out = std::process::Command::new("node").arg(script).args(argv).output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).into_owned())
}
