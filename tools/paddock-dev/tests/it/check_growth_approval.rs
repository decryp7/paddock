//! The user's approval of growth (ER-46) through the ONE reader every shrink-only list uses (`check::growth_approval`): per list (C139's
//! exception list, the allow-list reader C173 uses, C53's liveness baseline), growth with and without a matching `[[register_growth]]`
//! entry in docs/gates.toml, a digest mismatch, a line that is not the user's and a positive control (the finding is there before any
//! approval, and an approval of another check clears nothing). The digest is computed here by its definition, an oracle independent of the
//! reader's code. C155's own use is in `os_scoped.rs`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::allow_list::Spec;
use paddock_dev::check::c173::{ALLOW, C173};
use paddock_dev::check::config::Config;
use paddock_dev::check::finding::{Finding, Severity};
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{Ctx, RunOptions, registry, run_checks};
use sha2::{Digest, Sha256};
use std::path::Path;
use tempfile::TempDir;

fn write(root: &Path, rel: &str, text: &str) {
    let to = root.join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(to, text).unwrap();
}

fn git(root: &Path, args: &[&str]) {
    let out = paddock_dev::git_env::git()
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A repository whose merge base `main` holds `base` and whose `topic` commit holds `now` (files as (path, text)).
fn repo(base: &[(&str, &str)], now: &[(&str, &str)]) -> TempDir {
    let d = tempfile::tempdir().unwrap();
    for (p, t) in base {
        write(d.path(), p, t);
    }
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "base"]);
    git(d.path(), &["checkout", "-q", "-b", "topic"]);
    for (p, t) in now {
        write(d.path(), p, t);
    }
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "topic"]);
    d
}

/// The digest by its definition: SHA-256 (hex) of the sorted keys joined by a newline.
fn digest(keys: &[&str]) -> String {
    let mut k = keys.to_vec();
    k.sort_unstable();
    Sha256::digest(k.join("\n").as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn gates(check: &str, by: &str, count: usize, digest: &str) -> String {
    format!(
        "version = 1\n[[register_growth]]\ncheck = \"{check}\"\nby = \"{by}\"\ndate = \"2026-11-27\"\ncount = {count}\ndigest = \"{digest}\"\n"
    )
}

/// One list: how to make its growing repository and run its check, what its growth finding says, and the exact keys it added.
struct List {
    id: &'static str,
    marker: &'static str,
    keys: &'static [&'static str],
    tree: fn() -> TempDir,
    run: fn(&Path) -> Vec<Finding>,
}

impl List {
    /// The growth findings after the tree holds `gates_text` as docs/gates.toml (committed, as the user's line would be).
    fn growth_with(&self, gates_text: Option<&str>) -> Vec<Finding> {
        let d = (self.tree)();
        if let Some(t) = gates_text {
            write(d.path(), "docs/gates.toml", t);
            git(d.path(), &["add", "."]);
            git(d.path(), &["commit", "-q", "-m", "the gates line"]);
        }
        (self.run)(d.path())
            .into_iter()
            .filter(|f| f.message.contains(self.marker))
            .collect()
    }
}

fn lists() -> Vec<List> {
    vec![
        List {
            id: "C139",
            marker: "the exception list grew",
            keys: &["e2e/a.spec.ts\0paddock/e2e-ready-reads"],
            tree: c139_tree,
            run: c139_run,
        },
        List {
            id: "C173",
            marker: "the allow list grew",
            keys: &["scripts/hidden.sh\0tooling file"],
            tree: c173_tree,
            run: c173_run,
        },
        List {
            id: "C53",
            marker: "the baseline grew",
            keys: &["a.rs\0h2"],
            tree: c53_tree,
            run: c53_run,
        },
    ]
}

fn options(root: &Path, id: &str) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: Config::bare("fixture"),
        scope: None,
        only: Some(vec![id.into()]),
        files: None,
        today: "2026-11-27".into(),
        base_ref: Some("main".into()),
        ci: Some(false),
    }
}

// C139: the ui list's count goes from 1 to 2.
const R: &str = "paddock/e2e-ready-reads";
fn c139_tree() -> TempDir {
    let supp = |n: u32| format!("{{\"e2e/a.spec.ts\":{{\"{R}\":{{\"count\":{n}}}}}}}");
    repo(
        &[("ui/e2e/a.spec.ts", "const a = 1;\n"), ("ui/eslint-suppressions.json", &supp(1))],
        &[("ui/eslint-suppressions.json", &supp(2))],
    )
}
fn c139_run(root: &Path) -> Vec<Finding> {
    run_checks(&options(root, "C139"), &registry()).unwrap().findings
}

