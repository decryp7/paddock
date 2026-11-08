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

/// Runs `run` on `make(n)` and on `make(4 * n)`, each three times, and fails when the best time of the larger input is more
/// than 8 times the smaller's: linear work gives about 4, quadratic about 16. A ratio of the machine's own times, not a wall-time
/// limit, so a loaded machine slows both sides alike (ER-12). Both inputs are big enough to take milliseconds.
fn linear(shape: &str, n: usize, make: impl Fn(usize) -> String, run: impl Fn(&str)) {
    let best = |size: usize| {
        let input = make(size);
        (0..3)
            .map(|_| {
                let start = std::time::Instant::now();
                run(&input);
                start.elapsed()
            })
            .min()
            .unwrap()
    };
    let small = best(n).max(std::time::Duration::from_millis(1));
    let large = best(4 * n);
    assert!(
        large < small * 8,
        "{shape}: {n} -> {} units took {small:?} -> {large:?} (more than 8x for 4x the input)",
        4 * n
    );
}

#[test]
fn adversarial_shapes_stay_linear() {
    // Many unclosed brackets and backticks on one line.
    linear(
        "unclosed [ and `",
        50_000,
        |n| "[".repeat(n) + &"`".repeat(n / 200),
        |t| {
            assert!(text::links_of(t).is_empty());
            assert!(text::windows(t, 8).len() <= 1);
        },
    );
    // Many '[' and one ']' (each '[' used to look for the same ']' again).
    linear(
        "many [ one ]",
        100_000,
        |n| "[".repeat(n) + "]",
        |t| {
            assert!(text::links_of(t).is_empty());
            assert!(text::windows(t, 8).len() <= 1);
        },
    );
    // Many "[a](" with no ')' and no space: each href scan used to run to the end.
    linear(
        "[a]( without ) or space",
        125_000,
        |n| "[a](".repeat(n),
        |t| {
            assert!(text::links_of(t).is_empty());
            assert!(text::windows(t, 8).len() <= 1);
        },
    );
    // ... and with a space at the end of the run.
    linear(
        "[a]( run then a space",
        125_000,
        |n| "[a](".repeat(n) + " x",
        |t| assert!(text::links_of(t).is_empty()),
    );
    // A bullet of "](" with no ')' anywhere (32 s on 2 MB before).
    linear(
        "bullet of ](",
        250_000,
        |n| format!("- {}", "](".repeat(n)),
        |t| assert_eq!(text::bullets_without_source(t), vec![1]),
    );
    linear(
        "bullet of ](](",
        125_000,
        |n| format!("- {}", "](](".repeat(n)),
        |t| assert_eq!(text::bullets_without_source(t), vec![1]),
    );
    // Link syntax in the copy check's words.
    linear(
        "words of [x](",
        125_000,
        |n| "[x](".repeat(n),
        |t| assert!(text::words(t).len() < 600_000),
    );
    linear(
        "words of [x](y)",
        75_000,
        |n| "[x](y)".repeat(n),
        |t| assert_eq!(text::words(t).len(), 1),
    );
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
