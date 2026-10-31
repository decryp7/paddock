//! The command line: exit codes, JSON, `log`, `new`, defaults from --repo, git-ref rules sources.
use crate::common::*;
use serde_json::Value;

fn p(d: &tempfile::TempDir) -> String {
    d.path().to_str().unwrap().to_string()
}

#[test]
fn json_is_one_object_with_the_findings() {
    let dir = wiki(&[("index.md", s("# Index\n"))]);
    let (code, out, _) = cli(&["lint", "--json", "--wiki", &p(&dir), "--rules", &p(&dir), "--today", "2026-10-31"]);
    // a directory is not a readable rules file: exit 1 and a JSON error object
    assert_eq!(code, 1);
    assert_eq!(serde_json::from_str::<Value>(out.trim()).unwrap()["ok"], false);
    let rules = dir.path().join("r.txt");
    std::fs::write(&rules, "x\n").unwrap();
    let (code, out, _) = cli(&[
        "lint",
        "--json",
        "--wiki",
        &p(&dir),
        "--rules",
        rules.to_str().unwrap(),
        "--today",
        "2026-10-31",
    ]);
    assert_eq!(code, 5);
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["error_count"], 2);
    assert_eq!(v["errors"][0]["file"].as_str().map(|f| f.ends_with("one.md")), Some(true));
    assert_eq!(v["errors"][0]["message"], "not listed in index.md");
    assert_eq!(out.trim().lines().count(), 1);
}

#[test]
fn usage_errors_exit_2_with_a_next_step() {
    for args in [
        &["lint", "--bogus"][..],
        &["nothing"],
        &[],
        &["lint", "--today", "2026-02-30"],
        &["log", "edit", "x"],
        &["new", "Bad Slug", "--title", "t"],
    ] {
        let dir = wiki(&[]);
        let mut a = args.to_vec();
        let w = p(&dir);
        if args.first().is_some_and(|c| ["lint", "log", "new"].contains(c)) {
            a.extend(["--wiki", &w]);
        }
        let (code, _, err) = cli(&a);
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(err.contains("error"), "{err}");
    }
    assert_eq!(cli(&["--help"]).0, 0);
    assert_eq!(cli(&["--version"]).0, 0);
}

#[test]
fn log_appends_a_line_that_passes_the_lint() {
    let dir = wiki(&[("log.md", s("# Log\n## [2026-10-30] ingest | one | seed"))]); // no final newline
    let (code, out, _) = cli(&["log", "ingest", "one | added a claim", "--wiki", &p(&dir), "--today", "2026-10-31"]);
    assert_eq!(code, 0, "{out}");
    let log = std::fs::read_to_string(dir.path().join("log.md")).unwrap();
    assert_eq!(
        log,
        "# Log\n## [2026-10-30] ingest | one | seed\n## [2026-10-31] ingest | one | added a claim\n"
    );
    assert!(run(&[("log.md", s(&log))]).errors.is_empty());
    // the system clock is used without --today
    let (code, _, _) = cli(&["log", "fix", "all | x", "--wiki", &p(&dir)]);
    assert_eq!(code, 0);
    let last = std::fs::read_to_string(dir.path().join("log.md"))
        .unwrap()
        .lines()
        .last()
        .unwrap()
        .to_string();
    assert!(paddock_wiki::lint::is_log_line(&last), "{last}");
    assert!(last.starts_with(&format!(
        "## [{}] fix | ",
        paddock_wiki::date::format_date(paddock_wiki::date::today_local())
    )));
    // bad input does not touch the file
    let before = std::fs::read_to_string(dir.path().join("log.md")).unwrap();
    for (k, subj) in [("edit", "x"), ("fix", ""), ("fix", "two\nlines")] {
        assert_eq!(cli(&["log", k, subj, "--wiki", &p(&dir)]).0, 2);
    }
    assert_eq!(std::fs::read_to_string(dir.path().join("log.md")).unwrap(), before);
    // no log.md: an I/O error, not a created file
    std::fs::remove_file(dir.path().join("log.md")).unwrap();
    assert_eq!(cli(&["log", "fix", "x", "--wiki", &p(&dir)]).0, 1);
}

