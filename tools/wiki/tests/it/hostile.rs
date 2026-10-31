//! Parsers never panic on any input, and a hostile wiki folder is bounded.
use crate::common::*;
use paddock_wiki::{lint, ops, sources, text};

const SEEDS: &[&[u8]] = &[
    b"---\ntitle: T\nverified: 2026-10-20\nsources: [a, b]\n---\n- x [y](z.md#h) `code` src: q\n",
    b"---\n---\n",
    b"---\nsources:\n  - \n  -\n---",
    b"[[[[[[(((((]]]]]))))) [a](",
    b"```\n- [x](y)\n  ```\n\xff\xfe\xc3(",
    "## \u{1F680} \u{2028} # \n#\n\n".as_bytes(),
    b"merge-steward 14:33 \xe2\x80 99:99 1:2:3 :: 12:34\xc3\xa9",
    b"## [2026-10-3\xc3\xa9] fix | \xe2",
    "\u{feff}key: \u{a0}value\u{2029}x".as_bytes(),
];

fn t(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn no_text_parser_panics_on_hostile_input() {
    paddock_testkit::assert_never_panics(SEEDS, |b| text::parse_page(&t(b)).meta.len());
    paddock_testkit::assert_never_panics(SEEDS, |b| text::links_of(&t(b)).len());
    paddock_testkit::assert_never_panics(SEEDS, |b| text::headings_of(&t(b)).len());
    paddock_testkit::assert_never_panics(SEEDS, |b| text::bullets_without_source(&t(b)).len());
    paddock_testkit::assert_never_panics(SEEDS, |b| text::windows(&t(b), 8).len());
    paddock_testkit::assert_never_panics(SEEDS, |b| text::slug(&t(b)));
    paddock_testkit::assert_never_panics(SEEDS, |b| t(b).lines().filter(|l| text::has_clock_time(l)).count());
    paddock_testkit::assert_never_panics(SEEDS, |b| t(b).lines().filter(|l| lint::is_log_line(l)).count());
    paddock_testkit::assert_never_panics(SEEDS, |b| paddock_wiki::date::parse_date(&t(b)));
    paddock_testkit::assert_never_panics(SEEDS, |b| sources::looks_like_git_ref(&t(b)));
    paddock_testkit::assert_never_panics(SEEDS, |b| ops::check_slug(&t(b)));
}

#[test]
fn a_whole_wiki_of_hostile_pages_lints_without_panicking() {
    let pages: Vec<(String, Option<String>)> = SEEDS.iter().enumerate().map(|(i, b)| (format!("p{i}.md"), Some(t(b)))).collect();
    let over: Vec<(&str, Option<String>)> = pages.iter().map(|(n, c)| (n.as_str(), c.clone())).collect();
    let r = run(&over);
    assert!(!r.errors.is_empty());
}

#[test]
fn adversarial_shapes_stay_linear() {
    // Many unclosed brackets and backticks on one line must not take quadratic time.
    let line = "[".repeat(200_000) + &"`".repeat(1000);
    let start = std::time::Instant::now();
    assert!(text::links_of(&line).is_empty());
    assert!(text::windows(&line, 8).is_empty());
    let many = "[a](".repeat(50_000);
    assert!(text::windows(&many, 8).len() <= 1);
    assert!(start.elapsed().as_secs() < 20, "took {:?}", start.elapsed());
}

#[test]
fn a_file_over_the_limit_is_refused_with_its_name() {
    let dir = wiki(&[]);
    let big = dir.path().join("big.md");
    let f = std::fs::File::create(&big).unwrap();
    f.set_len(lint::MAX_FILE_BYTES + 1).unwrap();
    let cwd = std::env::current_dir().unwrap();
    let e = lint::lint(&lint::Request {
        wiki_dir: dir.path(),
        cwd: &cwd,
        rules: &[],
        today: 0,
        max_age: 30,
        cite_by_line: &[],
    })
    .unwrap_err();
    assert!(e.to_string().contains("big.md"), "{e}");
}

#[test]
fn a_missing_wiki_folder_is_an_error_naming_it() {
    let cwd = std::env::current_dir().unwrap();
    let e = lint::lint(&lint::Request {
        wiki_dir: std::path::Path::new("/no/such/wiki-folder"),
        cwd: &cwd,
        rules: &[],
        today: 0,
        max_age: 30,
        cite_by_line: &[],
    })
    .unwrap_err();
    assert!(e.to_string().contains("/no/such/wiki-folder"));
}
