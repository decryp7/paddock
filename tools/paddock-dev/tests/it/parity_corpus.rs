//! The shared parity corpus (`conformance/test/parity-corpus/variants.json`): every check that had a JavaScript host as well as a Rust
//! one is run over the same file in many spellings (a byte order mark, CRLF, astral characters and emoji, Unicode case-folding
//! letters, no-break spaces, line separators, a very long line) and the Rust findings are compared with what the JavaScript host
//! answered (recorded in `tests/fixtures/check/recorded/corpus-c<n>.json` before the module was retired, pair P3). The seed is the check's own
//! control tree (`Check::control`), so a check with a recorded answer needs one line here: `assert_no_divergence("C1xx")`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::repo_root;
use paddock_dev::check::{RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::Path;

/// One way of writing a file.
struct Variant {
    name: String,
    bom: bool,
    eol: Option<String>,
    prefix: String,
    suffix: String,
    nbsp: bool,
    trim: bool,
    /// The variant may take the site away (a different spelling of the code itself): the sites it must keep are then not asserted.
    may_empty: bool,
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
                may_empty: e["mayEmptyTree"].as_bool().unwrap_or(false),
            }
        })
        .collect()
}

impl Variant {
    fn apply(&self, text: &str) -> String {
        let mut t = text.to_string();
        if self.nbsp {
            // inside lines only: leading indentation is syntax (YAML), a space after the first word is not
            t = t
                .split('\n')
                .map(|l| {
                    let body = l.trim_start_matches([' ', '\t']);
                    format!("{}{}", &l[..l.len() - body.len()], body.replace(' ', "\u{a0}"))
                })
                .collect::<Vec<_>>()
                .join("\n");
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
        ci: Some(false),
    }
}

/// A file the corpus adds to the control tree: `(path, text)` with a site on line 1, the position a byte order mark or a different line
/// ending is most likely to move. In the `plain` variant the check must report it, on both hosts.
pub(crate) type Seed = (&'static str, &'static str);

/// What the Rust host is given in place of the file's text (a planted difference, to prove the corpus sees it).
type RustView = fn(&str) -> String;

fn field_difference(rust: &Value, js: &Value) -> Option<String> {
    for field in ["findings", "errors", "warnings", "waived"] {
        if rust[field] != js[field] {
            if field == "findings" {
                let (r, j) = (rust[field].as_array().unwrap(), js[field].as_array().unwrap());
                let first = r.iter().zip(j).find(|(a, b)| a != b).map_or_else(
                    || format!("{} findings in Rust, {} in JavaScript", r.len(), j.len()),
                    |(a, b)| format!("first difference: Rust {a} against JavaScript {b}"),
                );
                return Some(first);
            }
            return Some(format!("`{field}` differs: Rust {} against JavaScript {}", rust[field], js[field]));
        }
    }
    None
}

fn write_tree(files: &[(String, String)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (rel, body) in files {
        let to = dir.path().join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, body).unwrap();
    }
    dir
}

/// The variants on which the two hosts of check `id` differ, each with what differs. The seed files are the check's control tree and
/// `seeds` (its allow file is data and is written as it is). Three more things are asserted, so the corpus proves itself: in the `plain`
/// variant the findings are exactly the control's expected ones plus a line-1 finding per seed, and in every other variant (unless it is
/// marked `mayEmptyTree`) each file that holds an expected site still has a finding.
pub(crate) fn divergences_with(id: &str, seeds: &[Seed], rust_view: RustView) -> Vec<String> {
    let reg = registry();
    let check = reg
        .iter()
        .find(|c| c.id() == id)
        .unwrap_or_else(|| panic!("{id} has no Rust twin in the registry"));
    let data = check.allow_list().map(|s| s.file);
    let control = check.control();
    let seed_files: Vec<(&str, &str)> = control.files.iter().copied().chain(seeds.iter().copied()).collect();
    let mut expected: Vec<(String, usize)> = control.expect.iter().map(|(f, l)| (f.to_string(), *l)).collect();
    expected.extend(seeds.iter().map(|(f, _)| (f.to_string(), 1)));
    let mut trees = Vec::new();
    for v in variants() {
        let view = |_: bool, mapper: Option<RustView>| -> Vec<(String, String)> {
            seed_files
                .iter()
                .map(|(rel, text)| {
                    let body = if Some(*rel) == data { (*text).to_string() } else { v.apply(text) };
                    let body = mapper.map_or(body.clone(), |m| m(&body));
                    (rel.to_string(), body)
                })
                .collect()
        };
        trees.push((v.name.clone(), v.may_empty, write_tree(&view(true, Some(rust_view)))));
    }
    let runs: Vec<_> = trees
        .iter()
        .map(|(_, _, rust_dir)| run_checks(&options(rust_dir.path(), id), &registry()).unwrap())
        .collect();
    // What the JS host answered on every variant (recorded before it was retired); a bless takes the Rust answers in their place.
    let ours: serde_json::Map<String, Value> = trees
        .iter()
        .zip(&runs)
        .map(|((n, ..), run)| (n.clone(), shared(&run.json())))
        .collect();
    let js = crate::recorded::expect(&format!("corpus-{}", id.to_lowercase()), &Value::Object(ours));
    let mut out = Vec::new();
    for ((name, may_empty, ..), run) in trees.iter().zip(&runs) {
        let held: Vec<(&str, usize)> = run.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
        if name == "plain" {
            let mut want: Vec<(&str, usize)> = expected.iter().map(|(f, l)| (f.as_str(), *l)).collect();
            want.sort_unstable();
            let mut got = held.clone();
            got.sort_unstable();
            assert_eq!(
                got, want,
                "{id}: the plain variant must give the control's expected findings and one on line 1 per seed"
            );
        } else if !may_empty {
            for (f, _) in &expected {
                if !held.iter().any(|(g, _)| g == f) {
                    out.push(format!(
                        "{id} variant `{name}`: the Rust host lost the expected site in {f} (mark the variant mayEmptyTree if it rewrites the code itself)"
                    ));
                }
            }
        }
        let rust = shared(&run.json());
        if let Some(why) = field_difference(&rust, &js[name]) {
            out.push(format!("{id} variant `{name}`: {why}"));
        }
    }
    out
}

/// [`divergences_with`] with the same text for both hosts.
pub(crate) fn divergences(id: &str, seeds: &[Seed]) -> Vec<String> {
    divergences_with(id, seeds, str::to_string)
}

/// Fails with the divergences of check `id` from the recorded JS answers (none is parity).
pub(crate) fn assert_no_divergence(id: &str, seeds: &[Seed]) {
    let d = divergences(id, seeds);
    assert!(
        d.is_empty(),
        "the JavaScript and Rust hosts of {id} differ on the shared corpus:\n{}",
        d.join("\n")
    );
}

/// A known divergence, with its measured `cause`: it fails when the hosts agree, so the marker is removed with the fix.
pub(crate) fn assert_known_divergence(id: &str, seeds: &[Seed], cause: &str) {
    let d = divergences(id, seeds);
    assert!(
        !d.is_empty(),
        "{id} now agrees on the shared corpus: remove its known-divergence marker and call assert_no_divergence (cause was: {cause})"
    );
}

/// The corpus sees a planted difference: giving the Rust host the text without its byte order mark removes C121's `bom` divergence.
#[test]
fn the_corpus_sees_a_planted_byte_order_mark_difference() {
    let real = divergences("C121", &[]);
    assert!(
        real.iter().any(|d| d.contains("`bom`")),
        "C121 diverges on the bom variant today: {real:?}"
    );
    let planted = divergences_with("C121", &[], |t| t.trim_start_matches('\u{feff}').to_string());
    assert!(
        !planted.iter().any(|d| d.contains("`bom`")),
        "a Rust host given the text without its BOM sees what JavaScript sees: {planted:?}"
    );
    // and a planted difference in a checked field is named
    let (rust, js) = (
        json!({"findings": [], "errors": 0, "warnings": 1, "waived": 0}),
        json!({"findings": [], "errors": 0, "warnings": 2, "waived": 0}),
    );
    assert_eq!(
        field_difference(&rust, &js).unwrap(),
        "`warnings` differs: Rust 1 against JavaScript 2"
    );
}

// Every check that has both hosts, over the shared corpus. A seed puts a site on line 1.

#[test]
fn c39_hosts_agree_on_the_corpus() {
    let seed = (
        "crates/c/tests/first.rs",
        "let p = TcpListener::bind(\"127.0.0.1:0\").unwrap().local_addr().unwrap().port();\n",
    );
    assert_no_divergence("C39", &[seed]);
}

#[test]
fn c53_hosts_agree_on_the_corpus() {
    assert_no_divergence("C53", &[("crates/c/src/first.rs", "unsafe { TerminateProcess(h, 1) };\n")]);
}

#[test]
fn c120_hosts_agree_on_the_corpus() {
    let seed = (
        "crates/c/src/first.rs",
        // conformance: layout-guess — the text of a seed file for the check itself, not a layout this test computes
        "let d = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().to_path_buf();\n",
    );
    assert_no_divergence("C120", &[seed]);
}

// Known divergences, found by this corpus on 2026-11-16 and fixed in train 14. Each fails once the hosts agree: remove the marker.

/// C121 (measured 2026-11-16): on the `bom` variant the two hosts flag different sites: Rust warns 3 times where JavaScript warns 2.
#[test]
fn c121_known_divergence_with_a_byte_order_mark() {
    assert_known_divergence("C121", &[], "the BOM variant warns 3 in Rust against 2 in JavaScript");
}
