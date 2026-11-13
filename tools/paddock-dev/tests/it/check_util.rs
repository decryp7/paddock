//! The util helpers of the Rust host against `conformance/src/util/*.mjs`, the oracle: every workflow and action file of this tree,
//! every Markdown file under `docs/`, and generated inputs for the baseline reconcile, the inline-waiver comment, the scope and the
//! version sources. Node is required: a missing oracle fails the test (ER-33).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_dev::check::Ctx;
use paddock_dev::check::baseline::reconcile;
use paddock_dev::check::config::Config;
use paddock_dev::check::inline_waiver::{InlineWaiver, recognise};
use paddock_dev::check::tree::Tree;
use paddock_dev::check::util::{md_tables, scope, version, windows_bash, workflows};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn repo() -> PathBuf {
    crate::full_tree(&paddock_env::winpath::real(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap())
}

fn node(driver: &str, input: &Value) -> Value {
    use std::io::Write;
    let driver = format!(
        "import {{ readFileSync as __read }} from 'node:fs';\nconst __input = JSON.parse(__read(0, 'utf8'));\n{}",
        driver.replace("JSON.parse(process.env.UTIL_INPUT)", "__input")
    );
    let mut child = paddock_proc::command("node")
        .args(["--input-type=module", "-e", &driver])
        .current_dir(repo())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("node must be installed: the oracle cannot be skipped");
    let mut stdin = child.stdin.take().unwrap();
    let text = input.to_string();
    let writer = std::thread::spawn(move || stdin.write_all(text.as_bytes()));
    let o = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap()
}

fn files_of(root: &Path, dir: &str, keep: impl Fn(&str) -> bool) -> Vec<String> {
    let tree = Tree::open(root);
    tree.files().iter().filter(|f| f.starts_with(dir) && keep(f)).cloned().collect()
}

fn workflow_files(root: &Path) -> Vec<String> {
    files_of(root, ".gitea/", |f| f.ends_with(".yml") || f.ends_with(".yaml"))
}

fn workflow_parity(crlf: bool) {
    let root = repo();
    let files = workflow_files(&root);
    assert!(files.len() > 20, "{}", files.len());
    let js = node(
        r#"
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const w = await import(pathToFileURL(join(process.cwd(), 'conformance/src/util/workflows.mjs')).href);
const b = await import(pathToFileURL(join(process.cwd(), 'conformance/src/util/windows-bash.mjs')).href);
const { files, crlf } = JSON.parse(process.env.UTIL_INPUT);
const out = files.map((f) => {
  const raw = readFileSync(f, 'utf8');
  const text = crlf ? raw.replace(/\n/g, '\r\n') : raw;
  const steps = w.parseSteps(text).map((s) => ({ ...s, env: Object.entries(s.env) }));
  return { file: f, jobs: w.parseJobs(text), steps, bash: b.bashBeforeGitBash(text) };
});
process.stdout.write(JSON.stringify(out));
"#,
        &json!({ "files": files, "crlf": crlf }),
    );
    let rust: Vec<Value> = files
        .iter()
        .map(|f| {
            let raw = std::fs::read_to_string(root.join(f)).unwrap();
            let text = if crlf { raw.replace('\n', "\r\n") } else { raw };
            let steps: Vec<Value> = workflows::parse_steps(&text)
                .into_iter()
                .map(|s| {
                    json!({"name": s.name, "line": s.line, "text": s.text, "run": s.run, "uses": s.uses,
                           "env": s.env.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>()})
                })
                .collect();
            let jobs: Vec<Value> = workflows::parse_jobs(&text)
                .into_iter()
                .map(|j| json!({"id": j.id, "line": j.line, "needs": j.needs, "text": j.text}))
                .collect();
            let bash: Vec<Value> = windows_bash::bash_before_git_bash(&text)
                .into_iter()
                .map(|b| json!({"job": b.job, "step": b.step, "line": b.line, "why": b.why}))
                .collect();
            json!({"file": f, "jobs": jobs, "steps": steps, "bash": bash})
        })
        .collect();
    assert_eq!(Value::Array(rust), js);
}

#[test]
fn workflow_jobs_steps_and_the_windows_bash_scan_equal_the_javascript_reader_on_every_file() {
    workflow_parity(false);
}

/// A workflow checked out with CRLF line ends: the line-based reader and the multiline `$` of the scans see `\r` before the end of a line.
#[test]
fn crlf_workflows_are_read_as_the_javascript_reads_them() {
    workflow_parity(true);
}

#[test]
fn a_crlf_windows_job_is_scanned_as_the_javascript_scans_it() {
    let wf = "defaults:\n  run:\n    shell: bash\njobs:\n  w:\n    runs-on: windows-latest # the PC\n    # a comment\n    steps:\n      - name: build\n        if: runner.os == 'Windows'\n        run: make\n  x:\n    runs-on: ubuntu-latest\n    steps:\n      - run: make\n";
    let wf = wf.replace('\n', "\r\n");
    let js = node(
        r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const b = await import(pathToFileURL(join(process.cwd(), 'conformance/src/util/windows-bash.mjs')).href);
const { text } = JSON.parse(process.env.UTIL_INPUT);
process.stdout.write(JSON.stringify(b.bashBeforeGitBash(text)));
"#,
        &json!({ "text": wf }),
    );
    let rust: Vec<Value> = windows_bash::bash_before_git_bash(&wf)
        .into_iter()
        .map(|b| json!({"job": b.job, "step": b.step, "line": b.line, "why": b.why}))
        .collect();
    // Both hosts see the CRLF file the same way (the line-based step reader finds no `run:` behind a `\r`).
    assert_eq!(json!(rust), js);
}

#[test]
fn crlf_version_files_are_read_as_the_javascript_reads_them() {
    let d = tempfile::tempdir().unwrap();
    for (p, t) in [
        ("web/tauri/tauri.conf.json", "{\r\n  \"version\": \"1.2.3\"\r\n}\r\n"),
        ("web/package.json", "{\r\n  \"version\": \"1.2.4\"\r\n}\r\n"),
        (
            "web/tauri/Cargo.toml",
            "[package]\r\nname = \"x\"\r\nversion = \"1.2.5\"\r\nedition = \"2021\"\r\n\r\n[dependencies]\r\nversion = \"9.9.9\"\r\n",
        ),
        ("CHANGELOG.md", "# Changelog\r\n\r\n## [v1.2.6] - 2026-01-01\r\n"),
    ] {
        let f = d.path().join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, t).unwrap();
    }
    std::fs::write(
        d.path().join("paddock.conformance.json"),
        r#"{"app":"fx","appDir":"web","tauriDir":"web/tauri"}"#,
    )
    .unwrap();
    let js = node(
        r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const at = (p) => import(pathToFileURL(join(process.cwd(), p)).href);
const ctxm = await at('conformance/src/context.mjs');
const ver = await at('conformance/src/util/version.mjs');
const { root } = JSON.parse(process.env.UTIL_INPUT);
const ctx = ctxm.createContext({ root, config: ctxm.loadConfig(root, undefined) });
process.stdout.write(JSON.stringify(ver.versionSources(ctx)));
"#,
        &json!({ "root": d.path() }),
    );
    let config = Config::load(d.path(), None).unwrap();
    let tree = Tree::open(d.path());
    let ctx = Ctx::new(&tree, Rc::new(Vec::new()), None, &config, None, String::new());
    let rust: Vec<Value> = version::version_sources(&ctx)
        .iter()
        .map(|s| json!({"file": s.file, "version": s.version}))
        .collect();
    assert_eq!(json!(rust), js);
    assert_eq!(rust.len(), 4, "every source is read, CRLF or not: {rust:?}");
}

