//! The Node lint's 18 test cases, ported one to one (the last one runs on this repository's real docs/wiki).
use crate::common::*;
use paddock_wiki::lint::{self, RuleSource};
use paddock_wiki::text::{self, COPY_WORDS, Value};

#[test]
fn a_sound_wiki_has_no_errors_and_no_warnings() {
    let r = run(&[]);
    assert_eq!((r.errors, r.warnings), (vec![], vec![]));
}

#[test]
fn a_broken_link_and_a_missing_heading_anchor_are_errors() {
    let r = run(&[(
        "one.md",
        s(&page(
            "2026-10-20",
            "[a.md]",
            "- Gone [x](missing.md) and [y](two.md#nope), ok [z](two.md#current-state).\n",
        )),
    )]);
    assert!(has(&r.errors, "missing.md does not exist"));
    assert!(has(&r.errors, "two.md has no heading #nope"));
    assert!(!has(&r.errors, "#current-state"));
}

#[test]
fn links_inside_code_are_not_links_absolute_urls_and_pure_anchors_are_not_checked() {
    let links = text::links_of("See `[x](gone.md)` and [a](https://e.org/x) and [b](#top) and [c](d.md#h).\n```\n[f](g.md)\n```");
    assert_eq!(links.len(), 1);
    assert_eq!(
        (links[0].target.as_str(), links[0].anchor.as_str(), links[0].line),
        ("d.md", "h", 1)
    );
}

#[test]
fn a_page_missing_from_the_index_is_an_error() {
    let r = run(&[("index.md", s("# Index\n- [One](one.md)\n"))]);
    assert!(has(&r.errors, "two.md: not listed in index.md"));
}

#[test]
fn an_orphan_is_an_error() {
    let r = run(&[
        ("two.md", s(&page("2026-10-20", "[a.md]", "- Two alone [x](one.md).\n"))),
        ("one.md", s(&page("2026-10-20", "[a.md]", "- One alone [x](two.md).\n"))),
        ("three.md", s(&page("2026-10-20", "[a.md]", "- Three [x](one.md).\n"))),
        ("index.md", s("- [One](one.md)\n- [Two](two.md)\n- [Three](three.md)\n")),
    ]);
    assert!(has(&r.errors, "three.md: orphan"));
    assert!(!has(&r.errors, "one.md: orphan"));
}

#[test]
fn front_matter_needs_a_real_verified_date_and_a_non_empty_sources_list() {
    assert!(has(
        &run(&[("one.md", s("---\nsources: [a]\n---\n- x [y](two.md)\n"))]).errors,
        "verified: YYYY-MM-DD"
    ));
    assert!(has(
        &run(&[("one.md", s(&page("2026-13-45", "[a.md]", "- x [y](two.md)\n")))]).errors,
        "verified: YYYY-MM-DD"
    ));
    assert!(has(
        &run(&[("one.md", s(&page("2026-10-20", "[]", "- x [y](two.md)\n")))]).errors,
        "non-empty `sources:`"
    ));
    assert!(has(&run(&[("one.md", s("- x [y](two.md)\n"))]).errors, "verified: YYYY-MM-DD"));
}

#[test]
fn sources_may_be_a_block_list() {
    let t = "---\ntitle: T\nverified: 2026-10-20\nsources:\n  - a.md\n  - b.md\n---\n- c [two](two.md)\n";
    assert_eq!(
        text::parse_page(t).meta.get("sources"),
        Some(&Value::List(vec!["a.md".into(), "b.md".into()]))
    );
    assert_eq!(run(&[("one.md", s(t))]).errors, vec![]);
}

#[test]
fn a_page_older_than_the_limit_warns_and_a_future_date_is_an_error() {
    let old = run(&[("one.md", s(&page("2026-09-01", "[a.md]", "- x [y](two.md)\n")))]);
    assert_eq!(old.errors, vec![]);
    assert!(has(&old.warnings, "stale: verified 2026-09-01, 60 days ago"));
    let w = |d: &str| run(&[("one.md", s(&page(d, "[a.md]", "- x [y](two.md)\n")))]).warnings.len();
    assert_eq!(w("2026-09-30"), 1); // 31 days is over the limit of 30
    assert_eq!(w("2026-10-01"), 0); // exactly 30 days is inside
    assert!(has(
        &run(&[("one.md", s(&page("2026-11-30", "[a.md]", "- x [y](two.md)\n")))]).errors,
        "in the future"
    ));
}