#[test]
fn new_creates_a_page_and_its_index_line_and_never_overwrites() {
    let dir = wiki(&[]);
    let w = p(&dir);
    let (code, out, _) = cli(&[
        "new",
        "three-topic",
        "--title",
        "Three topic",
        "--summary",
        "what it covers",
        "--source",
        "a.md",
        "--source",
        "rule 5",
        "--wiki",
        &w,
        "--today",
        "2026-10-31",
    ]);
    assert_eq!(code, 0, "{out}");
    let page = std::fs::read_to_string(dir.path().join("three-topic.md")).unwrap();
    assert!(
        page.starts_with("---\ntitle: Three topic\nverified: 2026-10-31\nsources: [a.md, rule 5]\n---\n"),
        "{page}"
    );
    let index = std::fs::read_to_string(dir.path().join("index.md")).unwrap();
    assert!(
        index.ends_with("- [Three topic](three-topic.md): what it covers. Verified 2026-10-31.\n"),
        "{index}"
    );
    // a new page lints clean once another page links to it
    let one = std::fs::read_to_string(dir.path().join("one.md")).unwrap() + "- See [three](three-topic.md).\n";
    std::fs::write(dir.path().join("one.md"), one).unwrap();
    let out = lint_with(dir.path(), &[], "2026-10-31", 30);
    assert_eq!(out.errors, vec![]);
    // refuses an existing page, leaves it and the index alone
    let (code, _, err) = cli(&["new", "three-topic", "--title", "Other", "--wiki", &w]);
    assert_eq!((code, err.contains("already exists")), (4, true));
    assert_eq!(std::fs::read_to_string(dir.path().join("three-topic.md")).unwrap(), page);
    assert_eq!(std::fs::read_to_string(dir.path().join("index.md")).unwrap(), index);
    // reserved names and traversal
    for slug in ["index", "../evil", "a/b", "Log"] {
        assert_eq!(cli(&["new", slug, "--title", "t", "--wiki", &w]).0, 2, "{slug}");
    }
    // a page without sources is what the lint then asks for
    cli(&["new", "no-sources", "--title", "No sources", "--wiki", &w, "--today", "2026-10-31"]);
    assert!(has(&lint_with(dir.path(), &[], "2026-10-31", 30).errors, "non-empty `sources:`"));
}

