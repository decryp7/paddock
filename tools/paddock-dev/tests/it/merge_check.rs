//! `./dev merge check` against real and invented merges. The first group reproduces the merge that started it: f94934ec5 kept
//! one side's four plugin-test files and dropped b41d33baf's verdict, hook and test (087570da4 restored them); the fixture
//! `tests/fixtures/merge-check/087570da4` holds the real contents of those files at the merge base (e16df7328), the first
//! parent (80f1021a8) and the second (d3ec06cac), and the real merge message. The others are invented histories.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    fn new() -> Repo {
        let dir = tempfile::tempdir().unwrap();
        let r = Repo { dir };
        r.git(&["init", "-q", "-b", "main"]);
        r
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    // Starts git and the tool under test, never a process by pid.
    #[allow(clippy::disallowed_methods)]
    fn cmd(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
        c.current_dir(self.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", empty)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid");
        c
    }

    fn git(&self, args: &[&str]) -> String {
        let out = self.cmd("git").arg("-c").arg("commit.gpgsign=false").args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(&self, file: &str, text: &str) {
        let p = self.path().join(file);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn commit(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// `(exit code, standard output)` of `dev merge check <args>` in this repository.
    fn check(&self, args: &[&str]) -> (i32, String) {
        self.check_at(self.path(), args)
    }

    /// The same for the repository at `root`.
    fn check_at(&self, root: &Path, args: &[&str]) -> (i32, String) {
        let out = self
            .cmd(env!("CARGO_BIN_EXE_paddock-dev"))
            .args(["merge", "check", "--root"])
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap(), text)
    }
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/merge-check/087570da4")
}

const REAL_FILES: [(&str, &str); 3] = [
    ("crates/debrief-plugin-test/src/host.rs", "src_host.rs.txt"),
    ("crates/debrief-plugin-test/src/lib.rs", "src_lib.rs.txt"),
    ("crates/debrief-plugin-test/tests/it/harness.rs", "tests_it_harness.rs.txt"),
];

fn write_real(r: &Repo, side: &str) {
    for (file, name) in REAL_FILES {
        r.write(file, &fs::read_to_string(fixture_dir().join(side).join(name)).unwrap());
    }
}

/// Rebuilds f94934ec5: the merge base, the first parent (it has b41d33baf), the second parent (d3ec06cac), and a merge that
/// takes the second parent's files. `message` is the merge message.
fn real_merge(message: &str) -> Repo {
    let r = Repo::new();
    write_real(&r, "base");
    r.commit("base e16df7328");
    r.git(&["checkout", "-q", "-b", "second"]);
    write_real(&r, "second");
    r.commit("fix: d3ec06cac");
    r.git(&["checkout", "-q", "main"]);
    write_real(&r, "first");
    r.commit("fix: b41d33baf");
    r.git(&["merge", "-q", "-s", "ours", "--no-commit", "second"]);
    write_real(&r, "second");
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", message]);
    r
}

fn real_message() -> String {
    fs::read_to_string(fixture_dir().join("merge-message.txt")).unwrap()
}

const HOST: &str = "crates/debrief-plugin-test/src/host.rs";
const LIB: &str = "crates/debrief-plugin-test/src/lib.rs";
const LOST: &str = "a_plugin_whose_import_breaks_while_it_reads_fails_the_cancellation_check_with_why";

#[test]
fn the_real_merge_f94934ec5_loses_hunks_and_the_message_prose_is_no_reason() {
    let r = real_merge(&real_message());
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains(&format!("[dropped-hunk] {HOST}")), "{out}");
    assert!(out.contains(&format!("[dropped-hunk] {LIB}")), "{out}");
    assert!(out.contains("::error::"), "{out}");
}

#[test]
fn the_real_merge_f94934ec5_loses_the_verdict_test() {
    let r = real_merge(&real_message());
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains(&format!("[lost-test] {LOST}")), "{out}");
}

#[test]
fn the_real_merge_f94934ec5_goes_backwards_against_the_first_parent() {
    let r = real_merge(&real_message());
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    // host.rs: the result removes what the first parent's b41d33baf-shaped commit added (rule b); lib.rs: the second parent
    // never changed it, yet the merge did (rule a).
    assert!(out.contains(&format!("[backwards] {HOST}: removes")), "{out}");
    assert!(out.contains(&format!("[backwards] {LIB}: the branch never touched it")), "{out}");
}