#[test]
fn a_bullet_without_a_source_is_an_error_a_continuation_may_carry_it_and_src_counts() {
    let one = |body: &str| run(&[("one.md", s(&page("2026-10-20", "[a]", body)))]);
    assert!(has(
        &one("- Bare claim.\n- Linked [two](two.md).\n").errors,
        "one.md:8: bullet names no source"
    ));
    assert_eq!(
        one("- Wrapped claim\n  continues here [two](two.md).\n- By id, src: `run 7158`.\n").errors,
        vec![]
    );
    assert_eq!(one("```\n- not a claim\n```\n- Claim [two](two.md).\n").errors, vec![]);
}

#[test]
fn the_log_lines_must_keep_their_shape() {
    assert!(has(&run(&[("log.md", s("## 2026-10-31 something\n"))]).errors, "log line must be"));
    assert!(has(
        &run(&[("log.md", s("## [2026-10-31] edit | one | x\n"))]).errors,
        "log line must be"
    ));
}

#[test]
fn copying_a_run_of_rule_words_fails_a_paraphrase_passes_the_run_length_is_the_rule() {
    let one = |body: &str| run(&[("one.md", s(&page("2026-10-20", "[a]", body)))]);
    let copy = "The coordinator watches load and free disk continuously and keeps the machine busy.";
    assert!(has(&one(&format!("- {copy} [two](two.md)\n")).errors, "one.md: copies rule text"));
    assert_eq!(
        one("- Load is watched, disk too; more agents start when it is low [two](two.md)\n").errors,
        vec![]
    );
    let wrapped = one("- The coordinator watches load and free disk\n  continuously and keeps the machine busy [two](two.md)\n");
    assert!(has(&wrapped.errors, "copies rule text"));
    assert_eq!(COPY_WORDS, 8);
    assert_eq!(text::windows("a b c d e f g h i", 8).len(), 2);
    assert_eq!(text::windows("a b c d e f g", 8).len(), 0);
}

#[test]
fn index_and_log_are_required() {
    assert!(has(&run(&[("index.md", None)]).errors, "missing index.md"));
    assert!(has(&run(&[("log.md", None)]).errors, "missing log.md"));
}

#[test]
fn slugs_follow_the_forge() {
    assert_eq!(text::slug("CI economy (user, 2026-10-29)"), "ci-economy-user-2026-10-29");
    assert_eq!(text::slug("A `pid` is not a process"), "a-pid-is-not-a-process");
}

#[test]
fn the_cli_exits_1_on_errors_and_on_warnings_only_with_strict() {
    let stale = wiki(&[("one.md", s(&page("2026-09-01", "[a.md]", "- x [y](two.md)\n")))]);
    let rules = stale.path().join("rules.txt");
    std::fs::write(&rules, RULES).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let none = outside.path().join("none.md");
    let base = |extra: &[&str]| {
        let mut a = vec![
            "lint",
            "--wiki",
            stale.path().to_str().unwrap(),
            "--rules",
            rules.to_str().unwrap(),
            "--er",
            none.to_str().unwrap(),
            "--today",
            "2026-10-31",
        ];
        a.extend_from_slice(extra);
        cli(&a).0
    };
    // --er names a file that does not exist: it is skipped (it is an extra source), as the Node script did.
    assert_eq!(base(&[]), 0);
    std::fs::write(&none, "Nothing shared with the pages.\n").unwrap();
    assert_eq!(base(&[]), 0);
    assert_eq!(base(&["--strict"]), 1);
    assert_eq!(base(&["--max-age", "90", "--strict"]), 0);
    let bad = wiki(&[("index.md", s("# Index\n"))]);
    let a = [
        "lint",
        "--wiki",
        bad.path().to_str().unwrap(),
        "--rules",
        rules.to_str().unwrap(),
        "--er",
        none.to_str().unwrap(),
        "--today",
        "2026-10-31",
    ];
    assert_eq!(cli(&a).0, 1);
    // an unreadable rules source is exit 1 (LINT.md) and names the source
    let mut a = a.to_vec();
    let missing = stale.path().join("unreadable.txt");
    a.extend(["--rules", missing.to_str().unwrap()]);
    let (code, _, err) = cli(&a);
    assert_eq!(code, 1);
    assert!(err.contains("unreadable.txt"), "{err}");
}

