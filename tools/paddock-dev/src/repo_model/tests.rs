//! `--compare` and the batched reader, with their positive controls (ADR 0117 §8).
#![allow(clippy::unwrap_used)]

use super::*;
use paddock_repo_graph::Graph;
use paddock_repo_graph::model::NodeReach;

fn graph() -> RepoGraph {
    RepoGraph {
        cargo: Graph::new(),
        npm: Graph::new(),
        apps: ["dyno".to_string()].into_iter().collect(),
        configs: BTreeMap::new(),
        ..Default::default()
    }
}

fn declared_inputs() -> Inputs {
    crate::repo_inputs::load(&repo_root()).unwrap()
}

fn hit(reach: Reach, rust: bool, ui: bool) -> NodeReach {
    NodeReach {
        reach,
        rust,
        ui,
        why: vec!["model reason".into()],
    }
}

fn plan() -> Value {
    json!({"all": false, "crates": true, "ui": false, "eslint": false, "conformance": true, "c99": false, "tools": false,
           "icons": false, "ciImage": false, "paddockDev": false, "docs": false, "apps": ["dyno"],
           "kinds": {"dyno": ["rust"]}, "why": {"crates": ["crates/a/src/lib.rs"]}})
}

#[test]
fn the_same_answer_is_same() {
    let mut r = Reached::default();
    r.nodes.insert("crates".into(), hit(Reach::Code, true, true));
    r.nodes.insert("app:dyno".into(), hit(Reach::Code, true, false));
    let c = compare(&plan(), &r, &graph(), &declared_inputs());
    assert_eq!(c["same"], true, "{c}");
}

#[test]
fn compare_reports_a_rule_the_model_lacks_and_names_what_it_drops() {
    // Positive control (§8 "--compare"): take the app's edge out of the model's answer; --compare names the difference and the drop.
    let mut r = Reached::default();
    r.nodes.insert("crates".into(), hit(Reach::Code, true, true));
    let c = compare(&plan(), &r, &graph(), &declared_inputs());
    assert_eq!(c["same"], false);
    assert_eq!(c["dropped"], json!(["app:dyno"]));
    assert_eq!(c["differences"][0]["project"], "app:dyno");
    assert_eq!(c["differences"][0]["rules"], "rust");
    assert_eq!(c["differences"][0]["model"], Value::Null);
}

