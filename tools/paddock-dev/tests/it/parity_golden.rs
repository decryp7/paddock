//! The Rust output of cases that once ran side by side with the shell script they replaced (WP-2 of docs/plans/shell-to-rust.md,
//! plan section 5: "when the script is deleted, its side of the comparison becomes a golden file of the Rust output, and the test
//! stays"). Each file was written while both implementations ran and agreed on every case, so it holds what the script printed and did.
//!
//! One file per test, `golden/parity/<group>/<test>.txt`, holding the test's cases in the order it makes them, each after a line
//! `=== case <n> ===`. A case that is not in the file, or differs from it, fails the test with the first differing line.
//! `PADDOCK_UPDATE_GOLDEN=1` rewrites the file after a deliberate change of the output; read the diff before committing it.
// windows-twin: none: the helper of the Unix-only parity tests; it holds nothing on Windows, whose jobs run the real thing
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_testkit::golden::{GoldenMode, UPDATE_VAR};
use std::cell::Cell;
use std::io::Write;
use std::path::PathBuf;

thread_local! {
    static CASE: Cell<usize> = const { Cell::new(0) };
}

fn marker(n: usize) -> String {
    format!("=== case {n} ===\n")
}

/// Checks `text` against the calling test's next case in `group` (the test file's name).
pub(crate) fn case(group: &str, text: &str) {
    let test = std::thread::current()
        .name()
        .expect("the test runs on a thread named after it")
        .replace("::", "__");
    let n = CASE.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    if GoldenMode::from_env() == GoldenMode::Update {
        let path = golden_path(group, &test);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(n > 1)
            .truncate(n == 1)
            .open(&path)
            .unwrap();
        f.write_all(block(n, text).as_bytes()).unwrap();
        return;
    }
    if let Err(why) = check(group, &test, n, text) {
        panic!("{why}");
    }
}

fn golden_path(group: &str, test: &str) -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "tests/golden/parity", group, &format!("{test}.txt")]
        .iter()
        .collect()
}

fn block(n: usize, text: &str) -> String {
    format!("{}{}\n", marker(n), text.trim_end_matches('\n').replace("\r\n", "\n"))
}

/// Whether case `n` of `test` in `group` is `text`: `Err` says why not (no file, no such case, or the first line that differs).
fn check(group: &str, test: &str, n: usize, text: &str) -> Result<(), String> {
    let path = golden_path(group, test);
    let block = block(n, text);
    let file = std::fs::read_to_string(&path)
        .map_err(|e| {
            format!(
                "golden file {} cannot be read ({e}); run the test with {UPDATE_VAR}=1 and commit it",
                path.display()
            )
        })?
        .replace("\r\n", "\n");
    let want = file
        .split(&marker(n))
        .nth(1)
        .map(|rest| rest.split("=== case ").next().unwrap_or(""))
        .ok_or_else(|| {
            format!(
                "golden file {} has no case {n}; run the test with {UPDATE_VAR}=1 to add it",
                path.display()
            )
        })?;
    let want = format!("{}{}", marker(n), want);
    if want == block {
        return Ok(());
    }
    let line = want
        .lines()
        .zip(block.lines())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| want.lines().count().min(block.lines().count()));
    Err(format!(
        "case {n} differs from {} at line {}\n  file:   {:?}\n  actual: {:?}\nrun the test with {UPDATE_VAR}=1 to accept the new output",
        path.display(),
        line + 1,
        want.lines().nth(line).unwrap_or(""),
        block.lines().nth(line).unwrap_or("")
    ))
}

/// ER-33: a golden comparison counts only once it has been shown to see a difference. `_probe/probe.txt` holds one case, `same text`.
#[test]
fn a_changed_case_a_missing_case_and_a_missing_file_all_fail_and_the_same_text_passes() {
    assert_eq!(check("_probe", "probe", 1, "same text"), Ok(()));
    assert_eq!(
        check("_probe", "probe", 1, "same text\n"),
        Ok(()),
        "a trailing newline is not a difference"
    );
    let changed = check("_probe", "probe", 1, "other text").unwrap_err();
    assert!(changed.contains("differs") && changed.contains("at line 2"), "{changed}");
    let missing = check("_probe", "probe", 2, "same text").unwrap_err();
    assert!(missing.contains("has no case 2"), "{missing}");
    let nofile = check("_probe", "no-such-test", 1, "same text").unwrap_err();
    assert!(nofile.contains("cannot be read"), "{nofile}");
}