#[test]
fn markdown_table_findings_equal_the_javascript_ones_on_every_document() {
    let root = repo();
    let files = files_of(&root, "docs/", |f| f.ends_with(".md"));
    assert!(files.len() > 50, "{}", files.len());
    let tricky = "| a | b |\n|---|---|\n| 1 |\n\ntext\n| a | b |\n|--|--|\n| x \\| y | z |\nplain\n|---|\n> | q | r |\n> |--|--|\n> | 1 |\n```\n| f |\n|--|\n```\n";
    let mut docs: Vec<(String, String)> = files
        .iter()
        .map(|f| (f.clone(), std::fs::read_to_string(root.join(f)).unwrap()))
        .collect();
    docs.push(("tricky".into(), tricky.into()));
    let js = node(
        r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const m = await import(pathToFileURL(join(process.cwd(), 'conformance/src/util/md-tables.mjs')).href);
const { docs } = JSON.parse(process.env.UTIL_INPUT);
process.stdout.write(JSON.stringify(docs.map(([f, t]) => [f, m.tableFindings(t)])));
"#,
        &json!({ "docs": docs }),
    );
    let rust: Vec<Value> = docs
        .iter()
        .map(|(f, t)| {
            let found: Vec<Value> = md_tables::table_findings(t)
                .into_iter()
                .map(|x| json!({"line": x.line, "problem": x.problem, "fix": x.fix}))
                .collect();
            json!([f, found])
        })
        .collect();
    assert_eq!(Value::Array(rust), js);
    assert!(
        js.as_array().unwrap().last().unwrap()[1].as_array().unwrap().len() >= 4,
        "the tricky document finds problems"
    );
}