// C173: the allow list gains an entry (the blocking spec, as the reader's own blocking users have).
static BLOCKING: Spec = Spec {
    id: "C173",
    rule: 83,
    file: ALLOW,
    unseen: "a tooling file",
    restore: "Restore it.",
    growth_blocks: true,
};
fn allow_text(paths: &[&str]) -> String {
    let list: Vec<String> = paths
        .iter()
        .map(|p| format!("{{\"path\": \"{p}\", \"site\": \"tooling file\", \"reason\": \"baseline: ported later\"}}"))
        .collect();
    format!("{{\"allow\": [{}]}}", list.join(", "))
}
fn c173_tree() -> TempDir {
    repo(
        &[(ALLOW, &allow_text(&["scripts/old.sh"])), ("scripts/old.sh", "x\n")],
        &[
            (ALLOW, &allow_text(&["scripts/old.sh", "scripts/hidden.sh"])),
            ("scripts/hidden.sh", "x\n"),
        ],
    )
}
fn c173_run(root: &Path) -> Vec<Finding> {
    let tree = Tree::open(root);
    let config = Config::bare("fixture");
    let ctx = Ctx::new(&tree, tree.files(), Some("main".into()), &config, None, "2026-11-27".into());
    C173.run_with(&ctx, &BLOCKING).findings
}

// C53: the liveness baseline gains a site.
fn c53_tree() -> TempDir {
    let base = "{\"sites\":[{\"file\":\"a.rs\",\"line\":1,\"hash\":\"h1\"}]}";
    let now = "{\"sites\":[{\"file\":\"a.rs\",\"line\":1,\"hash\":\"h1\"},{\"file\":\"a.rs\",\"line\":2,\"hash\":\"h2\"}]}";
    repo(
        &[("conformance/baselines/C53.json", base), ("a.rs", "fn main() {}\n")],
        &[("conformance/baselines/C53.json", now)],
    )
}
fn c53_run(root: &Path) -> Vec<Finding> {
    run_checks(&options(root, "C53"), &registry()).unwrap().findings
}

/// Positive control: before any approval each list's growth is one error at its list that prints the exact line the user adds.
#[test]
fn every_list_keeps_its_growth_finding_without_an_approval_and_prints_the_line_to_add() {
    for l in lists() {
        let found = l.growth_with(None);
        assert_eq!(found.len(), 1, "{}: {found:?}", l.id);
        assert_eq!(found[0].severity, Severity::Error, "{}", l.id);
        let want = format!(
            "[[register_growth]] check = \"{}\", by = \"user\", date = \"<day>\", count = {}, digest = \"{}\"",
            l.id,
            l.keys.len(),
            digest(l.keys)
        );
        assert!(found[0].fix.contains(&want), "{}: {}", l.id, found[0].fix);
    }
}

#[test]
fn a_matching_entry_by_the_user_turns_each_growth_finding_off() {
    for l in lists() {
        let g = gates(l.id, "user", l.keys.len(), &digest(l.keys));
        let found = l.growth_with(Some(&g));
        assert!(found.is_empty(), "{}: {found:?}", l.id);
    }
}

#[test]
fn a_digest_of_other_additions_a_wrong_count_a_line_that_is_not_the_users_or_another_checks_clear_nothing_and_say_why() {
    for l in lists() {
        let right = digest(l.keys);
        let cases = [
            (gates(l.id, "user", l.keys.len(), &digest(&["something else"])), "`digest` is not"),
            (gates(l.id, "user", l.keys.len() + 1, &right), "`count` is not"),
            (gates(l.id, "coordinator", l.keys.len(), &right), "not `by = \"user\"`"),
        ];
        for (text, why) in cases {
            let found = l.growth_with(Some(&text));
            assert_eq!(found.len(), 1, "{}: {why}: {found:?}", l.id);
            assert!(
                found[0].message.contains("does not match") && found[0].message.contains(why),
                "{}: {}",
                l.id,
                found[0].message
            );
        }
        // The user's line for another check is not this check's: the finding stands as if there were none.
        let other = gates("C999", "user", l.keys.len(), &right);
        let found = l.growth_with(Some(&other));
        assert_eq!(found.len(), 1, "{}: {found:?}", l.id);
        assert!(!found[0].message.contains("does not match"), "{}", found[0].message);
    }
}