#[test]
fn a_docs_reach_runs_nothing_and_a_half_is_a_narrowing() {
    let mut r = Reached::default();
    r.nodes.insert("crates".into(), hit(Reach::Code, true, true));
    r.nodes.insert("app:dyno".into(), hit(Reach::Docs, false, false));
    assert_eq!(compare(&plan(), &r, &graph(), &declared_inputs())["dropped"], json!(["app:dyno"]));
    let mut p = plan();
    p["kinds"]["dyno"] = json!(["all"]);
    let mut r2 = Reached::default();
    r2.nodes.insert("crates".into(), hit(Reach::Code, true, true));
    r2.nodes.insert("app:dyno".into(), hit(Reach::Code, false, true));
    let c = compare(&p, &r2, &graph(), &declared_inputs());
    assert_eq!(c["dropped"], json!(["app:dyno"]), "{c}");
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn the_bulk_reader_starts_at_most_one_git_for_many_files_and_the_control_counts_one_per_reader() {
    // ER-20, ADR 0117 §9: one `git cat-file --batch-command` for every text. The control: a reader per file starts one git each,
    // so the counter sees spawns.
    let root = repo_root();
    let files: Vec<String> = ["Cargo.toml", "Cargo.lock", "src/lib.rs", "src/plan.rs", "src/repo_model.rs"]
        .iter()
        .map(|f| format!("tools/paddock-dev/{f}"))
        .collect();
    let one = GitBulk::through_git(&root, Some("HEAD"));
    let texts = one.read_all(&files);
    assert!(texts.iter().filter(|t| matches!(t, Ok(Some(_)))).count() >= 4, "the files are read");
    assert_eq!(one.starts(), 1, "one process for every read");
    let per_file: usize = files
        .iter()
        .map(|f| {
            let r = GitBulk::through_git(&root, Some("HEAD"));
            r.read_all(std::slice::from_ref(f));
            r.starts()
        })
        .sum();
    assert_eq!(per_file, files.len(), "the control: one reader per file starts one git each");
}

/// The repository's own declarations (docs/repo-graph.toml through the one accessor) on a small world: the behaviours the crate's
/// tests pin on fixture data, here on the real file.
mod declared {
    use super::*;
    use paddock_repo_graph::Node;
    use paddock_repo_graph::model::inputs::Inputs;
    use paddock_repo_graph::model::{Facts, reach};

    fn node(name: &str, ws: &str, deps: &[&str]) -> Node {
        Node {
            name: name.into(),
            workspace: ws.into(),
            deps: deps.iter().map(|d| d.to_string()).collect(),
        }
    }

    fn inputs() -> Inputs {
        crate::repo_inputs::load(&repo_root()).unwrap()
    }

    fn run(changed: &[&str], inputs: &Inputs, text: paddock_repo_graph::model::TextOf) -> Reached {
        let mut cargo = Graph::new();
        cargo.insert("crates/paddock-proc", node("paddock-proc", "crates", &[]));
        cargo.insert("apps/dyno/app/src-tauri", node("dyno", "apps/dyno", &["crates/paddock-proc"]));
        let mut npm = Graph::new();
        npm.insert("ui", node("ui", "", &[]));
        npm.insert("apps/dyno/app", node("dyno-app", "", &["ui"]));
        let projects: Vec<(String, String)> = inputs
            .projects
            .iter()
            .map(|p| (p.id.clone(), p.folder.clone()))
            .chain([("app:dyno".to_string(), "apps/dyno".to_string())])
            .collect();
        let files: Vec<String> = ["apps/dyno/scripts/run.ps1", "crates/paddock-proc/src/lib.rs"]
            .map(String::from)
            .to_vec();
        let facts = Facts {
            cargo: &cargo,
            npm: &npm,
            projects: &projects,
            files: &files,
            detect: None,
            model: None,
            seeds: &[],
        };
        let changed: Vec<String> = changed.iter().map(|s| s.to_string()).collect();
        reach(&changed, &facts, inputs, text)
    }

    fn none(_: &str) -> (Option<String>, Option<String>) {
        (None, None)
    }

    #[test]
    fn the_gaps_close_on_the_declared_inputs() {
        let i = inputs();
        // G3: a spike is not built, an unknown root file fails closed.
        let s = run(&["spikes/dialog-hidden/Cargo.lock"], &i, &none);
        assert!(s.nodes.is_empty() && !s.everything);
        assert!(run(&["mystery.cfg"], &i, &none).everything);
        // G7 and G8: a README under .gitea and a check-id reservation reach no project's tests.
        let g = run(&[".gitea/actions/README.md", "conformance/reserved/C999-x"], &i, &none);
        assert!(
            !g.everything && !g.nodes.keys().any(|k| k.starts_with("app:")),
            "{:?}",
            g.nodes.keys()
        );
        // G13: the lessons file is code for its readers.
        assert_eq!(run(&["docs/lessons.md"], &i, &none).nodes["conformance"].reach, Reach::Code);
        // G2: a merge-driver change reaches conformance only; an eol change reaches the owners of what it matches.
        let merge = |_: &str| {
            (
                Some("/docs/lessons.md merge=union\n".to_string()),
                Some("/docs/lessons.md merge=x\n".to_string()),
            )
        };
        assert_eq!(
            run(&[".gitattributes"], &i, &merge).nodes.keys().collect::<Vec<_>>(),
            ["conformance"]
        );
        let eol = |_: &str| (Some(String::new()), Some("*.ps1 eol=crlf\n".to_string()));
        assert!(run(&[".gitattributes"], &i, &eol).nodes.contains_key("app:dyno"));
        // Round 3 P1-A: test data in a project folder is code; a README is prose.
        assert_eq!(
            run(&["crates/paddock-proc/tests/recorded/x.txt"], &i, &none).nodes["crates"].reach,
            Reach::Code
        );
        let readme = format!("crates/paddock-proc/{}", "README.md");
        assert_eq!(run(&[readme.as_str()], &i, &none).nodes["crates"].reach, Reach::Docs);
        // The true globals reach every project.
        assert!(run(&["rust-toolchain.toml"], &i, &none).nodes.len() >= i.projects.len());
        assert!(
            !run(&["ui/package.json"], &i, &none).nodes.contains_key("tools"),
            "a leading slash anchors package.json at the root"
        );
    }

    #[test]
    fn each_additive_input_adds_its_reader_and_the_control_removes_it() {
        let cases = [
            ("c99-reads", "crates/paddock-proc/src/lib.rs", "c99"),
            ("cause-code-scan", "crates/paddock-proc/src/lib.rs", "ui"),
            ("c100-reads", "apps/x/AGENTS.md", "conformance"),
            ("conformance-parity", "conformance/src/checks/c1.mjs", "paddock-dev"),
            ("hooks-parity", "tools/paddock-dev/src/places.rs", "tools"),
            (
                "app-capabilities",
                "apps/dyno/app/src-tauri/capabilities/default.json",
                "conformance",
            ),
            ("per-machine", "apps/dyno/app/src-tauri/windows/hooks.nsh", "conformance"),
            ("e2e-guards", "ui/projects/paddock-e2e/src/env.ts", "app:dyno"),
            ("notices", "scripts/notices.sh", "app:dyno"),
            ("generated-bindings", "ui/projects/paddock-ui/src/generated/health.ts", "crates"),
            ("templates", "templates/brief-template.toml", "paddock-dev"),
            ("gitea-labels", ".gitea/labels.toml", "paddock-dev"),
        ];
        for (name, file, reader) in cases {
            let all = inputs();
            let by = |r: &Reached| {
                r.nodes
                    .get(reader)
                    .is_some_and(|n| n.why.iter().any(|y| y.starts_with(&format!("input {name} "))))
            };
            assert!(by(&run(&[file], &all, &none)), "{name}: {file} must reach {reader}");
            let mut without = inputs();
            without.inputs.retain(|i| i.name != name);
            assert!(!by(&run(&[file], &without, &none)), "{name}: the control still reaches {reader}");
        }
    }
}

/// The model's adapter code in paddock-dev: its non-test sources (ADR 0117 §14).
const GUARDED: [&str; 4] = ["repo_model.rs", "repo_model/ci.rs", "repo_model/code.rs", "repo_inputs.rs"];

/// Tokens the adapters read because a format names them, not because of this repository: the extensions of the languages and
/// configuration files they parse.
const FORMAT_TOKENS: [&str; 24] = [
    ".sh",
    ".mjs",
    ".cjs",
    ".js",
    ".ps1",
    ".py",
    ".ts",
    ".mts",
    ".md",
    ".yml",
    ".yaml",
    ".spec.ts",
    ".test.ts",
    ".spec.mjs",
    ".test.mjs",
    "*.rs",
    "/node_modules/",
    "/action.yml",
    "/action.yaml",
    "$GITHUB_WORKSPACE/",
    ".files",
    ".reaches",
    ".except",
    ".paths",
];

fn guarded_sources() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    GUARDED
        .iter()
        .map(|f| (f.to_string(), std::fs::read_to_string(src.join(f)).unwrap()))
        .collect()
}