#[test]
fn reasons_in_the_message_answer_the_hunk_and_test_findings_only() {
    let mut message = real_message();
    for (file, _) in REAL_FILES {
        message.push_str(&format!("Hunks-dropped: {file}: the second side supersedes it\n"));
    }
    message.push_str(&format!("Tests-removed: {LOST}: superseded by the wait_read_whole check\n"));
    let r = real_merge(&message);
    let (code, out) = r.check(&["HEAD"]);
    assert!(!out.contains("[dropped-hunk]") && !out.contains("[lost-test]"), "{out}");
    // The backwards detector has no waiver: the merge still goes backwards.
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[backwards]"), "{out}");
}

#[test]
fn an_empty_reason_is_no_reason() {
    let r = real_merge(&format!("{}Tests-removed: {LOST}: \nHunks-dropped: {HOST}:\n", real_message()));
    let (_, out) = r.check(&["HEAD"]);
    assert!(out.contains(&format!("[lost-test] {LOST}")), "{out}");
    assert!(out.contains(&format!("[dropped-hunk] {HOST}")), "{out}");
}

fn lines(n: usize, p: &str) -> String {
    (1..=n).map(|i| format!("{p} line {i} of the original file\n")).collect()
}

/// An invented history: a base, a branch that changes `b.txt` and a main that changes `a.txt`; the merge keeps both.
fn clean_merge() -> Repo {
    let r = Repo::new();
    r.write("a.txt", &lines(30, "a"));
    r.write("b.txt", &lines(30, "b"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("b.txt", &format!("{}branch one\nbranch two\nbranch three\n", lines(30, "b")));
    r.commit("feat: b");
    r.git(&["checkout", "-q", "main"]);
    r.write("a.txt", &format!("main head one\nmain head two\n{}", lines(30, "a")));
    r.commit("fix: a");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    r
}

#[test]
fn a_clean_merge_is_clean() {
    let r = clean_merge();
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        !out.contains("::error::") && !out.contains("[lost-test]") && !out.contains("[dropped-hunk]"),
        "{out}"
    );
    let (code, out) = r.check(&["--branch", "feature", "--target", "HEAD^1"]);
    assert_eq!(code, 0, "{out}");
}

/// Enough other code that removing one test is not "most of what a commit added" (the backwards detector's rule b).
fn padding() -> String {
    (1..=40).map(|i| format!("pub const LIMIT_{i}: u32 = {i};\n")).collect()
}
const KEPT: &str = "#[test]\nfn keeps_working() {\n    assert_eq!(1, 1);\n}\n";
const REMOVED: &str = "\n#[test]\nfn old_case() {\n    assert_eq!(2, 2);\n}\n";

/// The branch deletes `old_case`; main has it.
fn merge_that_removes_a_test(message: &str) -> Repo {
    let r = Repo::new();
    r.write("crates/x/src/lib.rs", &format!("{}{KEPT}{REMOVED}", padding()));
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("crates/x/src/lib.rs", &format!("{}{KEPT}", padding()));
    r.commit("test: drop old_case");
    r.git(&["checkout", "-q", "main"]);
    r.write("other.txt", &format!("{}more\nmore\nmore\n", lines(10, "o")));
    r.commit("fix: other");
    r.git(&["merge", "-q", "--no-ff", "-m", message, "feature"]);
    r
}

#[test]
fn a_merge_that_removes_a_test_needs_tests_removed() {
    let (code, out) = merge_that_removes_a_test("Merge feature").check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[lost-test] old_case"), "{out}");
    assert!(!out.contains("keeps_working"), "{out}");
}

#[test]
fn tests_removed_with_a_reason_clears_the_lost_test() {
    let (code, out) = merge_that_removes_a_test("Merge feature\n\nTests-removed: old_case: covered by keeps_working\n").check(&["HEAD"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn script_tests_are_collected_too() {
    let r = Repo::new();
    r.write(
        "ui/a.spec.ts",
        "test('opens the dialog', async () => {});\nit('closes it', () => {});\n",
    );
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("ui/a.spec.ts", "test('opens the dialog', async () => {});\n");
    r.commit("test: drop one");
    r.git(&["checkout", "-q", "main"]);
    r.write("other.txt", &format!("{}more\nmore\n", lines(10, "o")));
    r.commit("fix: other");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[lost-test] closes it"), "{out}");
}

#[test]
fn a_branch_commit_built_on_a_stale_tree_is_flagged_before_and_after_merging() {
    // The script's own case (b48df1818): the branch commit has the fixed commit as its parent but a tree made from the stale one.
    let r = Repo::new();
    r.write("src/a.txt", &lines(40, "a"));
    r.write("src/b.txt", &lines(10, "b"));
    let stale = r.commit("initial");
    r.write(
        "src/a.txt",
        &format!(
            "{}fix: release the buffer\nfix: redact the token\nfix: bound the cache\n",
            lines(40, "a")
        ),
    );
    let fixed = r.commit("fix: memory and redaction");
    r.git(&["checkout", "-q", "-b", "feature", &fixed]);
    r.git(&["checkout", &stale, "--", "src/a.txt"]);
    r.write("src/new.txt", "the feature\n");
    r.commit("feat: the feature");
    r.git(&["checkout", "-q", "main"]);
    r.write("src/b.txt", &format!("{}unrelated main work\n", lines(10, "b")));
    r.commit("chore: main moves on");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[backwards] src/a.txt"), "{out}");
    assert!(
        !out.contains("[backwards] src/new.txt") && !out.contains("[backwards] src/b.txt"),
        "{out}"
    );
    let (code, out) = r.check(&["--branch", "feature", "--target", "HEAD^1"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[backwards] src/a.txt"), "{out}");
}

#[test]
fn a_merge_that_changes_a_file_the_branch_never_touched_is_flagged() {
    let r = Repo::new();
    r.write("a.txt", &lines(20, "a"));
    r.write("b.txt", &lines(20, "b"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("b.txt", &format!("{}branch line\n", lines(20, "b")));
    r.commit("feat: b");
    r.git(&["checkout", "-q", "main"]);
    r.write("a.txt", &format!("{}main line one\nmain line two\n", lines(20, "a")));
    r.commit("fix: a");
    r.git(&["merge", "-q", "--no-ff", "--no-commit", "feature"]);
    r.write("a.txt", &lines(20, "a"));
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", "Merge feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[backwards] a.txt"), "{out}");
}

#[test]
fn a_commit_that_is_not_a_merge_is_a_usage_error() {
    let r = Repo::new();
    r.write("a.txt", "x\n");
    r.commit("initial");
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 2, "{out}");
}

#[test]
fn a_clone_without_the_parents_is_not_measured_never_a_pass() {
    let r = clean_merge();
    let url = format!("file://{}", r.path().display());
    for depth in ["1", "2"] {
        let dest = tempfile::tempdir().unwrap();
        let clone = dest.path().join("c");
        r.git(&["clone", "-q", "--no-local", "--depth", depth, &url, clone.to_str().unwrap()]);
        let (code, out) = r.check_at(&clone, &["HEAD"]);
        // TODO(ADR 0089 WP-1): exit 6 once paddock_cli::Exit has it; until then a failure, exit 1.
        assert_eq!(code, 1, "depth {depth}: {out}");
        assert!(out.contains("NOT MEASURED") && out.contains("::error::"), "depth {depth}: {out}");
        assert!(!out.contains("nothing a parent added"), "depth {depth}: {out}");
    }
}

#[test]
fn a_rust_file_that_does_not_parse_is_not_measured() {
    let r = Repo::new();
    r.write("crates/x/src/lib.rs", "#[test]\nfn ok() {}\n");
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    // The test is renamed and the file is left unparsable: its tests cannot be counted.
    r.write("crates/x/src/lib.rs", "#[test]\nfn renamed( {\n");
    r.commit("feat: broken");
    r.git(&["checkout", "-q", "main"]);
    r.write("other.txt", &format!("{}more\nmore\n", lines(10, "o")));
    r.commit("fix: other");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("NOT MEASURED") && out.contains("crates/x/src/lib.rs does not parse"),
        "{out}"
    );
}

#[test]
fn an_unknown_revision_is_not_measured() {
    let r = clean_merge();
    let (code, out) = r.check(&["no-such-revision"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("NOT MEASURED"), "{out}");
}