#[test]
fn the_inline_waiver_comment_is_read_as_the_javascript_does() {
    let tags = ["freed-port", "pid-kill", "dev-before-toolchain"];
    let pieces = [
        "// ",
        "# ",
        "conformance:",
        " ",
        "\t",
        "freed-port",
        "pid-kill",
        "dev-before-toolchain",
        "—",
        "–",
        "-",
        "--",
        " —",
        "\n",
        "because",
        "ab",
        "a b",
        "x",
        "é",
        "😀",
        "\r",
        "  ",
        "list",
        "- run: ./dev x",
        "freed-port-x",
    ];
    // A small linear congruential generator: the same inputs on every run.
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = |n: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_221)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) as usize) % n
    };
    let mut cases: Vec<String> = Vec::new();
    for _ in 0..4000 {
        let len = 2 + next(9);
        cases.push((0..len).map(|_| pieces[next(pieces.len())]).collect::<String>());
    }
    cases.extend([
        "// conformance: freed-port — handed to a child".into(),
        "conformance: pid-kill\nconformance: pid-kill — long reason".into(),
    ]);
    let js = node(
        r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const m = await import(pathToFileURL(join(process.cwd(), 'conformance/src/util/inline-waiver.mjs')).href);
const { cases, tags } = JSON.parse(process.env.UTIL_INPUT);
process.stdout.write(JSON.stringify(cases.map((c) => tags.map((t) => m.inlineWaiver(c, t)))));
"#,
        &json!({ "cases": cases, "tags": tags }),
    );
    let rust: Vec<Value> = cases
        .iter()
        .map(|c| {
            Value::Array(
                tags.iter()
                    .map(|t| match recognise(c, t) {
                        InlineWaiver::No => json!({"state": "none"}),
                        InlineWaiver::NoReason => json!({"state": "no-reason"}),
                        InlineWaiver::Reason(r) => json!({"state": "reason", "reason": r}),
                    })
                    .collect(),
            )
        })
        .collect();
    let rust = Value::Array(rust);
    for (i, (a, b)) in rust.as_array().unwrap().iter().zip(js.as_array().unwrap()).enumerate() {
        assert_eq!(a, b, "case {i}: {:?}", cases[i]);
    }
    let reasons = rust
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|a| a.as_array().unwrap())
        .filter(|v| v["state"] == "reason")
        .count();
    assert!(reasons > 20, "the generated inputs reach the reason path: {reasons}");
}