#[test]
fn git_ref_sources_run_in_the_repository_and_an_unreadable_one_is_a_warning_with_a_next_step() {
    let rules = "Specs: never wait on a fixed number of event loop turns ever again, in any test.\n";
    let repo = git_repo(rules);
    let wiki_dir = repo.path().join("docs/wiki");
    std::fs::create_dir_all(&wiki_dir).unwrap();
    for (n, t) in [
        (
            "one.md",
            page(
                "2026-10-20",
                "[a]",
                "- Never wait on a fixed number of event loop turns ever again, in any test [two](two.md)\n",
            ),
        ),
        ("two.md", page("2026-10-20", "[a]", "- Two [one](one.md).\n")),
        ("index.md", "- [One](one.md)\n- [Two](two.md)\n".to_string()),
        ("log.md", "# Log\n".to_string()),
    ] {
        std::fs::write(wiki_dir.join(n), t).unwrap();
    }
    let r = p(&repo);
    // an explicit git-ref source is read with git in the repository
    let (code, out, err) = cli(&["lint", "--repo", &r, "--rules", "HEAD:docs/rules.md", "--today", "2026-10-31"]);
    assert_eq!(code, 5, "{out}");
    assert!(err.contains("shared with HEAD:docs/rules.md"), "{err}");
    // an unreadable ref names the source, the cause and the next step, and only warns
    let (code, out, _) = cli(&["lint", "--repo", &r, "--rules", "nope:docs/rules.md", "--today", "2026-10-31"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("warning nope:docs/rules.md") && out.contains("does not exist") && out.contains("fetch-depth: 0"),
        "{out}"
    );
    // a file that cannot be read is an error (exit 1), as the JSON error object too
    let (code, out, _) = cli(&["lint", "--json", "--repo", &r, "--rules", "docs/missing.md"]);
    assert_eq!(code, 1);
    assert_eq!(serde_json::from_str::<Value>(out.trim()).unwrap()["ok"], false);
}

#[test]
fn the_config_file_supplies_settings_and_flags_win() {
    let dir = tempfile::tempdir().unwrap();
    let w = dir.path().join("w");
    std::fs::create_dir_all(&w).unwrap();
    for (n, t) in [
        (
            "one.md",
            page("2026-10-20", "[a]", "- Log mention merge-log.md at 14:33 [two](two.md)\n"),
        ),
        ("two.md", page("2026-10-01", "[a]", "- Two [one](one.md).\n")),
        ("index.md", "- [One](one.md)\n- [Two](two.md)\n".to_string()),
        ("log.md", "# Log\n".to_string()),
    ] {
        std::fs::write(w.join(n), t).unwrap();
    }
    let d = p(&dir);
    std::fs::write(
        dir.path().join("wiki-lint.json"),
        r#"{"wiki": "w", "citeByLine": ["merge-log.md"], "maxAge": 5}"#,
    )
    .unwrap();
    let (code, out, _) = cli(&["lint", "--repo", &d, "--today", "2026-10-31", "--er", &p(&dir)]);
    assert_eq!(code, 1, "an --er that is a folder is unreadable: {out}");
    let (code, out, err) = cli(&["lint", "--repo", &d, "--today", "2026-10-31"]);
    assert_eq!(code, 5, "{out}");
    assert!(
        err.contains("cites merge-log.md by clock time; cite `merge-log.md:<line>` instead"),
        "{out}"
    );
    assert!(out.contains("wiki-lint: 1 error(s), 2 warning(s); 0 rules source(s)"), "{out}");
    assert!(out.contains("stale: verified 2026-10-01, 30 days ago (limit 5)"), "{out}");
    assert!(out.contains("note: no rules source"), "{out}");
    // flags win
    let (_, out, err) = cli(&[
        "lint",
        "--repo",
        &d,
        "--today",
        "2026-10-31",
        "--max-age",
        "90",
        "--cite-by-line",
        "nothing-here",
    ]);
    assert!(!out.contains("stale") && !err.contains("clock time"), "{out}{err}");
    // a bad config is an error that names the file
    std::fs::write(dir.path().join("wiki-lint.json"), r#"{"rules": "docs/rules.md"}"#).unwrap();
    let (code, _, err) = cli(&["lint", "--repo", &d]);
    assert_eq!(code, 1);
    assert!(
        err.contains("wiki-lint.json") && err.contains("`rules` must be a list of strings"),
        "{err}"
    );
    std::fs::write(dir.path().join("wiki-lint.json"), "{").unwrap();
    assert_eq!(cli(&["lint", "--repo", &d]).0, 1);
    assert_eq!(cli(&["lint", "--repo", &d, "--config", "missing.json"]).0, 1);
}

fn ctx(ci: bool) -> paddock_wiki::Context {
    paddock_wiki::Context {
        clock: fixed_clock,
        ci,
        log_dir: None,
    }
}

#[test]
fn today_comes_from_the_injected_clock_for_log_new_and_freshness() {
    let dir = wiki(&[("log.md", s("# Log\n"))]);
    let w = p(&dir);
    let (code, _, _) = cli_in(&["log", "fix", "one | x", "--wiki", &w], &ctx(false));
    assert_eq!(code, 0);
    assert!(
        std::fs::read_to_string(dir.path().join("log.md"))
            .unwrap()
            .ends_with("## [2026-05-17] fix | one | x\n")
    );
    cli_in(&["new", "fresh", "--title", "Fresh", "--wiki", &w], &ctx(false));
    assert!(
        std::fs::read_to_string(dir.path().join("fresh.md"))
            .unwrap()
            .contains("verified: 2026-05-17\n")
    );
    // the pages are verified 2026-10-20: in the future for the injected clock of 2026-05-17
    let (code, _, err) = cli_in(&["lint", "--wiki", &w], &ctx(false));
    assert_eq!(code, 5);
    assert!(err.contains("verified 2026-10-20 is in the future"), "{err}");
}

#[test]
fn ci_mode_prints_annotations_and_every_unreadable_source_has_a_cause_and_a_next_step() {
    let dir = wiki(&[
        ("index.md", s("# Index\n")),
        ("one.md", s(&page("2026-01-01", "[a]", "- x [y](two.md)\n"))),
    ]);
    let w = p(&dir);
    let (code, out, err) = cli_in(&["lint", "--wiki", &w, "--today", "2026-10-31"], &ctx(true));
    assert_eq!(code, 5);
    assert!(
        err.lines()
            .any(|l| l.starts_with("::error::wiki-lint: ") && l.ends_with("two.md: not listed in index.md")),
        "{err}"
    );
    assert!(!err.contains("\nerror "), "annotations replace the plain lines: {err}");
    assert!(
        out.lines()
            .any(|l| l.starts_with("::warning::wiki-lint: ") && l.contains("stale: verified 2026-01-01")),
        "{out}"
    );
    let (code, out, err) = cli_in(&["lint", "--wiki", &w, "--rules", "nowhere/rules.md"], &ctx(true));
    assert_eq!(code, 1, "{out}");
    let line = err.lines().find(|l| l.starts_with("::error::wiki-lint: ")).unwrap_or_default();
    assert!(
        line.contains("nowhere/rules.md") && line.contains("no such file") && line.contains("Fix the path"),
        "{err}"
    );
    // without CI: errors are `error file:line: message` on standard error, warnings on standard output
    let (_, out, err) = cli_in(&["lint", "--wiki", &w, "--today", "2026-10-31"], &ctx(false));
    assert!(
        !err.contains("::") && err.contains("error ") && err.contains("two.md: not listed in index.md"),
        "{err}"
    );
    assert!(
        out.contains("warning ") && out.contains("wiki-lint: 2 error(s), 1 warning(s)"),
        "{out}"
    );
}

#[test]
fn the_config_is_found_from_a_subfolder_and_paths_resolve_against_its_folder() {
    let repo = git_repo("Specs: never wait on a fixed number of event loop turns ever again, in any test.\n");
    let r = repo.path();
    std::fs::create_dir_all(r.join("docs/wiki/deeper")).unwrap();
    for (n, t) in [
        (
            "one.md",
            page(
                "2026-10-20",
                "[a]",
                "- Never wait on a fixed number of event loop turns ever again, in any test [two](two.md)\n",
            ),
        ),
        ("two.md", page("2026-10-20", "[a]", "- Two [one](one.md).\n")),
        ("index.md", "- [One](one.md)\n- [Two](two.md)\n".to_string()),
        ("log.md", "# Log\n".to_string()),
    ] {
        std::fs::write(r.join("docs/wiki").join(n), t).unwrap();
    }
    std::fs::write(r.join("wiki-lint.json"), r#"{"wiki": "docs/wiki", "rules": ["docs/rules.md"]}"#).unwrap();
    let sub = p(&tempfile::TempDir::new_in(r.join("docs/wiki/deeper")).unwrap());
    let (code, out, err) = cli(&["lint", "--repo", &sub, "--today", "2026-10-31"]);
    assert_eq!(code, 5, "{out}");
    assert!(err.contains("shared with docs/rules.md"), "{err}");
    // the same settings found from the repository's top folder, JSON mode
    let (code, out, _) = cli(&["lint", "--json", "--repo", r.to_str().unwrap(), "--today", "2026-10-31"]);
    assert_eq!(code, 5);
    assert_eq!(
        serde_json::from_str::<Value>(out.trim()).unwrap()["rules_sources"][0],
        "docs/rules.md"
    );
}

#[test]
fn rule_branches_are_read_with_git_and_a_missing_ref_or_file_only_warns() {
    let repo = git_repo("Main rules say nothing that the wiki copies in any way at all today.\n");
    let r = repo.path();
    // branch docs/new carries a rule that the wiki copies; branch docs/old has no rules file
    git(r, &["checkout", "-q", "-b", "docs/new"]);
    std::fs::write(
        r.join("docs/rules.md"),
        "A new rule: never wait on a fixed number of event loop turns ever again, in any test.\n",
    )
    .unwrap();
    git(r, &["commit", "-q", "-am", "new rule"]);
    git(r, &["checkout", "-q", "main"]);
    git(r, &["checkout", "-q", "-b", "docs/old"]);
    git(r, &["rm", "-q", "docs/rules.md"]);
    git(r, &["commit", "-q", "-m", "drop"]);
    git(r, &["checkout", "-q", "main"]);
    std::fs::create_dir_all(r.join("docs/wiki")).unwrap();
    for (n, t) in [
        (
            "one.md",
            page(
                "2026-10-20",
                "[a]",
                "- Never wait on a fixed number of event loop turns ever again, in any test [two](two.md)\n",
            ),
        ),
        ("two.md", page("2026-10-20", "[a]", "- Two [one](one.md).\n")),
        ("index.md", "- [One](one.md)\n- [Two](two.md)\n".to_string()),
        ("log.md", "# Log\n".to_string()),
    ] {
        std::fs::write(r.join("docs/wiki").join(n), t).unwrap();
    }
    std::fs::write(
        r.join("wiki-lint.json"),
        r#"{"rules": ["docs/rules.md", "origin/main:docs/rules.md"], "ruleBranches": ["docs/*:docs/rules.md"]}"#,
    )
    .unwrap();
    let d = r.to_str().unwrap();
    let (code, out, err) = cli(&["lint", "--repo", d, "--today", "2026-10-31"]);
    assert_eq!(code, 5, "{out}{err}");
    assert!(err.contains("shared with docs/new:docs/rules.md"), "{err}");
    assert!(!err.contains("shared with docs/rules.md"), "main's rules do not match: {err}");
    // the ref origin/main does not exist here, and docs/old lacks the file: warnings, not errors
    assert!(
        out.contains("warning origin/main:docs/rules.md: ") && out.contains("does not exist"),
        "{out}"
    );
    assert!(out.contains("fetch-depth: 0"), "the next step for CI: {out}");
    assert!(out.contains("warning docs/old:docs/rules.md: "), "{out}");
    // the warnings alone do not fail the lint, but --strict makes them
    std::fs::write(r.join("docs/wiki/one.md"), page("2026-10-20", "[a]", "- Fine [two](two.md)\n")).unwrap();
    assert_eq!(cli(&["lint", "--repo", d, "--today", "2026-10-31"]).0, 0);
    assert_eq!(cli(&["lint", "--repo", d, "--today", "2026-10-31", "--strict"]).0, 5);
}

/// A repository with a two-page wiki whose first page copies the committed rule.
fn repo_with_wiki() -> tempfile::TempDir {
    let repo = git_repo("Specs: never wait on a fixed number of event loop turns ever again, in any test.\n");
    let w = repo.path().join("docs/wiki");
    std::fs::create_dir_all(&w).unwrap();
    for (n, t) in [
        (
            "one.md",
            page(
                "2026-10-20",
                "[a]",
                "- Never wait on a fixed number of event loop turns ever again, in any test [two](two.md)\n",
            ),
        ),
        ("two.md", page("2026-10-20", "[a]", "- Two [one](one.md).\n")),
        ("index.md", "- [One](one.md)\n- [Two](two.md)\n".to_string()),
        ("log.md", "# Log\n".to_string()),
    ] {
        std::fs::write(w.join(n), t).unwrap();
    }
    repo
}

#[test]
fn verbose_shows_phases_every_git_command_with_its_exit_code_and_every_source() {
    let repo = repo_with_wiki();
    let r = p(&repo);
    let (code, _, err) = cli(&[
        "lint",
        "--repo",
        &r,
        "--rules",
        "HEAD:docs/rules.md",
        "--rules",
        "nope:docs/rules.md",
        "--today",
        "2026-10-31",
        "--verbose",
    ]);
    assert_eq!(code, 5, "{err}");
    for key in [
        "phase config discovery done in",
        "settings file: none found, using defaults",
        "phase rules sources done in",
        "git rev-parse --verify --quiet HEAD^{commit} exit 0",
        "git show HEAD:docs/rules.md exit 0",
        "rules source HEAD:docs/rules.md: found",
        "git rev-parse --verify --quiet nope^{commit} exit 1",
        "rules source nope:docs/rules.md: not found",
        "phase pages done in",
        "pages checked: 2 pages (4 files)",
        "summary: wiki-lint: 1 error(s), 1 warning(s); 1 rules source(s)",
    ] {
        assert!(err.contains(key), "missing `{key}` in:\n{err}");
    }
    // not verbose: none of it
    let (_, _, quiet) = cli(&["lint", "--repo", &r, "--rules", "HEAD:docs/rules.md", "--today", "2026-10-31"]);
    assert!(!quiet.contains("[wiki +"), "{quiet}");
}

#[test]
fn log_file_gets_clef_lines_without_file_contents() {
    let repo = repo_with_wiki();
    let r = p(&repo);
    let out_dir = tempfile::tempdir().unwrap();
    let log = out_dir.path().join("wiki.log");
    let (code, _, err) = cli(&[
        "lint",
        "--repo",
        &r,
        "--rules",
        "HEAD:docs/rules.md",
        "--today",
        "2026-10-31",
        "--log-file",
        log.to_str().unwrap(),
    ]);
    assert_eq!(code, 5, "{err}");
    assert!(!err.contains("[wiki +"), "a log file alone does not print: {err}");
    let lines: Vec<Value> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        lines
            .iter()
            .all(|l| l["@t"].is_string() && l["@l"].is_string() && l["@m"].is_string())
    );
    let show = lines.iter().find(|l| l["command"] == "git show HEAD:docs/rules.md").unwrap();
    assert_eq!((show["exit"].as_str(), show["@l"].as_str()), (Some("0"), Some("Information")));
    assert!(lines.iter().any(|l| l["phase"] == "pages" && l["ms"].is_u64()));
    assert!(
        lines.last().unwrap()["@m"]
            .as_str()
            .unwrap()
            .starts_with("summary: wiki-lint: 1 error(s)")
    );
    let all = std::fs::read_to_string(&log).unwrap();
    assert!(!all.contains("event loop turns"), "page and rule text never reach the log");
    // the log is appended to, not replaced
    cli(&["lint", "--repo", &r, "--today", "2026-10-31", "--log-file", log.to_str().unwrap()]);
    assert!(std::fs::read_to_string(&log).unwrap().lines().count() > lines.len());
    // an unusable log file is an error that names it
    let (code, _, err) = cli(&["lint", "--repo", &r, "--log-file", out_dir.path().to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(err.contains("cannot open the log file"), "{err}");
}

#[test]
fn a_failure_in_ci_is_one_error_annotation_with_cause_and_next_step() {
    let repo = repo_with_wiki();
    let r = p(&repo);
    let (code, _, err) = cli_in(&["lint", "--repo", &r, "--rules", "docs/missing.md"], &ctx(true));
    assert_eq!(code, 1);
    let annotations: Vec<&str> = err.lines().filter(|l| l.starts_with("::error::")).collect();
    assert_eq!(annotations.len(), 1, "{err}");
    assert!(
        annotations[0].contains("docs/missing.md") && annotations[0].contains("no such file") && annotations[0].contains("Fix the path"),
        "{err}"
    );
}

fn ctx_logged(dir: &std::path::Path) -> paddock_wiki::Context {
    paddock_wiki::Context {
        clock: fixed_clock,
        ci: true,
        log_dir: Some(dir.to_path_buf()),
    }
}

#[test]
fn with_no_rules_listed_the_defaults_are_docs_rules_and_the_sibling_engineering_rules() {
    // docs/rules.md of the settings folder is read by default (origin/main is absent here and skipped quietly)
    let repo = repo_with_wiki();
    let r = p(&repo);
    let (code, out, err) = cli(&["lint", "--repo", &r, "--today", "2026-10-31"]);
    assert_eq!(code, 5, "{out}");
    assert!(err.contains("shared with docs/rules.md"), "{err}");
    assert!(out.contains("wiki-lint: 1 error(s), 0 warning(s); 1 rules source(s)"), "{out}");
    // ../engineering-rules/RULES.md next to the settings folder is added when it exists
    let outer = tempfile::tempdir().unwrap();
    let project = outer.path().join("project");
    std::fs::create_dir_all(project.join("w")).unwrap();
    std::fs::create_dir_all(outer.path().join("engineering-rules")).unwrap();
    std::fs::write(
        outer.path().join("engineering-rules/RULES.md"),
        "Siblings rule: always check the engineering rules folder next to the repository root before anything else happens.\n",
    )
    .unwrap();
    std::fs::write(project.join("wiki-lint.json"), r#"{"wiki": "w"}"#).unwrap();
    for (n, t) in [
        (
            "one.md",
            page(
                "2026-10-20",
                "[a]",
                "- Always check the engineering rules folder next to the repository root before anything else [two](two.md)\n",
            ),
        ),
        ("two.md", page("2026-10-20", "[a]", "- Two [one](one.md).\n")),
        ("index.md", "- [One](one.md)\n- [Two](two.md)\n".to_string()),
        ("log.md", "# Log\n".to_string()),
    ] {
        std::fs::write(project.join("w").join(n), t).unwrap();
    }
    let (code, _, err) = cli(&["lint", "--repo", project.to_str().unwrap(), "--today", "2026-10-31"]);
    assert_eq!(code, 5, "{err}");
    assert!(err.contains("engineering-rules/RULES.md"), "{err}");
    // an --er that is named but missing is a warning (a ::warning:: in CI), not silence
    let missing = outer.path().join("nowhere/RULES.md");
    let (code, out, _) = cli_in(
        &[
            "lint",
            "--repo",
            project.to_str().unwrap(),
            "--er",
            missing.to_str().unwrap(),
            "--today",
            "2026-10-31",
        ],
        &ctx(true),
    );
    assert_eq!(code, 0, "the named file replaces the sibling default, so nothing is copied: {out}");
    assert!(
        out.lines()
            .any(|l| l.starts_with("::warning::wiki-lint: ") && l.contains("does not exist")),
        "{out}"
    );
}

#[test]
fn every_run_writes_a_log_file_that_starts_with_the_run_and_keeps_only_the_latest_files() {
    let repo = repo_with_wiki();
    let r = p(&repo);
    let logs = tempfile::tempdir().unwrap();
    for i in 0..25 {
        std::fs::write(logs.path().join(format!("wiki-20200101-0000{i:02}-old.log")), "{}\n").unwrap();
    }
    let (code, _, _) = cli_in(&["lint", "--repo", &r, "--today", "2026-10-31"], &ctx_logged(logs.path()));
    assert_eq!(code, 5);
    let mut files: Vec<_> = std::fs::read_dir(logs.path()).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    assert_eq!(files.len(), paddock_wiki::trace::KEEP_LOG_FILES);
    let mine = files.iter().find(|f| !f.to_string_lossy().ends_with("-old.log")).unwrap();
    let lines: Vec<Value> = std::fs::read_to_string(mine)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let first = &lines[0];
    assert!(first["@m"].as_str().unwrap().starts_with("wiki "), "{first}");
    assert_eq!(first["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(first["os"], std::env::consts::OS);
    assert_eq!(first["ci"], true);
    assert!(first["args"].as_array().unwrap().iter().any(|a| a == "lint"));
    let id = first["OperationId"].as_str().unwrap();
    assert!(!id.is_empty() && lines.iter().all(|l| l["OperationId"] == id));
    assert!(mine.to_string_lossy().contains(id), "the file name carries the id");
    let last = lines.last().unwrap();
    assert_eq!(last["exit"], 5, "the summary logs the real exit code: {last}");
}

#[test]
fn a_failing_git_command_logs_the_tail_of_its_standard_error_live() {
    let repo = repo_with_wiki();
    let r = p(&repo);
    let (code, _, err) = cli(&[
        "lint",
        "--repo",
        &r,
        "--rules",
        "HEAD:docs/nope.md",
        "--today",
        "2026-10-31",
        "--verbose",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("git show HEAD:docs/nope.md exit 128"), "{err}");
    assert!(
        err.contains("stderr_tail=fatal: path 'docs/nope.md' does not exist in 'HEAD'"),
        "{err}"
    );
    assert!(err.contains(" started") && err.contains("os=") && err.contains("ci=false"), "{err}");
}

#[test]
fn the_run_log_masks_free_text_and_a_failed_run_names_its_log_file() {
    let dir = wiki(&[("log.md", s("# Log\n"))]);
    let w = p(&dir);
    let logs = tempfile::tempdir().unwrap();
    let ctx = ctx_logged(logs.path());
    let read_logs = || -> String {
        std::fs::read_dir(logs.path())
            .unwrap()
            .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
            .collect()
    };
    let (code, _, _) = cli_in(&["log", "fix", "secret subject text", "--wiki", &w], &ctx);
    assert_eq!(code, 0);
    let (code, _, _) = cli_in(
        &[
            "new",
            "topic",
            "--title",
            "Top Secret Title",
            "--summary",
            "hidden summary",
            "--source",
            "private source",
            "--wiki",
            &w,
        ],
        &ctx,
    );
    assert_eq!(code, 0);
    let all = read_logs();
    for secret in ["secret subject text", "Top Secret Title", "hidden summary", "private source"] {
        assert!(!all.contains(secret), "`{secret}` reached the log");
    }
    assert!(all.contains("<text: 19 chars>"), "the subject's length is logged: {all}");
    assert!(
        all.contains("\"log\"") && all.contains("\"--title\"") && all.contains("\"topic\""),
        "command, flags and slug stay"
    );
    // a failed run names the log of that run, in the message and in the CI annotation
    let (code, _, err) = cli_in(&["lint", "--wiki", &w, "--rules", "nowhere/rules.md"], &ctx);
    assert_eq!(code, 1);
    let file = std::fs::read_dir(logs.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|f| std::fs::read_to_string(f).unwrap().contains("nowhere/rules.md"))
        .unwrap();
    assert!(err.contains(&format!("The log of this run: {}", file.display())), "{err}");
    assert!(
        err.lines()
            .any(|l| l.starts_with("::error::wiki-lint: ") && l.contains("The log of this run: ")),
        "{err}"
    );
}