fn model_findings(files: &[(String, String)]) -> Vec<String> {
    let mut allowed: Vec<(&str, &str)> = files
        .iter()
        .flat_map(|(n, _)| FORMAT_TOKENS.iter().map(move |t| (n.as_str(), *t)))
        .collect();
    // The accessor is the one place that names the inputs file (ER-42); the file's path is its constant.
    allowed.push(("repo_inputs.rs", "docs/repo-graph.toml"));
    paddock_repo_graph::model::guard::findings(files, &allowed)
}

#[test]
fn the_paddock_dev_model_modules_hold_no_path_literal() {
    let files = guarded_sources();
    let found = model_findings(&files);
    assert!(
        found.is_empty(),
        "path facts belong in docs/repo-graph.toml (read through repo_inputs): {found:#?}"
    );
}

#[test]
fn the_bulk_reader_keeps_absent_and_failed_apart() {
    // Failing first (P1-5): `.ok().flatten()` turned every read error into "absent".
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("ok.txt"), "text").unwrap();
    std::fs::write(d.path().join("bin.dat"), [0xff, 0xfe, 0x00]).unwrap();
    std::fs::create_dir(d.path().join("folder")).unwrap();
    let r = GitBulk::new(d.path(), None);
    let got = r.read_all(&["ok.txt", "missing.txt", "bin.dat", "folder"].map(String::from));
    assert_eq!(got[0], Ok(Some("text".to_string())));
    assert_eq!(got[1], Ok(None), "a file that is not there is absent, not an error");
    assert!(got[2].as_ref().unwrap_err().contains("not UTF-8"), "{:?}", got[2]);
    assert!(got[3].is_err(), "a folder cannot be read as text: {:?}", got[3]);
    // A revision git does not know is an error for every path, never absent.
    let root = repo_root();
    let rev = GitBulk::through_git(&root, Some("no-such-revision-0000"));
    let got = rev.read_all(&["Cargo.toml".to_string(), "dev".to_string()]);
    assert!(got.iter().all(Result::is_err), "{got:?}");
    // Control: a real revision reads, and a missing path in it is absent.
    let head = GitBulk::through_git(&root, Some("HEAD"));
    let got = head.read_all(&["tools/paddock-dev/Cargo.toml".to_string(), "no/such/file.txt".to_string()]);
    assert!(matches!(got[0], Ok(Some(_))) && got[1] == Ok(None), "{got:?}");
}