/// The JavaScript host (`conformance/src/util/baseline.mjs`, which still blocks C71, C104, C115, C135 and its C139 in CI's conformance
/// line) reads the same `[[register_growth]]` entries with the same digest: for each case both readers agree on whether the user's entry
/// approves the additions, and on the digest itself (keys with a NUL, and an astral and a BMP character whose byte order differs from
/// their UTF-16 order).
#[test]
fn the_javascript_reader_agrees_with_the_rust_reader_on_digest_and_approval() {
    use super::check::{has_node, node, skip};
    use paddock_approvals::{Verdict, growth_digest, judge_growth, parse_gates};
    let growth_approved = |gates: &str, check: &str, keys: &[String]| {
        let records = parse_gates(gates).map(|g| g.growth).unwrap_or_default();
        judge_growth(&records, check, keys) == Verdict::Approved
    };
    if !has_node() {
        return skip("node is not installed");
    }
    let keys: Vec<String> = ["a.rs\0h2", "\u{FF5E}", "\u{1F600}", "b"].iter().map(|k| k.to_string()).collect();
    let right = growth_digest(&keys);
    let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    assert_eq!(right, digest(&refs), "the Rust digest is the definition");
    let cases: Vec<(String, String)> = vec![
        ("C139".into(), gates("C139", "user", 4, &right)),
        ("C139".into(), gates("C139", "user", 4, &digest(&["x"]))),
        ("C139".into(), gates("C139", "user", 3, &right)),
        ("C139".into(), gates("C139", "coordinator", 4, &right)),
        ("C139".into(), gates("C53", "user", 4, &right)),
        ("C139".into(), "version = 1\n".into()),
        (
            "C139".into(),
            format!("{}\n[[other]]\ncheck = \"C139\"\n", gates("C139", "user", 4, &right)),
        ),
    ];
    let want: Vec<bool> = cases.iter().map(|(c, g)| growth_approved(g, c, &keys)).collect();
    assert_eq!(
        want,
        [true, false, false, false, false, false, true],
        "the Rust reader's own answers"
    );
    let input = serde_json::json!({"keys": keys, "cases": cases}).to_string();
    let js = node(
        r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { growthApproved, growthDigest } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/util/baseline.mjs')).href);
const { keys, cases } = JSON.parse(process.env.GA_INPUT);
process.stdout.write(JSON.stringify({ digest: growthDigest(keys), approved: cases.map(([c, g]) => growthApproved(g, c, keys)) }));
"#,
        &[("GA_INPUT", input)],
    );
    let js: serde_json::Value = serde_json::from_str(&js).unwrap();
    assert_eq!(js["digest"], right.as_str());
    assert_eq!(js["approved"], serde_json::json!(want));
}

/// The stage of a run: an explicit `PADDOCK_APPROVAL_STAGE` wins, else the ref CI says (`PADDOCK_APPROVAL_STAGE_FROM_REF`, mapped by
/// `paddock_approvals::Stage::from_ref`), else landing, the strictest (user decision 125).
#[test]
fn the_stage_is_the_explicit_variable_else_the_ref_else_landing() {
    use paddock_approvals::Stage;
    use paddock_dev::check::growth_approval::{STAGE_ENV, STAGE_FROM_REF_ENV, stage_from_env};
    let stage = |pairs: &[(&str, &str)]| {
        let pairs: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        stage_from_env(&|k| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()))
    };
    assert_eq!(stage(&[]), Ok(Stage::Landing));
    assert_eq!(stage(&[(STAGE_FROM_REF_ENV, "refs/heads/train/x")]), Ok(Stage::MemberGate));
    assert_eq!(stage(&[(STAGE_FROM_REF_ENV, "refs/heads/trial/x")]), Ok(Stage::MemberGate));
    assert_eq!(stage(&[(STAGE_FROM_REF_ENV, "refs/heads/main")]), Ok(Stage::Landing));
    assert_eq!(
        stage(&[(STAGE_FROM_REF_ENV, "")]),
        Ok(Stage::Landing),
        "an empty ref (a skipped expression) is landing"
    );
    // the explicit name wins over the ref, either way
    assert_eq!(
        stage(&[(STAGE_ENV, "branch-push"), (STAGE_FROM_REF_ENV, "refs/heads/main")]),
        Ok(Stage::BranchPush)
    );
    assert_eq!(
        stage(&[(STAGE_ENV, "landing"), (STAGE_FROM_REF_ENV, "refs/heads/train/x")]),
        Ok(Stage::Landing)
    );
    // a name that is no stage is an error, never a default, even beside a ref
    let e = stage(&[(STAGE_ENV, "nonsense"), (STAGE_FROM_REF_ENV, "refs/heads/main")]).unwrap_err();
    assert!(e.contains(STAGE_ENV) && e.contains("nonsense"), "{e}");
}

/// ci.yml declares the ONE line that tells `./dev` the ref (a shared file: a single declared line), at the workflow level so every job
/// that runs a check inherits it; read through paddock-ci-config's YAML parser, never a regex (ER-42).
#[test]
fn ci_yml_declares_the_one_ref_line_for_the_stage() {
    use paddock_dev::check::growth_approval::STAGE_FROM_REF_ENV;
    let root = super::check::repo_root();
    let text = std::fs::read_to_string(root.join(".gitea/workflows/ci.yml")).unwrap();
    let doc = paddock_ci_config::yaml::parse(&text).unwrap();
    let env = doc.get("env").expect("ci.yml has a workflow-level env");
    assert_eq!(env.get_text(STAGE_FROM_REF_ENV), Some("${{ github.ref }}"));
    // positive control: the reader does see a line that is not there
    assert_eq!(env.get_text("PADDOCK_NO_SUCH_LINE"), None);
}