#[test]
fn the_baseline_reconcile_equals_the_javascript_one_on_generated_baselines() {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = |n: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_221)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % n
    };
    let mut cases: Vec<Value> = Vec::new();
    for _ in 0..600 {
        let mk = |len: u64, next: &mut dyn FnMut(u64) -> u64| -> Vec<Value> {
            (0..len)
                .map(|_| json!({"file": format!("f{}.rs", next(4)), "hash": format!("h{}", next(5))}))
                .collect()
        };
        let (cur, base) = (next(8), next(8));
        let current = mk(cur, &mut next);
        let basev = mk(base, &mut next);
        let loose = next(3) != 0;
        let allow = next(3) != 0;
        cases.push(json!({"current": current, "base": basev, "loose": loose, "allow": allow}));
    }
    let js = node(
        r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const m = await import(pathToFileURL(join(process.cwd(), 'conformance/src/util/baseline.mjs')).href);
const { cases } = JSON.parse(process.env.UTIL_INPUT);
const key = (e) => `${e.file}\u0000${e.hash}`;
process.stdout.write(JSON.stringify(cases.map((c) => {
  const r = m.reconcile(c.current, c.base, key, c.loose ? (e) => e.hash : undefined, () => c.allow);
  return { fresh: r.fresh.map((e) => c.current.indexOf(e)), stale: r.stale.map((e) => c.base.indexOf(e)), moved: r.moved.length };
})));
"#,
        &json!({ "cases": cases }),
    );
    // JS identifies items by reference; with duplicates `indexOf` finds the first, so the Rust side compares what is stable: counts
    // and the moved count, then the exact indexes on the cases without duplicates.
    for (i, c) in cases.iter().enumerate() {
        let items = |k: &str| -> Vec<(String, String)> {
            c[k].as_array()
                .unwrap()
                .iter()
                .map(|e| (e["file"].as_str().unwrap().to_string(), e["hash"].as_str().unwrap().to_string()))
                .collect()
        };
        let (cur, base) = (items("current"), items("base"));
        let exact = |e: &(String, String)| format!("{}\u{0}{}", e.0, e.1);
        let loose = |e: &(String, String)| e.1.clone();
        let allow = c["allow"].as_bool().unwrap();
        let r = reconcile(
            &cur,
            &base,
            &exact,
            c["loose"]
                .as_bool()
                .unwrap()
                .then_some(&loose as &dyn Fn(&(String, String)) -> String),
            &|_, _| allow,
        );
        let j = &js[i];
        assert_eq!(r.fresh.len(), j["fresh"].as_array().unwrap().len(), "case {i} fresh: {c}");
        assert_eq!(r.stale.len(), j["stale"].as_array().unwrap().len(), "case {i} stale: {c}");
        assert_eq!(r.moved.len() as u64, j["moved"].as_u64().unwrap(), "case {i} moved: {c}");
        let unique = |v: &[(String, String)]| v.iter().collect::<std::collections::BTreeSet<_>>().len() == v.len();
        if unique(&cur) && unique(&base) {
            let idx = |k: &str| -> Vec<u64> { j[k].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect() };
            assert_eq!(r.fresh.iter().map(|&x| x as u64).collect::<Vec<_>>(), idx("fresh"), "case {i}");
            assert_eq!(r.stale.iter().map(|&x| x as u64).collect::<Vec<_>>(), idx("stale"), "case {i}");
        }
    }
}

