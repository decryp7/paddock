//! The migration net of ADR 0117 §8 (WP-5): what the repository model answers beside what the rule tables answer, recorded.
//!
//! - `recorded_rule_goldens`: one golden per migrated rule table and per difference category (`tests/golden/rules/<rule>.json`), on a
//!   small fixture repository built in the test with its own frozen inputs file: for each probe path, what the rule tables run when
//!   only that file changes, what the model selects, and the category of every narrowing. A `category:` group is a positive control:
//!   its probes must produce that category.
//! - `recorded_content_rules`: the content rules (`guards_changed`, `lock_diff`, the `.gitattributes` extractor) on a table of
//!   before and after texts (`tests/golden/rules/content.json`).
//! - `train_replay_has_no_unlisted_narrowing`: every non-merge member commit of trains 15 to 20 against its parent on the real
//!   tree, with no golden (nothing to re-bless when the tree moves); any narrowing no category of §8 explains fails, the defects of
//!   WP-3's selection are pinned and may only shrink.
//!
//! Bless with `PADDOCK_GOLDEN_BLESS=1` (never in CI), review the diff, commit it.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::plan::{Request, plan_on_graph, read_graph};
use paddock_dev::repo_model::replay::{self, Category, Diff, Replayed};
use paddock_dev::repo_model::{build_world, changed_files, compare_targets, reach_in};
use paddock_repo_graph::GitHistory;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn repo_root() -> PathBuf {
    crate::full_tree(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap())
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn diff_json(d: &Diff) -> Value {
    json!({"project": d.project, "rules": d.rules, "model": d.model, "category": d.category.name(), "dropped": d.dropped})
}

/// Compares `got` with the recorded file, or rewrites it when blessing.
fn check_recorded(path: &Path, got: &Value) {
    if crate::golden::bless() {
        crate::golden::record(path, got);
        return;
    }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|_| "null".into())).unwrap();
    assert!(
        &want == got,
        "{} changed; review the difference, then run `PADDOCK_GOLDEN_BLESS=1 cargo nextest run -E 'test(repo_graph_net::)'` and commit it:\n{}",
        path.display(),
        serde_json::to_string_pretty(got).unwrap()
    );
}

/// One probe: the files `files` changed alone, answered by both planners.
fn probe(
    root: &Path,
    history: &GitHistory,
    graph: &paddock_dev::plan::RepoGraph,
    world: &paddock_dev::repo_model::World,
    files: &[&str],
) -> Vec<Diff> {
    let req = Request {
        files: files.iter().map(|s| s.to_string()).collect(),
        all: false,
        explain: true,
        root: None,
        base: None,
        head: "HEAD".into(),
    };
    let plan = plan_on_graph(root, history, &req, graph).unwrap();
    let changed = changed_files(history, &req).unwrap();
    let reached = reach_in(world, root, &req, &changed, graph);
    let c = compare_targets(&plan, &reached, graph, world);
    replay::diffs_of(&c, &changed)
}

/// The fixture repository the rule goldens plan: the planner's own fixture (`tests/golden/fixture`) plus the frozen inputs file of
/// `tests/golden/rules/extra`, committed on top. The real tree is never read, so a commit to it re-blesses nothing (ER-46).
fn rules_fixture() -> (tempfile::TempDir, PathBuf) {
    let (keep, root) = crate::golden::fixture_repo();
    copy_extra(&golden_dir().join("rules/extra"), &root.join(""));
    crate::golden::git(&root, &["add", "-A"]);
    crate::golden::git(&root, &["commit", "-q", "-m", "inputs"]);
    (keep, root)
}