/// A throw-away repository with `docs/repo-graph.toml`, a workflow, and `n` source files changed by its second commit.
fn two_commits(n: usize) -> (tempfile::TempDir, String, String) {
    let d = tempfile::tempdir().unwrap();
    let git_in = |args: &[&str]| {
        let mut c = crate::git_env::git();
        c.arg("-C")
            .arg(d.path())
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args);
        let out = c.output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git_in(&["init", "-q"]);
    std::fs::create_dir_all(d.path().join("docs")).unwrap();
    std::fs::create_dir_all(d.path().join(".gitea/workflows")).unwrap();
    std::fs::create_dir_all(d.path().join("tools/src")).unwrap();
    std::fs::copy(repo_root().join("docs/repo-graph.toml"), d.path().join("docs/repo-graph.toml")).unwrap();
    std::fs::write(
        d.path().join(".gitea/workflows/ci.yml"),
        "name: ci\non: [push]\njobs:\n  a:\n    steps:\n      - run: echo 1\n",
    )
    .unwrap();
    for i in 0..n {
        std::fs::write(d.path().join(format!("tools/src/f{i}.txt")), "one").unwrap();
    }
    git_in(&["add", "-A"]);
    git_in(&["commit", "-q", "-m", "one"]);
    let base = git_in(&["rev-parse", "HEAD"]);
    for i in 0..n {
        std::fs::write(d.path().join(format!("tools/src/f{i}.txt")), "two").unwrap();
    }
    git_in(&["add", "-A"]);
    git_in(&["commit", "-q", "-m", "two"]);
    let head = git_in(&["rev-parse", "HEAD"]);
    (d, base, head)
}

fn request(base: &str, head: &str) -> Request {
    Request {
        files: Vec::new(),
        all: false,
        explain: false,
        root: None,
        base: Some(base.into()),
        head: head.into(),
    }
}

