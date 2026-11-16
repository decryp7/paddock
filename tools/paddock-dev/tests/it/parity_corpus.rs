//! The shared parity corpus (`conformance/test/parity-corpus/variants.json`): every check that has both a JavaScript host and a Rust
//! twin is run over the same file in many spellings (a byte order mark, CRLF, astral characters and emoji, Unicode case-folding
//! letters, no-break spaces, line separators, a very long line) and the two hosts' findings are compared. The seed is the check's own
//! control tree (`Check::control`), so a new dual-host check needs one line in its parity test: `assert_no_divergence("C1xx")`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{has_node, node, repo_root, skip};
use paddock_dev::check::{RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::Path;

const JS_RUN: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { runChecks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/runner.mjs')).href);
const jobs = JSON.parse(process.env.RT5_JOBS);
const out = {};
for (const { name, root } of jobs) {
  const r = await runChecks({ root, app: 'paddock', only: [process.env.RT5_ONLY], options: {} });
  out[name] = r;
}
process.stdout.write(JSON.stringify(out));
"#;

/// One way of writing a file.
struct Variant {
    name: String,
    bom: bool,
    eol: Option<String>,
    prefix: String,
    suffix: String,
    nbsp: bool,
    trim: bool,
}

fn variants() -> Vec<Variant> {
    let text = std::fs::read_to_string(repo_root().join("conformance/test/parity-corpus/variants.json")).expect("the corpus");
    let v: Value = serde_json::from_str(&text).expect("the corpus is JSON");
    v["variants"]
        .as_array()
        .expect("variants")
        .iter()
        .map(|e| {
            let get = |k: &str| e[k].as_str().unwrap_or("").to_string();
            let repeat = e["suffixRepeat"]["text"].as_str().map_or(String::new(), |t| {
                t.repeat(e["suffixRepeat"]["times"].as_u64().unwrap_or(0) as usize)
            });
            Variant {
                name: get("name"),
                bom: e["bom"].as_bool().unwrap_or(false),
                eol: e["eol"].as_str().map(String::from),
                prefix: get("prefix"),
                suffix: format!("{}{repeat}", get("suffix")),
                nbsp: e["nbsp"].as_bool().unwrap_or(false),
                trim: e["trim"].as_bool().unwrap_or(false),
            }
        })
        .collect()
}

impl Variant {
    fn apply(&self, text: &str) -> String {
        let mut t = text.to_string();
        if self.nbsp {
            t = t.replace(' ', "\u{a0}");
        }
        if self.trim {
            t = t.trim_end_matches('\n').to_string();
        }
        t = format!("{}{t}{}", self.prefix, self.suffix);
        if let Some(eol) = &self.eol {
            t = t.replace('\n', eol);
        }
        if self.bom {
            t.insert(0, '\u{feff}');
        }
        t
    }
}

fn shared(v: &Value) -> Value {
    json!({"findings": v["findings"], "errors": v["errors"], "warnings": v["warnings"], "waived": v["waived"]})
}

fn options(root: &Path, id: &str) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare("paddock"),
        scope: None,
        only: Some(vec![id.to_string()]),
        files: None,
        today: "2026-11-16".into(),
        base_ref: Some("origin/main".into()),
    }
}

/// The variants on which the two hosts of check `id` differ, each with the first finding that differs ("" when both agree: nothing
/// is listed then). An empty list is parity. The seed files are the check's control tree; its allow file is data and is written as it is.
pub(crate) fn divergences(id: &str) -> Vec<String> {
    let reg = registry();
    let check = reg
        .iter()
        .find(|c| c.id() == id)
        .unwrap_or_else(|| panic!("{id} has no Rust twin in the registry"));
    let data = check.allow_list().map(|s| s.file);
    let control = check.control();
    let mut trees = Vec::new();
    for v in variants() {
        let dir = tempfile::tempdir().unwrap();
        for (rel, text) in &control.files {
            let body = if Some(*rel) == data { (*text).to_string() } else { v.apply(text) };
            let to = dir.path().join(rel);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::write(to, body).unwrap();
        }
        trees.push((v.name, dir));
    }
    let jobs: Vec<Value> = trees
        .iter()
        .map(|(n, d)| json!({"name": n, "root": d.path().display().to_string()}))
        .collect();
    let js: Value = serde_json::from_str(&node(
        JS_RUN,
        &[("RT5_JOBS", Value::Array(jobs).to_string()), ("RT5_ONLY", id.to_string())],
    ))
    .unwrap();
    let mut out = Vec::new();
    for (name, dir) in &trees {
        let rust = shared(&run_checks(&options(dir.path(), id), &registry()).unwrap().json());
        let want = shared(&js[name]);
        if rust != want {
            let (r, j) = (rust["findings"].as_array().unwrap(), want["findings"].as_array().unwrap());
            let first = r.iter().zip(j).find(|(a, b)| a != b).map_or_else(
                || format!("{} findings in Rust, {} in JavaScript", r.len(), j.len()),
                |(a, b)| format!("Rust {a} against JavaScript {b}"),
            );
            out.push(format!("{id} variant `{name}`: {first}"));
        }
    }
    out
}

/// Fails with the divergences of check `id` (none is parity); skips without node.
pub(crate) fn assert_no_divergence(id: &str) {
    if !has_node() {
        return skip("node is not installed");
    }
    let d = divergences(id);
    assert!(
        d.is_empty(),
        "the JavaScript and Rust hosts of {id} differ on the shared corpus:\n{}",
        d.join("\n")
    );
}
