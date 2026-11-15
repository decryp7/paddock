//! The Rust output of cases that once ran side by side with the shell script they replaced (WP-2 of docs/plans/shell-to-rust.md,
//! plan section 5: "when the script is deleted, its side of the comparison becomes a golden file of the Rust output, and the test
//! stays"). Each file was written while both implementations ran and agreed on every case, so it holds what the script printed and did.
//!
//! One file per test, `golden/parity/<group>/<test>.txt`, holding the test's cases in the order it makes them, each after a line
//! `=== case <n> ===`. A case that is not in the file, or differs from it, fails the test with the first differing line.
//! `PADDOCK_UPDATE_GOLDEN=1` rewrites the file after a deliberate change of the output; read the diff before committing it.
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
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests/golden/parity", group, &format!("{test}.txt")]
        .iter()
        .collect();
    let block = format!("{}{}\n", marker(n), text.trim_end_matches('\n').replace("\r\n", "\n"));
    if GoldenMode::from_env() == GoldenMode::Update {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(n > 1)
            .truncate(n == 1)
            .open(&path)
            .unwrap();
        f.write_all(block.as_bytes()).unwrap();
        return;
    }
    let file = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("golden file {} cannot be read ({e}); run the test with {UPDATE_VAR}=1 and commit it", path.display()))
        .replace("\r\n", "\n");
    let want = file
        .split(&marker(n))
        .nth(1)
        .map(|rest| rest.split("=== case ").next().unwrap_or(""))
        .unwrap_or_else(|| panic!("golden file {} has no case {n}; run the test with {UPDATE_VAR}=1 to add it", path.display()));
    let want = format!("{}{}", marker(n), want);
    if want != block {
        let line = want
            .lines()
            .zip(block.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| want.lines().count().min(block.lines().count()));
        panic!(
            "case {n} differs from {} at line {}\n  file:   {:?}\n  actual: {:?}\nrun the test with {UPDATE_VAR}=1 to accept the new output",
            path.display(),
            line + 1,
            want.lines().nth(line).unwrap_or(""),
            block.lines().nth(line).unwrap_or("")
        );
    }
}