#[test]
fn reach_for_starts_one_reader_and_one_grep_whatever_the_number_of_changed_files() {
    // Failing first (P2 spawn budget): reach_for opened two readers (the working-tree one and the head one) and asked per item.
    // The whole process count is pinned: one `git cat-file` reader for every read (the tracked files, the inputs file, both sides
    // of every changed file, every adapter text) and the code adapter's one `git grep`; it does not grow with the change.
    for n in [3, 40] {
        let (d, base, head) = two_commits(n);
        let history = paddock_repo_graph::GitHistory::new(d.path(), std::time::Duration::from_secs(60));
        let bulk = GitBulk::through_git(d.path(), Some(&head));
        let (changed, _, spawns) = reach_with(d.path(), &history, &request(&base, &head), &RepoGraph::default(), &bulk).unwrap();
        assert_eq!(changed.len(), n);
        assert_eq!(spawns, Spawns { reader: 1, adapters: 1 }, "{n} changed files");
    }
}

#[test]
fn reach_for_reads_the_requested_revision_not_the_checkout() {
    // Failing first (P2 mixed revisions): the tracked files, git grep and the inputs file came from the working tree. A replay of
    // old commits must answer the same whatever the checkout holds: the working tree here lost the inputs file and gained files.
    let (d, base, head) = two_commits(3);
    let history = paddock_repo_graph::GitHistory::new(d.path(), std::time::Duration::from_secs(60));
    let run = || {
        let bulk = GitBulk::new(d.path(), Some(&head));
        let (_, r, _) = reach_with(d.path(), &history, &request(&base, &head), &RepoGraph::default(), &bulk).unwrap();
        format!("{:?}", r.nodes)
    };
    let clean = run();
    std::fs::remove_file(d.path().join("docs/repo-graph.toml")).unwrap();
    std::fs::write(d.path().join("tools/src/f0.txt"), "dirty").unwrap();
    std::fs::write(d.path().join("untracked.rs"), "include_str!(\"x\")").unwrap();
    assert_eq!(run(), clean, "the answer does not depend on the checkout");
    // Control: a revision without the inputs file is an error, not an empty set of declarations.
    let e = reach_with(
        d.path(),
        &history,
        &request(&base, &head),
        &RepoGraph::default(),
        &GitBulk::new(d.path(), Some(&base)),
    );
    assert!(e.is_ok(), "the base revision has the file too");
    let (_, no_file_head) = {
        let git_in = |args: &[&str]| {
            let mut c = crate::git_env::git();
            c.arg("-C")
                .arg(d.path())
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args(args);
            let o = c.output().unwrap();
            assert!(o.status.success());
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        git_in(&["rm", "-q", "docs/repo-graph.toml"]);
        git_in(&["commit", "-q", "-m", "drop"]);
        ((), git_in(&["rev-parse", "HEAD"]))
    };
    let err = reach_with(
        d.path(),
        &history,
        &request(&head, &no_file_head),
        &RepoGraph::default(),
        &GitBulk::new(d.path(), Some(&no_file_head)),
    )
    .unwrap_err();
    assert!(err.contains("repo-graph.toml"), "{err}");
}

#[test]
fn the_guard_sees_a_path_fact_put_back_into_each_paddock_dev_module() {
    // Positive control (ER-33): a path literal put back at the end of each guarded file, after its test module declaration, is found;
    // and a literal in a `#[cfg(test)]` block is not.
    let files = guarded_sources();
    assert!(model_findings(&files).is_empty());
    for i in 0..files.len() {
        let mut changed = files.clone();
        changed[i].1.push_str("\nfn late() -> &'static str { \"conformance/ci/\" }\n");
        let last = changed[i].1.lines().count();
        let found = model_findings(&changed);
        assert!(
            found.iter().any(|f| f.starts_with(&format!("{}:{last}:", changed[i].0))),
            "{}: {found:?}",
            changed[i].0
        );
        let mut test_only = files.clone();
        test_only[i]
            .1
            .push_str("\n#[cfg(test)]\nmod late {\n    const X: &str = \"conformance/ci/\";\n}\n");
        assert!(model_findings(&test_only).is_empty());
    }
}