#[test]
fn a_copy_of_any_one_of_several_rules_sources_fails_and_the_message_names_that_source() {
    let dir = wiki(&[(
        "one.md",
        s(&page(
            "2026-10-20",
            "[a]",
            "- Branch rule says never wait on a fixed number of event loop turns ever [two](two.md)\n",
        )),
    )]);
    let rules = [
        RuleSource {
            name: "rules.txt".into(),
            text: RULES.into(),
        },
        RuleSource {
            name: "origin/docs/branch:docs/rules.md".into(),
            text: "Specs: never wait on a fixed number of event loop turns ever again.".into(),
        },
    ];
    let r = lint_with(dir.path(), &rules, "2026-10-31", 30);
    assert!(has(&r.errors, "shared with origin/docs/branch:docs/rules.md"));
    assert!(!has(&r.errors, "shared with rules.txt"));
}

#[test]
fn a_steward_log_citation_by_clock_time_is_an_error_by_line_number_it_passes() {
    let one = |body: &str| run(&[("one.md", s(&page("2026-10-20", "[a]", body)))]);
    assert!(has(
        &one("- Claim, src: `merge-steward.md` entry 14:33 [two](two.md)\n").errors,
        "clock time"
    ));
    assert_eq!(one("- Claim, src: `merge-steward.md:560` [two](two.md)\n").errors, vec![]);
    assert_eq!(one("- The runner starts at 14:33 [two](two.md)\n").errors, vec![]);
}

#[test]
fn a_rules_source_is_a_file_or_a_git_ref_path_or_unreadable() {
    use paddock_wiki::sources::read;
    let off = paddock_wiki::trace::Trace::off();
    let repo = git_repo("# Test rules\nOne rule here.\n");
    let file = repo.path().join("docs/rules.md");
    let f = file.to_str().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    assert_eq!(read(f, f, elsewhere.path(), &off).unwrap().text, "# Test rules\nOne rule here.\n");
    let e = read("no/such/file.md", "no/such/file.md", repo.path(), &off).unwrap_err();
    assert!(e.cause.contains("no/such/file.md") && !e.next_step.is_empty() && !e.is_ref, "{e:?}");
    // an unreadable `<ref>:<path>` (the ref is not fetched, or the path is absent on it) is flagged for a warning
    let e = read("no-such-ref-xyz:docs/rules.md", "no-such-ref-xyz:docs/rules.md", repo.path(), &off).unwrap_err();
    assert!(
        e.is_ref && e.cause.contains("no-such-ref-xyz") && e.next_step.contains("git fetch"),
        "{e:?}"
    );
    let e = read("HEAD:docs/nope.md", "HEAD:docs/nope.md", repo.path(), &off).unwrap_err();
    assert!(e.is_ref && e.cause.contains("HEAD:docs/nope.md"), "{e:?}");
    // git runs in the repository given, not in the folder the tool runs from
    assert_eq!(
        read("HEAD:docs/rules.md", "HEAD:docs/rules.md", repo.path(), &off).unwrap().text,
        "# Test rules\nOne rule here.\n"
    );
    assert!(read("HEAD:docs/rules.md", "HEAD:docs/rules.md", elsewhere.path(), &off).is_err());
}

#[test]
fn the_real_docs_wiki_passes_against_docs_rules_stale_pages_only_warn() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let rules = std::fs::read_to_string(repo.join("docs/rules.md")).unwrap();
    let today = paddock_wiki::date::format_date(paddock_wiki::date::today_local());
    let r = lint_with(
        &repo.join("docs/wiki"),
        &[RuleSource {
            name: "docs/rules.md".into(),
            text: rules,
        }],
        &today,
        lint::MAX_AGE_DAYS,
    );
    assert_eq!(r.errors, vec![]);
}