fn copy_extra(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_extra(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

#[test]
fn recorded_rule_goldens() {
    let (_keep, root) = rules_fixture();
    let history = GitHistory::new(&root, Duration::from_secs(60));
    let graph = read_graph(&root, &history).unwrap();
    let world = build_world(&root, &history, &graph, None).unwrap();
    let probes: Value = serde_json::from_str(&std::fs::read_to_string(golden_dir().join("rules/probes.json")).unwrap()).unwrap();
    let mut unlisted = Vec::new();
    let mut with_difference = 0;
    for (rule, files) in probes.as_object().unwrap() {
        let mut rows = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for f in files.as_array().unwrap().iter().map(|f| f.as_str().unwrap()) {
            let diffs = probe(&root, &history, &graph, &world, &[f]);
            unlisted.extend(
                diffs
                    .iter()
                    .filter(|d| d.category == Category::Unlisted)
                    .map(|d| format!("{rule}: {f}: {d:?}")),
            );
            with_difference += usize::from(!diffs.is_empty());
            seen.extend(diffs.iter().map(|d| d.category.name()));
            rows.push(json!({"file": f, "differences": diffs.iter().map(diff_json).collect::<Vec<_>>()}));
        }
        // A category group is a positive control: its probes produce that category, or the group watches nothing.
        if let Some(cat) = rule.strip_prefix("category:") {
            assert!(
                seen.contains(&cat),
                "the probes of {rule} produce no {cat} difference (they produce {seen:?})"
            );
        }
        let name = rule.trim_start_matches('.').replace(':', "-");
        check_recorded(
            &golden_dir().join(format!("rules/{name}.json")),
            &json!({"rule": rule, "probes": rows}),
        );
    }
    assert!(unlisted.is_empty(), "narrowings no category explains:\n{}", unlisted.join("\n"));
    assert!(
        with_difference > 0,
        "a positive control: some probe differs, or the comparison proves nothing"
    );
}

/// The content rules on texts: what `guards_changed`, `lock_diff` and the attributes extractor answer for each before and after.
#[test]
fn recorded_content_rules() {
    let cases: Vec<(&str, &str, Option<&str>, Option<&str>)> = vec![
        (
            "guards_changed",
            "an ignore is added",
            Some("#[test]\nfn a() {}\n"),
            Some("#[test]\n#[ignore]\nfn a() {}\n"),
        ),
        (
            "guards_changed",
            "an ignore moves to another test",
            Some("#[test]\n#[ignore]\nfn a() {}\n#[test]\nfn b() {}\n"),
            Some("#[test]\nfn a() {}\n#[test]\n#[ignore]\nfn b() {}\n"),
        ),
        (
            "guards_changed",
            "no guard changes",
            Some("#[test]\nfn a() {}\n"),
            Some("#[test]\nfn a() { assert!(true); }\n"),
        ),
        ("guards_changed", "a file is deleted", Some("#[test]\n#[ignore]\nfn a() {}\n"), None),
        ("lock_diff", "a package version changes", Some(LOCK_A), Some(LOCK_B)),
        ("lock_diff", "the lock is unchanged", Some(LOCK_A), Some(LOCK_A)),
        ("lock_diff", "no lock before", None, Some(LOCK_A)),
        (
            "attributes",
            "an eol attribute changes",
            Some("*.sh text eol=lf\n"),
            Some("*.sh text eol=crlf\n"),
        ),
        (
            "attributes",
            "only a merge driver changes",
            Some("*.lock text\n"),
            Some("*.lock text merge=ours\n"),
        ),
        (
            "attributes",
            "only a comment changes",
            Some("# a\n*.sh text eol=lf\n"),
            Some("# b\n*.sh text eol=lf\n"),
        ),
    ];
    let mut rows = Vec::new();
    for (rule, name, before, after) in cases {
        let answer = match rule {
            "guards_changed" => json!(paddock_dev::plan::guards_changed(before, after)),
            "lock_diff" => match paddock_repo_graph::lock_diff(before, after) {
                Ok(w) => json!(w),
                Err(e) => json!({"error": e.to_string()}),
            },
            _ => {
                let x = paddock_repo_graph::model::narrow::content_extractor("attributes").unwrap();
                match x.narrow(before, after) {
                    Ok(n) => json!(format!("{n:?}")),
                    Err(e) => json!({"error": e}),
                }
            }
        };
        rows.push(json!({"rule": rule, "case": name, "answer": answer}));
    }
    // Positive control: the same rule answers differently for a changed and an unchanged text.
    let guards: Vec<&Value> = rows
        .iter()
        .filter(|r| r["rule"] == "guards_changed")
        .map(|r| &r["answer"])
        .collect();
    assert!(
        guards.contains(&&json!(true)) && guards.contains(&&json!(false)),
        "guards_changed sees both"
    );
    check_recorded(&golden_dir().join("rules/content.json"), &Value::Array(rows));
}

const LOCK_A: &str = r#"{"lockfileVersion":3,"packages":{"":{"workspaces":["ui"]},"ui":{"name":"ui","version":"1.0.0"},"node_modules/left-pad":{"version":"1.0.0"}}}"#;
const LOCK_B: &str = r#"{"lockfileVersion":3,"packages":{"":{"workspaces":["ui"]},"ui":{"name":"ui","version":"1.0.0"},"node_modules/left-pad":{"version":"1.1.0"}}}"#;

/// The train replay's range: the head of train 14 (not replayed) to the head of train 20, as commit ids so a moved ref changes
/// nothing. Its non-merge commits are the member commits of trains 15 to 20 and what they merged from main.
const REPLAY_FROM: &str = "9cbd77a63";
const REPLAY_TO: &str = "803aed12d";

fn replayed() -> Vec<Replayed> {
    let root = repo_root();
    let history = GitHistory::new(&root, Duration::from_secs(120));
    let graph = read_graph(&root, &history).unwrap();
    let world = build_world(&root, &history, &graph, None).unwrap();
    let range = format!("{REPLAY_FROM}..{REPLAY_TO}");
    let members = replay::members(&root, &range).unwrap_or_else(|e| {
        panic!("::error::the train replay cannot list {range}: {e}. Next step: fetch the full history (`git fetch --unshallow`, all branches); a shallow clone cannot replay trains 15 to 20")
    });
    assert!(
        members.len() > 300,
        "the range holds the member commits of trains 15 to 20, not {}",
        members.len()
    );
    let subject = |sha: &str| {
        use paddock_repo_graph::History;
        history
            .log(sha, 1)
            .ok()
            .and_then(|l| l.first().map(|c| c.subject.clone()))
            .unwrap_or_default()
    };
    replay::replay(&root, &history, &graph, &world, &members, &subject)
}

/// The real tree, checked and not recorded: no golden, so nothing here is re-blessed when the tree moves. A narrowing no category of
/// §8 explains fails; the report by category goes to the test's output.
#[test]
fn train_replay_has_no_unlisted_narrowing() {
    let r = replayed();
    let bad = replay::unlisted(&r);
    assert!(
        bad.is_empty(),
        "narrowings no category of ADR 0117 §8 explains ({}):\n{}",
        bad.len(),
        bad.join("\n")
    );
    assert!(
        r.iter().flat_map(|c| &c.diffs).any(|d| d.narrowing),
        "a positive control: the replay finds narrowings, or comparing proves nothing"
    );
    let mut by: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for d in r.iter().flat_map(|c| &c.diffs) {
        *by.entry(d.category.name()).or_default() += 1;
    }
    eprintln!("train replay: {} commits, differences by category {by:?}", r.len());
    // The pinned defects only shrink (ER-46): at 0, none may come back.
    assert_eq!(replay::defects(&r), PINNED_DEFECTS, "the release-installers-only defect came back");
}

/// How many (commit, project) differences the `ReleaseInstallersOnly` defect of WP-3's `selection` has in the replay today: none
/// since WP-3's union selection (a release reach keeps the app's smoke set or suite), measured on the combined tree on 2026-11-23.
const PINNED_DEFECTS: usize = 0;

#[test]
fn control_a_narrowing_no_category_explains_is_listed_and_a_listed_one_is_not() {
    let member = replay::Member {
        sha: "0123456789abcdef".into(),
        parent: "fedcba9876543210".into(),
    };
    let narrowing = |category| Replayed {
        member: member.clone(),
        subject: "feat: x".into(),
        changed: vec!["crates/a/src/lib.rs".into()],
        error: None,
        diffs: vec![Diff {
            project: "app:dyno".into(),
            rules: Some("all".into()),
            model: None,
            narrowing: true,
            category,
            dropped: vec!["nextest:dyno".into()],
            why_rules: Vec::new(),
            why_model: Vec::new(),
        }],
        took: Default::default(),
    };
    assert_eq!(
        replay::unlisted(&[narrowing(Category::Unlisted)]).len(),
        1,
        "an unexplained narrowing fails"
    );
    assert!(
        replay::unlisted(&[narrowing(Category::TestOnly)]).is_empty(),
        "control: an explained one does not"
    );
    let mut failed = narrowing(Category::TestOnly);
    failed.error = Some("the rule tables: boom".into());
    assert_eq!(
        replay::unlisted(&[failed]).len(),
        1,
        "a commit a planner could not answer is listed"
    );
}

#[test]
fn members_are_the_non_merge_commits_with_their_first_parent() {
    let (_keep, root) = crate::golden::fixture_repo();
    let g = |args: &[&str]| crate::golden::git(&root, args);
    g(&["checkout", "-q", "-b", "side", "fixture-head"]);
    std::fs::write(root.join("side.txt"), "side\n").unwrap();
    g(&["add", "side.txt"]);
    g(&["commit", "-q", "-m", "feat: side"]);
    g(&["checkout", "-q", "main"]);
    std::fs::write(root.join("main.txt"), "main\n").unwrap();
    g(&["add", "main.txt"]);
    g(&["commit", "-q", "-m", "feat: main"]);
    g(&["merge", "-q", "--no-ff", "-m", "chore: merge side", "side"]);
    let members = replay::members(&root, "fixture-head..HEAD").unwrap();
    assert_eq!(members.len(), 2, "the two commits, not the merge: {members:?}");
    let parent_of = |rev: &str| {
        let mut cmd = paddock_proc::command("git");
        cmd.arg("-C").arg(&root).args(["rev-parse", &format!("{rev}^")]);
        paddock_proc::run_bounded(cmd, paddock_proc::Limits::new(Duration::from_secs(30)))
            .unwrap()
            .stdout_text()
            .trim()
            .to_string()
    };
    assert!(
        members.iter().all(|m| m.parent == parent_of(&m.sha)),
        "each member names its first parent"
    );
    // control: a range that is not <from>..<to> is refused, and a flag cannot pass for a range
    assert!(replay::members(&root, "fixture-head").is_err());
    assert!(replay::members(&root, "--all..HEAD").is_err());
}

/// WP-7: the registry's generators as nodes of the model. The two entries that name `reads` are due through the graph, and the
/// control takes the edge away.
mod generators {
    use super::*;
    use paddock_dev::generators::{graph, registry::Registry};

    fn registry(root: &Path) -> Registry {
        paddock_dev::generators::load(root).unwrap()
    }

    #[test]
    fn a_script_a_generator_reads_makes_it_due_through_the_graph_and_the_control_removes_the_edge() {
        let root = repo_root();
        let history = GitHistory::new(&root, Duration::from_secs(60));
        let graph_ = read_graph(&root, &history).unwrap();
        let mut world = build_world(&root, &history, &graph_, None).unwrap();
        let reg = registry(&root);
        let missing = graph::with_generators(&mut world, &reg);
        assert!(
            missing.is_empty(),
            "every `reads` of docs/generators.toml names a node the model has: {missing:?}"
        );
        let ids = |changed: &[&str], reg: &Registry, world: &paddock_dev::repo_model::World| -> Vec<String> {
            let changed: Vec<String> = changed.iter().map(|s| s.to_string()).collect();
            graph::due(world, reg, &changed).into_iter().map(|d| d.id).collect()
        };
        // scripts/notices.sh and ci.yml are no longer typed in any `paths`: the graph alone makes these due
        let notices = ids(&["scripts/notices.sh"], &reg, &world);
        for app in ["debrief", "dyno", "pitwall"] {
            assert!(notices.contains(&format!("notices-{app}")), "{notices:?}");
        }
        assert!(ids(&[".gitea/workflows/ci.yml"], &reg, &world).contains(&"ci-run-golden".to_string()));
        // control: with the `reads` taken out of the declarations the same change marks none of them
        let mut bare = registry(&root);
        for g in &mut bare.generators {
            g.reads.clear();
        }
        let mut world2 = build_world(&root, &history, &graph_, None).unwrap();
        graph::with_generators(&mut world2, &bare);
        let after = ids(&["scripts/notices.sh"], &bare, &world2);
        assert!(
            after.iter().all(|i| !i.starts_with("notices-")),
            "control: no reads, not due: {after:?}"
        );
        assert!(!ids(&[".gitea/workflows/ci.yml"], &bare, &world2).contains(&"ci-run-golden".to_string()));
    }

    #[test]
    fn every_file_the_hand_lists_named_before_the_move_still_marks_its_generator() {
        // The two paths WP-7 moved out of `paths` into `reads` (docs/generators.toml history: scripts/notices.sh in the three notices
        // entries, .gitea/workflows/ci.yml in the ci-run golden): the net for the migration of the hand lists.
        let root = repo_root();
        let history = GitHistory::new(&root, Duration::from_secs(60));
        let graph_ = read_graph(&root, &history).unwrap();
        let mut world = build_world(&root, &history, &graph_, None).unwrap();
        let reg = registry(&root);
        graph::with_generators(&mut world, &reg);
        let moved = [
            ("notices-debrief", "scripts/notices.sh"),
            ("notices-dyno", "scripts/notices.sh"),
            ("notices-pitwall", "scripts/notices.sh"),
            ("ci-run-golden", ".gitea/workflows/ci.yml"),
        ];
        for (id, file) in moved {
            let due = graph::due(&world, &reg, &[file.to_string()]);
            assert!(due.iter().any(|d| d.id == id), "{id} must stay due for {file}: {due:?}");
        }
    }

    #[test]
    fn a_merge_is_stale_for_the_generators_both_sides_reach() {
        let root = repo_root();
        let history = GitHistory::new(&root, Duration::from_secs(60));
        let graph_ = read_graph(&root, &history).unwrap();
        let mut world = build_world(&root, &history, &graph_, None).unwrap();
        let reg = registry(&root);
        graph::with_generators(&mut world, &reg);
        // The train 20 shape (instance 1 of ADR 0116): two members each changed a job of ci.yml, and the ci-run golden text-merged.
        let a = vec![".gitea/workflows/ci.yml".to_string()];
        let b = vec![".gitea/workflows/ci.yml".to_string(), "docs/lessons.md".to_string()];
        let stale = graph::stale_after_merge(&world, &reg, &a, &b);
        assert!(stale.iter().any(|d| d.id == "ci-run-golden"), "{stale:?}");
        // control: one side only is not stale (its own regeneration holds)
        let one = graph::stale_after_merge(&world, &reg, &a, &["docs/lessons.md".to_string()]);
        assert!(one.iter().all(|d| d.id != "ci-run-golden"), "{one:?}");
    }
}

/// Every register of the real tree (`docs/*.toml`) has an owner, a named input of docs/repo-graph.toml: an unowned file makes every
/// plan that changes it answer everything (fail closed), so a new register is declared with the change that adds it (a merge of
/// feat/shared-crate-dep-guard brought docs/shared-crate-features.toml without one). The positive control: the registers exist.
#[test]
fn every_register_under_docs_is_owned() {
    use paddock_repo_graph::History;
    let root = repo_root();
    let history = GitHistory::new(&root, Duration::from_secs(120));
    let graph = read_graph(&root, &history).unwrap();
    let files: Vec<String> = history
        .tracked_files()
        .unwrap()
        .into_iter()
        .filter(|f| f.strip_prefix("docs/").is_some_and(|r| !r.contains('/') && r.ends_with(".toml")))
        .collect();
    assert!(files.len() >= 10, "the registers: {files:?}");
    let req = Request {
        files,
        all: false,
        explain: true,
        root: None,
        base: None,
        head: "HEAD".into(),
    };
    let (_, reached) = paddock_dev::repo_model::reach_for(&root, &history, &req, &graph).unwrap();
    assert!(
        reached.unowned.is_empty(),
        "unowned (declare them in docs/repo-graph.toml): {:?}",
        reached.unowned
    );
}
