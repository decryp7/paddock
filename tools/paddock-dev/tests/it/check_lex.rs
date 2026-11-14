//! The language tokenizers of the Rust host (rustc's lexer, tree-sitter-bash, tree-sitter-yaml) are the oracle for the JavaScript host's
//! hand lexers (`conformance/src/util/mask-rust.mjs`, `shell-lexer.mjs`, `yaml-lexer.mjs`): every tracked `.rs` file, generated sources
//! (nested `/*`, `cr"..."`, `$'...'`, here-documents, tabs, plain-scalar apostrophes) and the 36 probes of the WP-10 batch 2 review
//! (`tests/fixtures/lex/probes.json`, each with the answer the language gives). Node is required: a missing oracle fails the test.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_dev::check::inline_waiver::{CommentView, InlineWaiver, Kind, comments_of};
use paddock_dev::check::mask::mask_rust;
use paddock_dev::check::tree::Tree;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

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

const DRIVER: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const at = (p) => import(pathToFileURL(join(process.cwd(), p)).href);
const m = await at('conformance/src/util/mask-rust.mjs');
const w = await at('conformance/src/util/inline-waiver.mjs');
const { cases } = JSON.parse(process.env.UTIL_INPUT);
process.stdout.write(JSON.stringify(cases.map(([lang, text]) => lang === 'rust' ? [m.maskRust(text), w.commentsOf(text, 'rust')] : [null, w.commentsOf(text, lang)])));
"#;

fn kind_of(lang: &str) -> Kind {
    match lang {
        "rust" => Kind::Rust,
        "shell" => Kind::Shell,
        _ => Kind::Yaml,
    }
}

/// The oracle's answer, then the JavaScript's, for every case; the first difference is named.
fn same_everywhere(cases: &[(String, String)]) {
    let js = node(DRIVER, &json!({ "cases": cases }));
    for (i, (lang, text)) in cases.iter().enumerate() {
        let want_comments = comments_of(text, kind_of(lang));
        let want_mask = (lang == "rust").then(|| mask_rust(text));
        let got = &js[i];
        assert_eq!(got[1], json!(want_comments), "comments of case {i} ({lang}): {text:?}");
        assert_eq!(got[0], json!(want_mask), "mask of case {i} ({lang}): {text:?}");
    }
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_221)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as usize) % n
    }
}

#[test]
fn every_tracked_rust_file_masks_and_comments_alike_in_both_hosts() {
    let root = repo();
    let tree = Tree::open(&root);
    let files: Vec<String> = tree.files().iter().filter(|f| f.ends_with(".rs")).cloned().collect();
    assert!(files.len() > 400, "{}", files.len());
    let cases: Vec<(String, String)> = files
        .iter()
        .map(|f| ("rust".to_string(), std::fs::read_to_string(root.join(f)).unwrap_or_default()))
        .collect();
    same_everywhere(&cases);
}

#[test]
fn generated_rust_sources_lex_alike_in_both_hosts() {
    let pieces = [
        "// ",
        "/* ",
        " */",
        "/*",
        "*/",
        "\"",
        "'",
        "r\"",
        "r#\"",
        "\"#",
        "br#\"",
        "cr\"",
        "cr#\"",
        "b\"",
        "c\"",
        "b'",
        "'a'",
        "'\\n'",
        "'\"'",
        "'a",
        "<'a>",
        "\\",
        "x",
        " ",
        "\n",
        "\n",
        "é",
        "😀",
        "r#type",
        "k#fn",
        "0x1f",
        "1.5e3",
        "1..2",
        "12u8",
        "///",
        "//!",
        "/**",
        "/*!",
        "#!",
        "#[",
        "#\"",
        "##",
        "conformance: t — because",
        "\t",
        "\u{b}",
        "\u{85}",
    ];
    let mut rng = Lcg(0x2545_F491_4F6C_DD1D);
    let cases: Vec<(String, String)> = (0..4000)
        .map(|_| {
            let n = 2 + rng.next(14);
            ("rust".into(), (0..n).map(|_| pieces[rng.next(pieces.len())]).collect::<String>())
        })
        .collect();
    same_everywhere(&cases);
}