#[test]
fn scope_config_and_version_sources_equal_the_javascript_ones() {
    let paths = [
        "apps/dyno",
        "apps/dyno/a.ts",
        "apps/dynox/a",
        "crates/a/b.rs",
        "ui/projects/paddock-ui/x",
        "ui/projects/dyno/y",
        "ui/projects/debrief/y",
        "docs/a.md",
        "apps/dyno/Cargo.lock#key",
        "",
    ];
    let scopes = ["apps/dyno", "apps/dyno/", "apps/dyno//", "/", "", "apps/debrief"];
    let d = tempfile::tempdir().unwrap();
    for (p, t) in [
        (
            "paddock.conformance.json",
            r#"{"app":"fx","appDir":"web","tauriDir":"web/tauri","timingLint":true}"#,
        ),
        ("web/tauri/tauri.conf.json", r#"{"version":"1.2.3"}"#),
        ("web/package.json", r#"{"version":"1.2.4-rc.1"}"#),
        (
            "web/tauri/Cargo.toml",
            "[package]\nname = \"x\"\nversion = \"1.2.5\"\n[dependencies]\nversion = \"9.9.9\"\n",
        ),
        ("CHANGELOG.md", "# Changelog\n\n## [v1.2.6] - 2026-01-01\n"),
    ] {
        let f = d.path().join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, t).unwrap();
    }
    let js = node(
        r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const at = (p) => import(pathToFileURL(join(process.cwd(), p)).href);
const scope = await at('conformance/src/util/scope.mjs');
const ctxm = await at('conformance/src/context.mjs');
const ver = await at('conformance/src/util/version.mjs');
const { paths, scopes, root } = JSON.parse(process.env.UTIL_INPUT);
const config = ctxm.loadConfig(root, undefined);
const ctx = ctxm.createContext({ root, config });
process.stdout.write(JSON.stringify({
  scope: scopes.map((s) => paths.map((p) => scope.inScope(s || undefined, p))),
  config: Object.keys(ctxm.DEFAULT_CONFIG).map((k) => [k, config[k] ?? null]),
  versions: ver.versionSources(ctx),
  app: ver.appVersion(ctx),
}));
"#,
        &json!({"paths": paths, "scopes": scopes, "root": d.path()}),
    );
    let rust_scope: Vec<Vec<bool>> = scopes
        .iter()
        .map(|s| paths.iter().map(|p| scope::in_scope((!s.is_empty()).then_some(*s), p)).collect())
        .collect();
    assert_eq!(json!(rust_scope), js["scope"]);
    let config = Config::load(d.path(), None).unwrap();
    let keys: Vec<String> = js["config"]
        .as_array()
        .unwrap()
        .iter()
        .map(|kv| kv[0].as_str().unwrap().to_string())
        .collect();
    let rust_config: Vec<Value> = keys.iter().map(|k| json!([k, config.get(k)])).collect();
    assert_eq!(json!(rust_config), js["config"]);
    let tree = Tree::open(d.path());
    let ctx = Ctx::new(&tree, Rc::new(Vec::new()), None, &config, None, String::new());
    let sources: Vec<Value> = version::version_sources(&ctx)
        .iter()
        .map(|s| json!({"file": s.file, "version": s.version}))
        .collect();
    assert_eq!(json!(sources), js["versions"]);
    assert_eq!(json!(version::app_version(&ctx)), js["app"]);
    assert_eq!(sources.len(), 4);
}

#[test]
fn the_comment_reader_equals_the_javascript_one_and_a_marker_in_a_string_is_no_waiver() {
    use paddock_dev::check::inline_waiver::{CommentView, Kind, comments_of};
    let pieces = [
        "// ",
        "# ",
        "/* ",
        " */",
        "\"",
        "'",
        "r#\"",
        "\"#",
        "\\",
        "conformance: t — because",
        "conformance: t",
        "x",
        " ",
        "\n",
        "\n",
        "é",
        "😀",
        "b\"",
        "'\"'",
        "$(",
        "#!",
        "-",
    ];
    let mut state: u64 = 0xD1B5_4A32_D192_ED03;
    let mut next = |n: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_221)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) as usize) % n
    };
    let cases: Vec<String> = (0..3000)
        .map(|_| (0..2 + next(12)).map(|_| pieces[next(pieces.len())]).collect::<String>())
        .collect();
    let js = node(
        r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const m = await import(pathToFileURL(join(process.cwd(), 'conformance/src/util/inline-waiver.mjs')).href);
const { cases } = JSON.parse(process.env.UTIL_INPUT);
process.stdout.write(JSON.stringify(cases.map((c) => [m.commentsOf(c, 'rust'), m.commentsOf(c, 'hash'), m.commentView(c, 'rust').file('t'), m.commentView(c, 'hash').waiver(2, 't')])));
"#,
        &json!({ "cases": cases }),
    );
    let state_json = |w: InlineWaiver| match w {
        InlineWaiver::No => json!({"state": "none"}),
        InlineWaiver::NoReason => json!({"state": "no-reason"}),
        InlineWaiver::Reason(r) => json!({"state": "reason", "reason": r}),
    };
    let mut reasons = 0;
    for (i, c) in cases.iter().enumerate() {
        let (rust, hash) = (comments_of(c, Kind::Rust), comments_of(c, Kind::Hash));
        let file = state_json(CommentView::of(c, Kind::Rust).file("t"));
        let line2 = state_json(CommentView::of(c, Kind::Hash).waiver(2, "t"));
        reasons += usize::from(file["state"] == "reason");
        assert_eq!(json!([rust, hash, file, line2]), js[i], "case {i}: {c:?}");
    }
    assert!(reasons > 10, "the generated inputs reach a comment with a reason: {reasons}");
    // The class this fixes: a marker in a string literal is data.
    assert_eq!(
        CommentView::of("let m = \"// conformance: t — x y z\";\n", Kind::Rust).file("t"),
        InlineWaiver::No
    );
    assert!(matches!(
        CommentView::of("// conformance: t — x y z\n", Kind::Rust).file("t"),
        InlineWaiver::Reason(_)
    ));
}