#[test]
fn generated_shell_scripts_lex_alike_in_both_hosts() {
    let m = "conformance: t — because";
    let blocks: Vec<String> = vec![
        format!("echo a # {m}"),
        format!("echo 'x # {m}'"),
        format!("echo \"x # {m}\" # {m}"),
        format!("# {m}"),
        format!("echo $'a\\'b # {m}' # {m}"),
        format!("echo ${{#v}} # {m}"),
        format!("echo $# $$ # {m}"),
        format!("curl http://x/#f # {m}"),
        format!("echo a\t# {m}"),
        format!("x=1;# {m}"),
        format!("echo \"it's\" # {m}"),
        format!("echo a#b # {m}"),
        "cd /tmp && ls".to_string(),
        format!("cat <<EOF\n# {m}\nbody 'x\nEOF"),
        format!("cat <<-EOF\n\t# {m}\n\tEOF\n# {m}"),
        format!("cat <<'EOF'\n$x # {m}\nEOF\n# {m}"),
        format!("echo 'a\n # {m}\n' # {m}"),
        format!("echo \"a\n # {m}\n\" # {m}"),
        format!("if true; then # {m}\n  echo hi\nfi"),
        format!("echo a \\\n  b # {m}"),
        format!("cat <<EOF # {m}\nline\nEOF"),
    ];
    let mut rng = Lcg(0xD1B5_4A32_D192_ED03);
    let cases: Vec<(String, String)> = (0..1500)
        .map(|_| {
            let n = 1 + rng.next(6);
            (
                "shell".into(),
                (0..n)
                    .map(|_| blocks[rng.next(blocks.len())].clone())
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n",
            )
        })
        .collect();
    same_everywhere(&cases);
}

#[test]
fn generated_yaml_documents_lex_alike_in_both_hosts() {
    let m = "conformance: t — because";
    let blocks: Vec<String> = vec![
        format!("key: value # {m}"),
        format!("key: don't build # {m}"),
        format!("key: \"q # {m}\" # {m}"),
        format!("key: 'q''s # {m}' # {m}"),
        format!("key: \"a\\\"b # {m}\" # {m}"),
        format!("# {m}"),
        format!("key: v\t# {m}"),
        "key: v#glued-not-a-comment".to_string(),
        format!("url: http://x/#frag # {m}"),
        format!("run: |\n  # {m}\n  echo \"it's\"\n  ./dev x"),
        format!("run: >-\n  # {m}\n  folded"),
        format!("steps:\n  - run: |\n      # {m}\n      echo hi\n  - name: next # {m}"),
        format!("key: 'a\n  # {m}\n  b' # {m}"),
        format!("key: \"a\n  # {m}\n  b\" # {m}"),
        "list:\n  - a\n  - b".to_string(),
    ];
    // A sequence document and a mapping document are each valid YAML; a mix of the two entries is not, and error recovery is no oracle.
    let items: Vec<String> = vec![
        format!("- name: it's here # {m}"),
        format!("- \"q # {m}\" # {m}"),
        format!("- 'it''s # {m}' # {m}"),
        format!("- run: |\n    # {m}\n    echo \"it's\"\n- plain\tword # {m}"),
        format!("# {m}"),
        format!("- a: b\n  c: don't # {m}"),
    ];
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);
    let cases: Vec<(String, String)> = (0..1500)
        .map(|i| {
            let from = if i % 3 == 0 { &items } else { &blocks };
            let n = 1 + rng.next(6);
            (
                "yaml".into(),
                (0..n).map(|_| from[rng.next(from.len())].clone()).collect::<Vec<_>>().join("\n") + "\n",
            )
        })
        .collect();
    same_everywhere(&cases);
}

#[test]
fn the_review_probes_give_the_language_answer_in_both_hosts() {
    let probes: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lex/probes.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(probes.len(), 36);
    let cases: Vec<(String, String)> = probes
        .iter()
        .map(|p| (p["lang"].as_str().unwrap().to_string(), p["text"].as_str().unwrap().to_string()))
        .collect();
    same_everywhere(&cases);
    let state = |w: InlineWaiver| if matches!(w, InlineWaiver::Reason(_)) { "reason" } else { "none" };
    let js = node(
        r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const w = await import(pathToFileURL(join(process.cwd(), 'conformance/src/util/inline-waiver.mjs')).href);
const { probes } = JSON.parse(process.env.UTIL_INPUT);
process.stdout.write(JSON.stringify(probes.map((p) => { const v = w.commentView(p.text, p.lang); return [v.waiver(p.line, p.tag).state, v.file(p.tag).state]; })));
"#,
        &json!({ "probes": probes }),
    );
    for (i, p) in probes.iter().enumerate() {
        let (text, lang, tag, line) = (
            p["text"].as_str().unwrap(),
            p["lang"].as_str().unwrap(),
            p["tag"].as_str().unwrap(),
            p["line"].as_u64().unwrap() as usize,
        );
        let view = CommentView::of(text, kind_of(lang));
        let want = p["expect"].as_str().unwrap();
        assert_eq!(
            state(view.waiver(line, tag)),
            want,
            "{}: the language's answer (Rust host)",
            p["name"]
        );
        assert_eq!(js[i][0], want, "{}: the JavaScript host", p["name"]);
        assert_eq!(js[i][0], state(view.waiver(line, tag)), "{}: both hosts", p["name"]);
        assert_eq!(js[i][1], state(view.file(tag)), "{}: the file level, both hosts", p["name"]);
    }
}
