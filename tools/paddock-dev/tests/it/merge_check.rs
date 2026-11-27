//! `./dev merge check` against real and invented merges. The first group reproduces the merge that started it: f94934ec5 kept
//! one side's four plugin-test files and dropped b41d33baf's verdict, hook and test (087570da4 restored them); the fixture
//! `tests/fixtures/merge-check/087570da4` holds the real contents of those files at the merge base (e16df7328), the first
//! parent (80f1021a8) and the second (d3ec06cac), and the real merge message. The others are invented histories.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_testkit::TestCommand;
#[cfg(unix)]
use paddock_testkit::{Actions, Case, StandIn};
use std::fs;
use std::path::{Path, PathBuf};

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
    fn cmd(&self, program: &str) -> TestCommand {
        let mut c = TestCommand::new(program);
        // Whatever this starts may be the binary under test: never a compile-cache server on this machine (ER-45).
        crate::dev_bin::harden(&mut c);
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

    /// The tool under test, started like git above.
    fn dev_cmd(&self) -> TestCommand {
        self.cmd(crate::dev_bin::PATH)
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
            .dev_cmd()
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

const REAL_FILES: [(&str, &str); 4] = [
    ("crates/debrief-plugin-test/src/conformance.rs", "src_conformance.rs.txt"),
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
const CONFORMANCE: &str = "crates/debrief-plugin-test/src/conformance.rs";
const LOST: &str = "a_plugin_whose_import_breaks_while_it_reads_fails_the_cancellation_check_with_why";

#[test]
fn the_real_merge_f94934ec5_loses_hunks_and_the_message_prose_is_no_reason() {
    let r = real_merge(&real_message());
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains(&format!("[dropped-hunk] {HOST}")), "{out}");
    assert!(out.contains(&format!("[dropped-hunk] {LIB}")), "{out}");
    assert!(out.contains(&format!("[dropped-hunk] {CONFORMANCE}")), "{out}");
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
    // lib.rs: the second parent never changed it, yet the merge did (rule a). host.rs is no revert in the sense of rule b: the
    // result's blob is the second parent's own, not a blob the first parent's history had before.
    assert!(out.contains(&format!("[backwards] {LIB}: the branch never touched it")), "{out}");
}

#[test]
fn reasons_in_the_message_answer_each_finding_they_name() {
    let mut message = real_message();
    for (file, _) in REAL_FILES {
        message.push_str(&format!("Hunks-dropped: {file}: the second side supersedes it\n"));
    }
    // The hunk reason answers the hunk, whole-file and backwards findings of those files; the test needs its own.
    let (code, out) = real_merge(&message).check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(
        !out.contains("[dropped-hunk]") && !out.contains("[backwards]") && !out.contains("[whole-file]"),
        "{out}"
    );
    assert!(out.contains(&format!("[lost-test] {LOST}")), "{out}");
    message.push_str(&format!("Tests-removed: {LOST}: superseded by the wait_read_whole check\n"));
    let (code, out) = real_merge(&message).check(&["HEAD"]);
    assert_eq!(code, 0, "{out}");
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

/// The branch deletes `old_case`; main still has it (the first parent has the test, the base has it, the other side deleted it).
fn merge_where_the_branch_deletes_a_test(message: &str) -> Repo {
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

const ADDED: &str = "\n#[test]\nfn new_case() {\n    assert_eq!(3, 3);\n}\n";

/// Main adds `new_case`; the branch changes the same file elsewhere; the merge result keeps the branch's change and drops
/// `new_case` (nothing in the base or the branch had it).
fn merge_that_drops_a_test(message: &str) -> Repo {
    let r = Repo::new();
    r.write("crates/x/src/lib.rs", &format!("{}{KEPT}", padding()));
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("crates/x/src/lib.rs", &format!("pub const BRANCH: u32 = 1;\n{}{KEPT}", padding()));
    r.commit("feat: branch change");
    r.git(&["checkout", "-q", "main"]);
    r.write("crates/x/src/lib.rs", &format!("{}{KEPT}{ADDED}", padding()));
    r.commit("test: new_case");
    r.git(&["merge", "-q", "-s", "ours", "--no-commit", "feature"]);
    r.write("crates/x/src/lib.rs", &format!("pub const BRANCH: u32 = 1;\n{}{KEPT}", padding()));
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", message]);
    r
}

#[test]
fn a_test_the_other_side_deleted_is_not_lost() {
    // Present in the base and in this parent, deleted on the other side on purpose: a three-way merge keeps it deleted.
    let (code, out) = merge_where_the_branch_deletes_a_test("Merge feature").check(&["HEAD"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("[lost-test]"), "{out}");
}

#[test]
fn a_test_nothing_but_this_parent_had_needs_tests_removed() {
    let (code, out) = merge_that_drops_a_test("Merge feature").check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[lost-test] new_case"), "{out}");
    assert!(!out.contains("keeps_working"), "{out}");
}

#[test]
fn tests_removed_with_a_reason_clears_the_lost_test() {
    for subject in ["new_case", "crates/x/src/lib.rs::new_case"] {
        let message =
            format!("Merge feature\n\nTests-removed: {subject}: covered by keeps_working\nHunks-dropped: crates/x/src/lib.rs: same\n");
        let (code, out) = merge_that_drops_a_test(&message).check(&["HEAD"]);
        assert_eq!(code, 0, "{subject}: {out}");
    }
}

#[test]
fn script_tests_are_collected_too() {
    let r = Repo::new();
    r.write("ui/a.spec.ts", "test('opens the dialog', async () => {});\n");
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("ui/a.spec.ts", "// a note\ntest('opens the dialog', async () => {});\n");
    r.commit("test: note");
    r.git(&["checkout", "-q", "main"]);
    r.write(
        "ui/a.spec.ts",
        "test('opens the dialog', async () => {});\nit('closes it', () => {});\n",
    );
    r.commit("test: close");
    r.git(&["merge", "-q", "-s", "ours", "--no-commit", "feature"]);
    r.write("ui/a.spec.ts", "// a note\ntest('opens the dialog', async () => {});\n");
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", "Merge feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[lost-test] closes it"), "{out}");
}

#[test]
fn a_test_that_moved_with_its_renamed_file_is_not_lost() {
    // The branch renames the file and edits a line in it; the first parent's file is read at its old path and the result's at
    // the new one.
    let r = Repo::new();
    r.write("crates/x/src/a.rs", &format!("{}{KEPT}{REMOVED}", padding()));
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.git(&["mv", "crates/x/src/a.rs", "crates/x/src/b.rs"]);
    r.write(
        "crates/x/src/b.rs",
        &format!(
            "{}{}{REMOVED}",
            padding(),
            KEPT.replace("fn keeps_working() {", "fn keeps_working()  {")
        ),
    );
    r.commit("refactor: rename and touch");
    r.git(&["checkout", "-q", "main"]);
    r.write("other.txt", &format!("{}more\nmore\nmore\n", lines(10, "o")));
    r.commit("fix: other");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 0, "{out}");
}

fn stale_tree_merge(message: &str) -> Repo {
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
    r.git(&["merge", "-q", "--no-ff", "-m", message, "feature"]);
    r
}

#[test]
fn a_branch_commit_built_on_a_stale_tree_is_flagged_before_and_after_merging() {
    let r = stale_tree_merge("Merge feature");
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
fn a_backwards_finding_can_be_waived_with_a_reason() {
    for key in ["Reverted", "Hunks-dropped"] {
        let r = stale_tree_merge(&format!("Merge feature\n\n{key}: src/a.txt: the fix moved to another module\n"));
        let (code, out) = r.check(&["HEAD"]);
        assert_eq!(code, 0, "{key}: {out}");
    }
    let r = stale_tree_merge("Merge feature\n\nReverted: src/a.txt: \n");
    assert_eq!(r.check(&["HEAD"]).0, 5, "an empty reason is no reason");
}

#[test]
fn a_branch_that_renames_deletes_or_rewrites_files_is_not_backwards() {
    let r = Repo::new();
    r.write("keep.txt", &lines(30, "k"));
    r.write("moved.txt", &lines(30, "m"));
    r.write("gone.txt", &lines(30, "g"));
    r.write("rewritten.txt", &lines(30, "w"));
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.git(&["mv", "moved.txt", "renamed.txt"]);
    r.git(&["rm", "-q", "gone.txt"]);
    r.write("rewritten.txt", &lines(25, "new"));
    r.commit("refactor: move, delete, rewrite");
    r.git(&["checkout", "-q", "main"]);
    r.write("other.txt", &format!("{}more\nmore\nmore\n", lines(10, "o")));
    r.commit("fix: other");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 0, "{out}");
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
    let url = format!("file://{}", paddock_testkit::path::text(r.path()));
    for depth in ["1", "2"] {
        let dest = tempfile::tempdir().unwrap();
        let clone = dest.path().join("c");
        r.git(&["clone", "-q", "--no-local", "--depth", depth, &url, clone.to_str().unwrap()]);
        let (code, out) = r.check_at(&clone, &["HEAD"]);
        assert_eq!(code, 6, "depth {depth}: {out}");
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
    // Main touches the file too, so that both sides' versions of it matter and it is read.
    r.write("crates/x/src/lib.rs", "// a main note\n#[test]\nfn ok() {}\n");
    r.commit("fix: main note");
    // The result is not git's own merge (it drops main's note), so the file is read.
    r.git(&["merge", "-q", "-s", "ours", "--no-commit", "feature"]);
    r.write("crates/x/src/lib.rs", "#[test]\nfn renamed( {\n");
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", "Merge feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 6, "{out}");
    assert!(
        out.contains("NOT MEASURED") && out.contains("crates/x/src/lib.rs does not parse"),
        "{out}"
    );
}

#[test]
fn an_unknown_revision_is_not_measured() {
    let r = clean_merge();
    let (code, out) = r.check(&["no-such-revision"]);
    assert_eq!(code, 6, "{out}");
    assert!(out.contains("NOT MEASURED"), "{out}");
}

/// A repository where main adds `added` to `file` after the base, the branch edits another line of it, and the merge result is
/// `result` (a text that keeps the branch's edit). `others` are files written in the base, so both parents have them.
fn evil_file_merge(file: &str, base: &str, added_by_main: &str, branch: &str, result: &str, message: &str) -> Repo {
    let r = Repo::new();
    r.write(file, base);
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write(file, branch);
    r.commit("feat: branch edit");
    r.git(&["checkout", "-q", "main"]);
    r.write(file, added_by_main);
    r.write("other.txt", &format!("{}main other\n", lines(10, "o")));
    r.commit("fix: main adds");
    r.git(&["merge", "-q", "-s", "ours", "--no-commit", "feature"]);
    r.write(file, result);
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", message]);
    r
}

fn five() -> &'static str {
    "added one here\nadded two here\nadded three here\nadded four here\nadded five here\n"
}

fn hunk_merge(kept_of_five: usize, message: &str) -> Repo {
    let base = lines(30, "a");
    let main = format!("{}{}", five(), base);
    let branch = format!("{base}branch tail one\n");
    let kept: String = five().lines().take(kept_of_five).map(|l| format!("{l}\n")).collect();
    evil_file_merge("f.txt", &base, &main, &branch, &format!("{kept}{base}branch tail one\n"), message)
}

#[test]
fn a_hunk_with_most_of_its_lines_kept_is_not_dropped_but_half_is() {
    // 5 lines, 2 missing: 40 percent, below half.
    let (_, out) = hunk_merge(3, "Merge").check(&["HEAD"]);
    assert!(!out.contains("[dropped-hunk]"), "{out}");
    // 5 lines, 3 missing: more than half; and 6 lines of which 3 are missing is exactly half.
    let (_, out) = hunk_merge(2, "Merge").check(&["HEAD"]);
    assert!(out.contains("[dropped-hunk] f.txt"), "{out}");
    // A hunk of two lines of which one is missing is half, but one line is not enough to call a drop.
    let base = lines(30, "a");
    let main = format!("added one here\nadded two here\n{base}");
    let branch = format!("{base}branch tail one\n");
    let r = evil_file_merge(
        "f.txt",
        &base,
        &main,
        &branch,
        &format!("added one here\n{base}branch tail one\n"),
        "Merge",
    );
    let (_, out) = r.check(&["HEAD"]);
    assert!(!out.contains("[dropped-hunk]"), "one missing line of two: {out}");
    let base = lines(30, "a");
    let six = format!("{}six six six six\n", five());
    let main = format!("{six}{base}");
    let branch = format!("{base}branch tail one\n");
    let kept: String = six.lines().take(3).map(|l| format!("{l}\n")).collect();
    let r = evil_file_merge("f.txt", &base, &main, &branch, &format!("{kept}{base}branch tail one\n"), "Merge");
    let (_, out) = r.check(&["HEAD"]);
    assert!(out.contains("[dropped-hunk] f.txt"), "half of six: {out}");
}

#[test]
fn a_hunk_moved_inside_the_file_is_not_dropped() {
    let base = lines(30, "a");
    let main = format!("{}{}", five(), base);
    let branch = format!("{base}branch tail one\n");
    // The result has the five lines at the end instead of the top: removed in one place, added in another.
    let r = evil_file_merge(
        "f.txt",
        &base,
        &main,
        &branch,
        &format!("{base}{}branch tail one\n", five()),
        "Merge",
    );
    let (_, out) = r.check(&["HEAD"]);
    assert!(!out.contains("[dropped-hunk]"), "{out}");
}

#[test]
fn a_folder_reason_needs_its_trailing_slash() {
    let base = lines(30, "a");
    let main = format!("{}{}", five(), base);
    let branch = format!("{base}branch tail one\n");
    let result = format!("{base}branch tail one\n");
    let make = |trailer: &str| evil_file_merge("docs/img/f.txt", &base, &main, &branch, &result, &format!("Merge\n\n{trailer}\n"));
    let (_, out) = make("Hunks-dropped: docs/img/: the images are redrawn").check(&["HEAD"]);
    assert!(!out.contains("[dropped-hunk]"), "{out}");
    let (_, out) = make("Hunks-dropped: docs/img: the images are redrawn").check(&["HEAD"]);
    assert!(out.contains("[dropped-hunk] docs/img/f.txt"), "{out}");
}

#[test]
fn a_file_the_other_side_deleted_is_a_conflict_not_a_drop() {
    let r = Repo::new();
    r.write("f.txt", &lines(30, "a"));
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.git(&["rm", "-q", "f.txt"]);
    r.commit("refactor: delete f");
    r.git(&["checkout", "-q", "main"]);
    r.write("f.txt", &format!("{}{}", five(), lines(30, "a")));
    r.commit("fix: main adds");
    r.git(&["merge", "-q", "-s", "ours", "--no-commit", "feature"]);
    r.git(&["rm", "-q", "f.txt"]);
    r.git(&["commit", "-q", "-m", "Merge feature"]);
    let (_, out) = r.check(&["HEAD"]);
    assert!(!out.contains("[dropped-hunk]"), "{out}");
}

// Hunks main added to regenerated files are not "dropped" when the merge regenerates them. One test per kind of generated file:
// the five in one test took 45 to 49 s on the Windows runner under the suite's load (ci.yml 8271, windows-host-probe 8277 and 8280),
// next to the 60 s slow-timeout; each case alone is a fifth of it.
#[test]
fn generated_files_are_skipped_by_every_detector_a_lockfile() {
    generated_file_is_skipped("Cargo.lock");
}

#[test]
fn generated_files_are_skipped_by_every_detector_notices() {
    generated_file_is_skipped("THIRD_PARTY_NOTICES.md");
}

#[test]
fn generated_files_are_skipped_by_every_detector_a_drawio_file() {
    generated_file_is_skipped("docs/x.drawio");
}

#[test]
fn generated_files_are_skipped_by_every_detector_a_generated_folder() {
    generated_file_is_skipped("crates/x/widgets/generated.rs");
}

#[test]
fn generated_files_are_skipped_by_every_detector_a_linguist_generated_file() {
    generated_file_is_skipped("gen/out.gen.txt");
}

fn generated_file_is_skipped(file: &str) {
    let base = lines(30, "a");
    let main = format!("{}{}", five(), base);
    let branch = format!("{base}branch tail one\n");
    let r = Repo::new();
    r.write(".gitattributes", "*.gen.txt linguist-generated\n");
    r.write(file, &base);
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write(file, &branch);
    r.commit("feat: branch edit");
    r.git(&["checkout", "-q", "main"]);
    r.write(file, &main);
    r.commit("fix: main adds");
    r.git(&["merge", "-q", "-s", "ours", "--no-commit", "feature"]);
    r.write(file, &branch);
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", "Merge feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 0, "{file}: {out}");
}

#[cfg(unix)]
#[test]
fn a_deleted_file_whose_path_git_quotes_is_not_measured() {
    let r = Repo::new();
    r.write("we\"ird.txt", &lines(30, "q"));
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.git(&["rm", "-q", "we\"ird.txt"]);
    r.commit("refactor: delete");
    r.git(&["checkout", "-q", "main"]);
    r.write("other.txt", &format!("{}more\nmore\nmore\n", lines(10, "o")));
    r.commit("fix: other");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 6, "{out}");
    assert!(out.contains("NOT MEASURED"), "{out}");
}

#[test]
fn json_mode_does_not_call_findings_ok() {
    let r = stale_tree_merge("Merge feature");
    let (code, out) = r.check(&["HEAD", "--json"]);
    assert_eq!(code, 5, "{out}");
    let line = out.lines().find(|l| l.starts_with('{')).unwrap_or("");
    assert!(line.contains("\"ok\":false") && line.contains("\"kind\":\"findings\""), "{out}");
}

/// Both sides edit `file`; the merge result is the branch's version whole.
fn take_branch_whole(file: &str, message: &str, main_edits: bool) -> Repo {
    let base = lines(30, "a");
    let branch = format!("{base}branch tail one\nbranch tail two\n");
    let main = if main_edits {
        format!("main head one\nmain head two\n{base}")
    } else {
        base.clone()
    };
    evil_file_merge(file, &base, &main, &branch, &branch, message)
}

#[test]
fn a_file_both_sides_changed_and_the_merge_took_whole_is_flagged() {
    let (code, out) = take_branch_whole("notes/ecosystem.md", "Merge", true).check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[whole-file] notes/ecosystem.md"), "{out}");
}

#[test]
fn a_file_only_one_side_changed_is_no_whole_file_take() {
    let (code, out) = take_branch_whole("notes/ecosystem.md", "Merge", false).check(&["HEAD"]);
    assert!(!out.contains("[whole-file]"), "{out}");
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_whole_file_take_is_answered_by_a_reason_and_images_follow_their_generator() {
    let m = "Merge\n\nHunks-dropped: notes/ecosystem.md: the page is redrawn\n";
    let (code, out) = take_branch_whole("notes/ecosystem.md", m, true).check(&["HEAD"]);
    assert_eq!(code, 0, "{out}");
    let (_, out) = take_branch_whole("docs/images/ecosystem.png", "Merge", true).check(&["HEAD"]);
    assert!(!out.contains("[whole-file]"), "{out}");
}

#[test]
fn a_file_the_branch_renamed_and_main_edited_merges_cleanly() {
    // Renames are followed: main's hunk is in the renamed file, not "removed with the old name".
    let r = Repo::new();
    let base = lines(30, "a");
    r.write("f.txt", &base);
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.git(&["mv", "f.txt", "g.txt"]);
    r.write("g.txt", &format!("{base}branch tail one\n"));
    r.commit("refactor: rename f");
    r.git(&["checkout", "-q", "main"]);
    r.write("f.txt", &format!("{}{base}", five()));
    r.commit("fix: main edits f");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    assert!(r.path().join("g.txt").exists());
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_shallow_clone_is_not_measured_even_when_every_object_is_there() {
    let r = clean_merge();
    let root = r.git(&["rev-list", "--max-parents=0", "HEAD"]);
    fs::write(r.path().join(".git").join("shallow"), format!("{root}\n")).unwrap();
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 6, "{out}");
    assert!(out.contains("NOT MEASURED") && out.contains("shallow"), "{out}");
}

#[test]
fn a_change_the_result_already_has_is_no_whole_file_take() {
    // The branch fixes line 5; main cherry-picks that fix and edits line 15; git merges cleanly to main's file.
    let r = Repo::new();
    let base = lines(30, "a");
    let fixed = base.replace("a line 5 of the original file", "a line 5 fixed by the branch");
    let main_edit = fixed.replace("a line 15 of the original file", "a line 15 edited on main");
    r.write("f.txt", &base);
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("f.txt", &fixed);
    r.commit("fix: line 5");
    r.git(&["checkout", "-q", "main"]);
    r.write("f.txt", &fixed);
    r.commit("fix: line 5 (cherry-pick)");
    r.write("f.txt", &main_edit);
    r.commit("fix: line 15");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert!(!out.contains("[whole-file]"), "{out}");
    assert_eq!(code, 0, "{out}");
}

#[test]
fn edits_a_branch_made_while_renaming_a_file_cannot_be_dropped_quietly() {
    let r = Repo::new();
    let base = lines(30, "x");
    r.write("x.txt", &base);
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.git(&["mv", "x.txt", "y.txt"]);
    r.write(
        "y.txt",
        &format!("{base}renamed and edited one\nrenamed and edited two\nrenamed and edited three\n"),
    );
    r.commit("refactor: rename x and edit it");
    r.git(&["checkout", "-q", "main"]);
    r.write("other.txt", &format!("{}more\nmore\nmore\n", lines(10, "o")));
    r.commit("fix: other");
    r.git(&["merge", "-q", "-s", "ours", "--no-commit", "feature"]);
    r.git(&["rm", "-q", "x.txt"]);
    r.write("y.txt", &base);
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", "Merge feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[dropped-hunk] y.txt"), "{out}");
}

#[test]
fn a_revert_of_few_lines_of_a_big_commit_is_no_stale_tree() {
    // c2 adds ten lines, c3 removes eight of them; a stale-tree branch brings back the file as it was before c2, so the result
    // removes the two that are left: 2 of the 10 lines c2 added, below half.
    let r = Repo::new();
    let base = lines(30, "a");
    let ten: Vec<String> = (1..=10).map(|i| format!("added line number {i} of the big commit\n")).collect();
    r.write("a.txt", &base);
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.write("a.txt", &format!("{base}{}", ten.concat()));
    r.commit("feat: ten lines");
    r.write("a.txt", &format!("{base}{}", ten[8..].concat()));
    let c3 = r.commit("refactor: remove eight");
    r.git(&["checkout", "-q", "-b", "feature", &c3]);
    r.write("a.txt", &base);
    r.commit("feat: stale tree");
    r.git(&["checkout", "-q", "main"]);
    r.write("other.txt", &format!("{}more\nmore\nmore\n", lines(10, "o")));
    r.commit("fix: other");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    let (_, out) = r.check(&["HEAD"]);
    assert!(!out.contains("[backwards] a.txt"), "{out}");
}

#[test]
fn a_test_moved_into_a_new_file_while_the_old_one_stays_is_not_lost() {
    let r = Repo::new();
    r.write("crates/x/src/tests.rs", &format!("{}{KEPT}{REMOVED}", padding()));
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("crates/x/src/tests.rs", &format!("{}{KEPT}", padding()));
    r.write(
        "crates/x/src/moved.rs",
        &format!("{}{}", padding().replace("LIMIT", "MOVED"), REMOVED),
    );
    r.commit("refactor: move old_case to its own file");
    r.git(&["checkout", "-q", "main"]);
    r.write("crates/x/src/tests.rs", &format!("// a main note\n{}{KEPT}{REMOVED}", padding()));
    r.commit("fix: main note");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    let (_, out) = r.check(&["HEAD"]);
    assert!(!out.contains("[lost-test]"), "{out}");
}

#[test]
fn a_moved_script_test_is_found_by_its_escaped_name() {
    let r = Repo::new();
    r.write(
        "ui/a.spec.ts",
        "it('keeps cargo\\'s default', () => {});\ntest('stays', () => {});\n",
    );
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("ui/a.spec.ts", "test('stays', () => {});\n");
    r.write("ui/b.spec.ts", "it('keeps cargo\\'s default', () => {});\n");
    r.commit("test: move");
    r.git(&["checkout", "-q", "main"]);
    r.write(
        "ui/a.spec.ts",
        "it('keeps cargo\\'s default', () => {});\ntest('stays', () => {});\n// main note\n",
    );
    r.commit("test: main note");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    let (_, out) = r.check(&["HEAD"]);
    assert!(!out.contains("[lost-test]") && !out.contains("NOT MEASURED"), "{out}");
}

#[test]
fn a_file_found_only_by_the_moved_test_search_that_does_not_parse_is_skipped_not_fatal() {
    // A fixture that mentions `fn new_case` but is not valid Rust must not make the whole merge not measured.
    let r = Repo::new();
    r.write("crates/x/src/lib.rs", &format!("{}{KEPT}", padding()));
    r.write("crates/x/tests/fixtures/bad.rs", "fn new_case( {\n");
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("crates/x/src/lib.rs", &format!("pub const BRANCH: u32 = 1;\n{}{KEPT}", padding()));
    r.commit("feat: branch change");
    r.git(&["checkout", "-q", "main"]);
    r.write("crates/x/src/lib.rs", &format!("{}{KEPT}{ADDED}", padding()));
    r.commit("test: new_case");
    r.git(&["merge", "-q", "-s", "ours", "--no-commit", "feature"]);
    r.write("crates/x/src/lib.rs", &format!("pub const BRANCH: u32 = 1;\n{}{KEPT}", padding()));
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", "Merge feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[lost-test] new_case") && !out.contains("NOT MEASURED"), "{out}");
}

#[test]
fn findings_print_one_line_each_and_json_keeps_them_structured() {
    let r = stale_tree_merge("Merge feature");
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].starts_with("error: merge check: "), "{out}");
    assert!(
        lines
            .iter()
            .filter(|l| l.contains("[backwards]"))
            .all(|l| l.starts_with("  [backwards] ")),
        "{out}"
    );
    let (code, out) = r.check(&["HEAD", "--json"]);
    assert_eq!(code, 5, "{out}");
    let json = out.lines().find(|l| l.starts_with('{')).unwrap();
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["kind"], "findings");
    assert_eq!(v["findings"][0]["kind"], "backwards");
    assert_eq!(v["findings"][0]["subject"], "src/a.txt");
    assert!(v["findings"][0]["detail"].as_str().is_some());
}

#[test]
fn a_test_the_merge_moved_to_another_file_is_not_lost() {
    // Main adds `new_case`; the result keeps it, in a file of its own: found by name, not lost.
    let r = Repo::new();
    r.write("crates/x/src/lib.rs", &format!("{}{KEPT}", padding()));
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("crates/x/src/lib.rs", &format!("pub const BRANCH: u32 = 1;\n{}{KEPT}", padding()));
    r.commit("feat: branch change");
    r.git(&["checkout", "-q", "main"]);
    r.write("crates/x/src/lib.rs", &format!("{}{KEPT}{ADDED}", padding()));
    r.commit("test: new_case");
    r.git(&["merge", "-q", "-s", "ours", "--no-commit", "feature"]);
    r.write("crates/x/src/lib.rs", &format!("pub const BRANCH: u32 = 1;\n{}{KEPT}", padding()));
    r.write("crates/x/src/more.rs", ADDED);
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", "Merge feature"]);
    let (_, out) = r.check(&["HEAD"]);
    assert!(!out.contains("[lost-test]"), "{out}");
}

impl Repo {
    /// Runs git and returns whether it succeeded (a conflicting merge fails on purpose).
    fn git_may_fail(&self, args: &[&str]) -> bool {
        self.cmd("git")
            .arg("-c")
            .arg("commit.gpgsign=false")
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    }
}

#[test]
fn a_binary_conflict_resolved_with_ours_is_not_skipped_as_cleanly_merged() {
    // Git cannot merge a binary file changed on both sides; its automatic result is parent 1's blob, which is also what
    // `--ours` gives. That equality is no evidence of a clean merge.
    let r = Repo::new();
    let bin = |tag: &str| -> Vec<u8> { [b"head\0".as_slice(), tag.as_bytes(), b"\0tail\n"].concat() };
    fs::write(r.path().join("bin.dat"), bin("base")).unwrap();
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    fs::write(r.path().join("bin.dat"), bin("branch")).unwrap();
    r.commit("feat: branch changes it");
    r.git(&["checkout", "-q", "main"]);
    fs::write(r.path().join("bin.dat"), bin("main")).unwrap();
    r.commit("fix: main changes it");
    assert!(
        !r.git_may_fail(&["merge", "--no-commit", "--no-ff", "feature"]),
        "the merge must conflict"
    );
    r.git(&["checkout", "--ours", "bin.dat"]);
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", "Merge feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[whole-file] bin.dat"), "{out}");
}

#[test]
fn a_hunk_whose_lines_exist_elsewhere_in_a_cleanly_merged_file_is_not_dropped() {
    // Main adds two boilerplate lines at the top; the branch deletes the same two lines at the bottom (they were there in the
    // base). Git merges cleanly: the lines are at the top. The old file's removed lines look like the hunk's lines.
    let r = Repo::new();
    let boiler = "beta boilerplate line one\ngamma boilerplate line two\n";
    let base = format!("{}{boiler}", lines(30, "a"));
    r.write("f.txt", &base);
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("f.txt", &lines(30, "a"));
    r.commit("refactor: drop the boilerplate at the bottom");
    r.git(&["checkout", "-q", "main"]);
    r.write("f.txt", &format!("{boiler}{base}"));
    r.commit("feat: boilerplate at the top");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert!(!out.contains("[dropped-hunk]"), "{out}");
    assert_eq!(code, 0, "{out}");
}

#[test]
fn an_octopus_merge_is_not_measured() {
    let r = Repo::new();
    r.write("a.txt", &lines(10, "a"));
    r.write("b.txt", &lines(10, "b"));
    r.write("c.txt", &lines(10, "c"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "one"]);
    r.write("a.txt", &format!("{}one\n", lines(10, "a")));
    r.commit("feat: one");
    r.git(&["checkout", "-q", "main"]);
    r.git(&["checkout", "-q", "-b", "two"]);
    r.write("b.txt", &format!("{}two\n", lines(10, "b")));
    r.commit("feat: two");
    r.git(&["checkout", "-q", "main"]);
    r.write("c.txt", &format!("{}main\n", lines(10, "c")));
    r.commit("fix: main");
    r.git(&["merge", "-q", "-m", "Merge one and two", "one", "two"]);
    assert_eq!(r.git(&["rev-list", "--parents", "-n", "1", "HEAD"]).split(' ').count(), 4);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 6, "{out}");
    assert!(
        out.contains("NOT MEASURED") && out.contains("octopus merges are not supported"),
        "{out}"
    );
}

#[test]
fn one_side_adding_a_test_and_the_other_deleting_another_is_judged_per_test() {
    // The result takes the branch's file whole: main's new test is lost, the test the branch deleted is not.
    let r = Repo::new();
    r.write("crates/x/src/lib.rs", &format!("{}{KEPT}{REMOVED}", padding()));
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("crates/x/src/lib.rs", &format!("{}{KEPT}", padding()));
    r.commit("test: drop old_case");
    r.git(&["checkout", "-q", "main"]);
    r.write("crates/x/src/lib.rs", &format!("{}{KEPT}{REMOVED}{ADDED}", padding()));
    r.commit("test: add new_case");
    r.git(&["merge", "-q", "-s", "ours", "--no-commit", "feature"]);
    r.write("crates/x/src/lib.rs", &format!("{}{KEPT}", padding()));
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", "Merge feature"]);
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[lost-test] new_case"), "{out}");
    assert!(!out.contains("[lost-test] old_case"), "{out}");
}

/// Both sides change `file`, which `.gitattributes` makes unmergeable (`merge=binary`), and the resolution is `--ours`: git's
/// automatic blob for it is parent 1's, equal to the result, yet the file is listed as conflicted.
fn conflicted_taken_as_ours(file: &str, base: &str, main: &str, branch: &str) -> Repo {
    let r = Repo::new();
    r.write(".gitattributes", "*.rs merge=binary\n*.txt merge=binary\n");
    r.write(file, base);
    r.write("other.txt", &lines(10, "o"));
    r.commit("initial");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write(file, branch);
    r.commit("feat: branch");
    r.git(&["checkout", "-q", "main"]);
    r.write(file, main);
    r.commit("fix: main");
    assert!(
        !r.git_may_fail(&["merge", "--no-commit", "--no-ff", "feature"]),
        "the merge must conflict"
    );
    r.git(&["checkout", "--ours", file]);
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", "Merge feature"]);
    r
}

#[test]
fn a_conflicted_file_taken_as_ours_still_loses_the_other_sides_hunks() {
    let base = lines(30, "a");
    let r = conflicted_taken_as_ours(
        "notes.txt",
        &base,
        &format!("main head one\nmain head two\n{base}"),
        &format!("{base}branch tail one\nbranch tail two\nbranch tail three\n"),
    );
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[dropped-hunk] notes.txt"), "{out}");
}

#[test]
fn a_conflicted_file_taken_as_ours_still_loses_the_other_sides_tests() {
    let base = format!("{}{KEPT}", padding());
    let r = conflicted_taken_as_ours(
        "crates/x/src/lib.rs",
        &base,
        &format!("// main note\n{base}"),
        &format!("{base}{ADDED}"),
    );
    let (code, out) = r.check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    assert!(out.contains("[lost-test] new_case"), "{out}");
}

#[test]
fn a_dropped_hunk_finding_shows_where_it_was_and_what_it_said() {
    // Main adds five lines at the top of f.txt; the result keeps only the first (4 of 5 missing).
    let (code, out) = hunk_merge(1, "Merge").check(&["HEAD"]);
    assert_eq!(code, 5, "{out}");
    let finding = out.lines().position(|l| l.starts_with("  [dropped-hunk] f.txt")).expect(&out);
    let shown: Vec<&str> = out.lines().skip(finding + 1).take(3).collect();
    // The parent-side line range of the hunk and the first missing lines, under the finding.
    assert!(
        shown[0].contains("parent 1 lines 1-5") && shown[0].contains("added two here"),
        "{out}"
    );
    let (code, out) = hunk_merge(1, "Merge").check(&["HEAD", "--json"]);
    assert_eq!(code, 5, "{out}");
    let json = out.lines().find(|l| l.starts_with('{')).unwrap();
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    let f = v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "dropped-hunk")
        .unwrap();
    let ex = f["excerpts"].as_array().unwrap();
    assert!(
        ex.len() == 1 && ex[0].as_str().unwrap().contains("lines 1-5") && ex[0].as_str().unwrap().contains("added two here"),
        "{out}"
    );
}

// The workspace-hack detector: a stand-in for `cargo` plays `cargo hakari generate --diff` (exit 1 and a diff when a member
// names base64 and the hack does not). The real tool is exercised by the unit tests of `hakari.rs` and by the CI line.
#[cfg(unix)]
mod hakari {
    use super::*;

    /// Plays `cargo hakari generate --diff`, run in `<tree>/crates`. Like cargo-hakari 0.9.39, it prints the diff on STANDARD ERROR.
    fn fake_cargo(dir: &Path) -> PathBuf {
        let hakari = ["hakari", "generate", "--diff"];
        let stale = Case::new()
            .args_start_with(&hakari)
            .file_contains(Path::new("x/Cargo.toml"), "base64")
            .file_lacks(Path::new("paddock-workspace-hack/Cargo.toml"), "base64");
        StandIn::new(dir, "cargo")
            .answers(Actions::new().stderr_with("unexpected: {args}").exit(2))
            .when(&Case::new().args_start_with(&hakari), Actions::new())
            .when(
                &stale,
                Actions::new()
                    .stderr("info: ")
                    .stderr("--- original")
                    .stderr("+++ modified")
                    .stderr("+base64 = { version = \"0.23\" }")
                    .exit(1),
            )
            .install()
            .unwrap()
            .program()
            .to_path_buf()
    }

    /// A merge of two clean branches: one adds a member that needs base64, the other touches an unrelated file; `fixed` is
    /// the one that regenerates the hack (as the train's own fix would).
    fn train(with_config: bool, fixed: bool) -> Repo {
        let r = Repo::new();
        if with_config {
            r.write("crates/.config/hakari.toml", "hakari-package = \"paddock-workspace-hack\"\n");
        }
        r.write("crates/paddock-workspace-hack/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        r.write("crates/x/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        r.write("notes.txt", "one\n");
        r.commit("base");
        r.git(&["checkout", "-q", "-b", "other"]);
        r.write("notes.txt", "one\ntwo\n");
        if fixed {
            r.write(
                "crates/paddock-workspace-hack/Cargo.toml",
                "[dependencies]\nserde = \"1\"\nbase64 = \"0.23\"\n",
            );
        }
        r.commit("docs: notes");
        r.git(&["checkout", "-q", "main"]);
        r.write("crates/x/Cargo.toml", "[dependencies]\nserde = \"1\"\nbase64 = \"0.23\"\n");
        r.commit("feat: x needs base64");
        r.git(&["merge", "-q", "--no-ff", "-m", "Merge other", "other"]);
        r
    }

    /// The merge of `evil_file_merge` (main's five added lines are dropped from `f.txt`), in a tree that also has a stale
    /// workspace-hack or, with `broken`, whose hakari cannot run.
    fn dropped_hunk_and_hakari() -> Repo {
        let r = Repo::new();
        let base = lines(30, "a");
        r.write("crates/.config/hakari.toml", "hakari-package = \"paddock-workspace-hack\"\n");
        r.write("crates/paddock-workspace-hack/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        r.write("crates/x/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
        r.write("f.txt", &base);
        r.commit("initial");
        r.git(&["checkout", "-q", "-b", "feature"]);
        r.write("f.txt", &format!("{base}branch tail one\n"));
        r.commit("feat: branch edit");
        r.git(&["checkout", "-q", "main"]);
        r.write("f.txt", &format!("{}{base}", five()));
        r.write("crates/x/Cargo.toml", "[dependencies]\nserde = \"1\"\nbase64 = \"0.23\"\n");
        r.commit("fix: main adds");
        r.git(&["merge", "-q", "-s", "ours", "--no-commit", "feature"]);
        r.write("f.txt", &format!("{base}branch tail one\n"));
        r.git(&["add", "--all"]);
        r.git(&["commit", "-q", "-m", "Merge feature"]);
        r
    }

    #[test]
    fn a_stale_hack_is_reported_beside_the_other_detectors_findings() {
        let (code, out) = check(&dropped_hunk_and_hakari());
        assert_eq!(code, 5, "{out}");
        assert!(out.contains("[dropped-hunk] f.txt"), "{out}");
        assert!(out.contains("[workspace-hack] crates/paddock-workspace-hack/Cargo.toml"), "{out}");
    }

    #[test]
    fn a_hakari_that_cannot_run_keeps_the_other_detectors_findings_and_says_it_was_not_measured() {
        let r = dropped_hunk_and_hakari();
        let out = r
            .dev_cmd()
            .env("PADDOCK_MERGE_CHECK_CARGO", "/nonexistent/cargo")
            .args(["merge", "check", "--root"])
            .arg(r.path())
            .arg("HEAD")
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert_eq!(out.status.code(), Some(5), "{text}");
        assert!(text.contains("[dropped-hunk] f.txt"), "the detectors' finding is not lost: {text}");
        assert!(text.contains("NOT MEASURED") && text.contains("cargo hakari"), "{text}");
    }

    fn check(r: &Repo) -> (i32, String) {
        let bin = tempfile::tempdir().unwrap();
        fake_cargo(bin.path());
        let out = r
            .dev_cmd()
            .env("PADDOCK_MERGE_CHECK_CARGO", bin.path().join("cargo"))
            .args(["merge", "check", "--root"])
            .arg(r.path())
            .arg("HEAD")
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap(), text)
    }

    #[test]
    fn a_stale_workspace_hack_in_the_merge_result_is_a_finding_with_one_error_naming_the_fix() {
        let (code, out) = check(&train(true, false));
        assert_eq!(code, 5, "{out}");
        assert!(out.contains("[workspace-hack] crates/paddock-workspace-hack/Cargo.toml"), "{out}");
        assert!(out.contains("+base64"), "{out}");
        let errors: Vec<&str> = out.lines().filter(|l| l.contains("::error::")).collect();
        assert_eq!(errors.len(), 1, "{out}");
        assert!(
            errors[0].contains("cargo hakari generate") && errors[0].contains("lockfiles"),
            "{out}"
        );
        assert!(!errors[0].contains("Hunks-dropped"), "a merge message cannot excuse it: {out}");
    }

    #[test]
    fn a_merge_that_regenerated_the_workspace_hack_is_clean() {
        let (code, out) = check(&train(true, true));
        assert_eq!(code, 0, "{out}");
    }

    #[test]
    fn a_tree_without_a_hakari_config_is_not_checked() {
        let (code, out) = check(&train(false, false));
        assert_eq!(code, 0, "{out}");
    }

    #[test]
    fn a_hakari_that_cannot_run_is_not_measured_never_a_pass() {
        let r = train(true, false);
        let out = r
            .dev_cmd()
            .env("PADDOCK_MERGE_CHECK_CARGO", "/nonexistent/cargo")
            .args(["merge", "check", "--root"])
            .arg(r.path())
            .arg("HEAD")
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        // Alone it is the merge check's not-measured, exit 6 (ADR 0089 amendment A2), not a finding (5) and never 0.
        assert_eq!(out.status.code(), Some(6), "{text}");
        assert!(text.contains("NOT MEASURED"), "{text}");
        assert_eq!(text.lines().filter(|l| l.contains("::error::")).count(), 1, "{text}");
    }

    /// A pass says what it is a pass of: the merge commit and its tree, in the JSON (ADR 0089 amendment section 2).
    #[test]
    fn a_clean_merge_names_the_commit_and_tree_it_is_clean_for() {
        let r = train(true, true);
        let bin = tempfile::tempdir().unwrap();
        fake_cargo(bin.path());
        let out = r
            .dev_cmd()
            .env("PADDOCK_MERGE_CHECK_CARGO", bin.path().join("cargo"))
            .args(["merge", "check", "--json", "--root"])
            .arg(r.path())
            .arg("HEAD")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let ev = &v["result"]["evidence"];
        let head = r.git(&["rev-parse", "HEAD"]);
        let tree = r.git(&["rev-parse", "HEAD^{tree}"]);
        assert_eq!(v["result"]["status"], "clean", "{v}");
        assert_eq!(
            (ev["subject"]["sha"].as_str(), ev["subject"]["tree"].as_str()),
            (Some(head.trim()), Some(tree.trim())),
            "{v}"
        );
    }
}

// ADR 0098 WP-1: the member gate (`merge check --gates`). A stand-in for `dev check --pre-push` plays the fast gates: it fails
// (exit 5) when the checkout it is run in holds both a.txt and b.txt, the shape of a pair of members that only fail together
// (train 8: a lint list of one branch meeting new code of another).
#[cfg(unix)]
mod member_gate {
    use super::*;

    /// Plays `check --pre-push --member-gate --base <base> --root <root>`, run in `<root>`. Logs its argv and the checkout's HEAD.
    /// What the real push gate prints with --json: an outcome whose evidence names this checkout's commit and tree (ADR 0089 WP-T5).
    fn fake_gates(dir: &Path) {
        let ev = r#"{{"check":"pre-push","subject":{{"kind":"commit","sha":"{run:git|rev-parse|HEAD}","tree":"{run:git|rev-parse|HEAD:}"}},"tool":"fake","at":1}}"#;
        let log = Actions::new().appends_with("{env:GATES_LOG}", "{args} HEAD={run:git|rev-parse|HEAD}\n");
        let together = Case::new().file_exists(Path::new("a.txt")).file_exists(Path::new("b.txt"));
        StandIn::new(dir, "gates")
            .answers(
                log.clone()
                    .stdout("fmt ok; clippy ok")
                    .stdout_with(&format!(r#"{{{{"outcome":{{{{"status":"clean","evidence":{ev}}}}}}}}}"#)),
            )
            .when(
                &together,
                log.stdout("clippy FAILED: a.txt and b.txt together")
                    .stdout_with(&format!(
                        r#"{{{{"outcome":{{{{"status":"findings","findings":[{{{{"message":"clippy","failing":true}}}}],"evidence":{ev}}}}}}}}}"#
                    ))
                    .exits_with_env("GATES_EXIT", 5),
            )
            .install()
            .unwrap();
    }

    struct Train {
        repo: Repo,
        gates: tempfile::TempDir,
    }

    impl Train {
        fn log(&self) -> PathBuf {
            self.gates.path().join("log")
        }

        fn run(&self, args: &[&str], gates_exit: &str) -> (i32, String) {
            let out = self
                .repo
                .dev_cmd()
                .env("PADDOCK_MERGE_CHECK_GATES", self.gates.path().join("gates"))
                .env("GATES_LOG", self.log())
                .env("GATES_EXIT", gates_exit)
                .env("PADDOCK_MERGE_CHECK_CARGO", "/nonexistent/cargo")
                .args(["merge", "check", "--root"])
                .arg(self.repo.path())
                .args(args)
                .output()
                .unwrap();
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            (out.status.code().unwrap(), text)
        }
    }

    /// A train with member `a` merged; `b` is merged too when `both`. HEAD is the last merge.
    fn train(both: bool) -> Train {
        let repo = Repo::new();
        repo.write("base.txt", "base\n");
        repo.commit("base");
        for (member, on) in [("a", true), ("b", both)] {
            if !on {
                continue;
            }
            repo.git(&["checkout", "-q", "-b", member, "main"]);
            repo.write(&format!("{member}.txt"), &format!("{member}\n"));
            repo.commit(&format!("feat: {member}"));
            repo.git(&["checkout", "-q", "main"]);
            repo.git(&["merge", "-q", "--no-ff", "-m", &format!("Merge {member}"), member]);
        }
        let gates = tempfile::tempdir().unwrap();
        fake_gates(gates.path());
        Train { repo, gates }
    }

    #[test]
    fn a_member_that_passes_the_gates_on_the_combined_tree_is_reported_passed() {
        let t = train(false);
        let (code, out) = t.run(&["--gates", "HEAD"], "5");
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("member gate: passed"), "{out}");
        assert!(
            out.contains("check --pre-push --member-gate --base"),
            "the pass names the command that ran: {out}"
        );
        assert!(!out.contains("FAILED"), "{out}");
    }

    /// Row 7 of the ADR 0089 amendment, through the real binary: a stand-in gate that exits 0 and prints no outcome, or an outcome
    /// whose evidence is for another commit, is not a pass: exit 6 (the gate blocks since 2026-11-24) and NOT MEASURED, never "passed".
    #[test]
    fn a_gate_that_exits_0_without_an_outcome_for_this_merge_is_not_a_pass() {
        let t = train(false);
        let stale = r#"{"outcome":{"status":"clean","evidence":{"check":"pre-push","subject":{"kind":"commit","sha":"0000000000000000000000000000000000000001","tree":"0000000000000000000000000000000000000002"},"tool":"x","at":1}}}"#;
        for body in ["fine", r#"{"outcome":{"status":"clean"}}"#, stale] {
            StandIn::new(t.gates.path(), "gates").stdout(body).install().unwrap();
            let (code, out) = t.run(&["--gates", "--json", "HEAD"], "0");
            assert_eq!(code, 6, "not measured is never a pass: {out}");
            assert!(out.contains("NOT MEASURED") || out.contains("not measured"), "{body}: {out}");
            assert!(!out.contains("member gate: passed"), "{body}: {out}");
        }
    }

    #[test]
    fn two_members_that_only_fail_together_are_named_at_the_merge_that_joins_them_and_the_gate_refuses() {
        let t = train(true);
        let (code, out) = t.run(&["--gates", "HEAD"], "5");
        // Enforced since 2026-11-24 (ADR 0098's status): the rows and the error are printed and the exit is the findings exit.
        assert_eq!(code, 5, "{out}");
        assert!(out.contains("clippy FAILED: a.txt and b.txt together"), "{out}");
        assert!(out.contains("::error::member gate FAILED"), "{out}");
        assert!(
            !out.lines().any(|l| l.contains("member gate FAILED") && l.contains("REPORT-ONLY")),
            "{out}"
        );
        assert!(out.contains("take the member out") && out.contains("send it back"), "{out}");
        assert!(
            out.contains("`check --pre-push --member-gate --base") && !out.contains("`check --pre-push --base"),
            "the next step names the member-gate command (a plain push skips the audits): {out}"
        );
    }

    #[test]
    fn the_gates_run_on_the_merge_with_the_train_before_it_as_the_base() {
        let t = train(true);
        let first_parent = t.repo.git(&["rev-parse", "HEAD^1"]);
        let head = t.repo.git(&["rev-parse", "HEAD"]);
        t.run(&["--gates", "HEAD"], "5");
        let log = fs::read_to_string(t.log()).unwrap();
        assert!(
            log.contains(&format!("check --pre-push --member-gate --base {first_parent} --root")) && log.contains(&format!("HEAD={head}")),
            "{log}"
        );
    }

    /// `--gates-base <ref>` measures the gate from that ref, not from the merge's first parent: the whole train's rows, so a push of the
    /// same tree finds a pass for each (user, 2026-11-26). The default is unchanged (the test above); this is the positive control that
    /// the flag changes what the gate is asked.
    #[test]
    fn gates_base_measures_the_gate_from_the_named_ref() {
        let t = train(true);
        let first_parent = t.repo.git(&["rev-parse", "HEAD^1"]);
        let base = t.repo.git(&["rev-parse", "HEAD~2"]);
        t.run(&["--gates", "--gates-base", &base, "HEAD"], "5");
        let log = fs::read_to_string(t.log()).unwrap();
        assert!(
            log.contains(&format!("check --pre-push --member-gate --base {base} --root")),
            "{log}"
        );
        assert!(
            !log.contains(&format!("--base {first_parent} --root")),
            "the first parent is not the base now: {log}"
        );
    }

    #[test]
    fn without_gates_nothing_is_run() {
        let t = train(true);
        let (code, out) = t.run(&["HEAD"], "5");
        assert_eq!(code, 0, "{out}");
        assert!(!t.log().exists(), "the gates ran without --gates");
        assert!(!out.contains("member gate"), "{out}");
    }

    #[test]
    fn a_checkout_that_is_not_the_merge_is_not_measured_and_runs_no_gate() {
        let t = train(true);
        t.repo.git(&["checkout", "-q", "a"]);
        let merge = t.repo.git(&["rev-parse", "main"]);
        let (code, out) = t.run(&["--gates", &merge], "5");
        assert_eq!(code, 6, "not measured: {out}");
        assert!(
            out.contains("member gate NOT MEASURED") && out.contains("not the merge result"),
            "{out}"
        );
        assert!(!t.log().exists(), "a gate ran on a checkout that is not the merge");
    }

    #[test]
    fn a_gate_that_could_not_look_is_never_reported_as_a_pass() {
        let t = train(true);
        let (code, out) = t.run(&["--gates", "HEAD"], "6");
        assert_eq!(code, 6, "{out}");
        assert!(out.contains("member gate NOT MEASURED"), "{out}");
        assert!(!out.contains("member gate: passed"), "{out}");
    }

    #[test]
    fn json_carries_the_gate_result_under_its_own_key() {
        let t = train(true);
        let (code, out) = t.run(&["--gates", "HEAD", "--json"], "5");
        assert_eq!(code, 5, "{out}");
        let v: serde_json::Value = serde_json::from_str(out.lines().find(|l| l.starts_with('{')).unwrap()).unwrap();
        assert_eq!(v["gates"]["outcome"], "failed", "{out}");
        assert_eq!(v["gates"]["enforced"], true, "{out}");
    }

    #[test]
    fn a_branch_checked_before_merging_has_no_checkout_to_gate() {
        let t = train(false);
        t.repo.git(&["checkout", "-q", "-b", "c", "main"]);
        t.repo.write("c.txt", "c\n");
        t.repo.commit("feat: c");
        let (code, out) = t.run(&["--branch", "c", "--target", "main"], "5");
        assert_eq!(code, 0, "{out}");
        let (code, out) = t.run(&["--branch", "c", "--target", "main", "--gates"], "5");
        assert_eq!(code, 0, "{out}");
        assert!(
            out.contains("member gate not run") && out.contains("before it is merged"),
            "said out loud, not silent: {out}"
        );
        assert!(!t.log().exists(), "a gate ran without a merge checkout");
    }
}

/// main merges `feat/x` (M), `member` starts from `from` ("M", "x": the reverted branch's own tip, or "base": before the merge),
/// and main then does `after` (reverts). `member` and main touch other files, so the member merges cleanly into main and only the
/// reverted-base warning can speak.
fn reverted_base_from(from: &str, after: impl Fn(&Repo, &str)) -> Repo {
    let r = Repo::new();
    r.write("base.txt", "base\n");
    let base = r.commit("base");
    r.git(&["checkout", "-q", "-b", "feat/x"]);
    r.write("x.txt", "x\n");
    r.commit("feat: x");
    r.git(&["checkout", "-q", "main"]);
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge branch 'feat/x'", "feat/x"]);
    let m = r.git(&["rev-parse", "HEAD"]);
    let start = match from {
        "M" => m.clone(),
        "x" => "feat/x".to_string(),
        _ => base,
    };
    r.git(&["checkout", "-q", "-b", "member", &start]);
    r.write("member.txt", "member\n");
    r.commit("feat: member");
    r.git(&["checkout", "-q", "main"]);
    r.write("other.txt", "other\n");
    r.commit("feat: other");
    after(&r, &m);
    r
}

fn reverted_base_repo(after: impl Fn(&Repo, &str)) -> Repo {
    reverted_base_from("M", after)
}

fn revert_merge(r: &Repo, sha: &str) -> String {
    r.git(&["revert", "-m", "1", "--no-edit", sha]);
    r.git(&["rev-parse", "HEAD"])
}

const WARNING: &str = "::warning::";
const NEXT: &str = "on main, or land the reverted branch first";

#[test]
fn a_member_based_on_a_merge_the_target_reverted_warns_and_still_passes() {
    // The positive control (ER-33): the same history as the two silent cases below, with the revert present.
    let r = reverted_base_repo(|r, m| {
        revert_merge(r, m);
    });
    let (code, text) = r.check(&["--branch", "member", "--target", "main", "--no-generators"]);
    assert_eq!(code, 0, "report-only, never a refusal: {text}");
    assert_eq!(text.matches(WARNING).count(), 1, "{text}");
    assert!(text.contains("feat/x") && text.contains(NEXT), "{text}");
    let revert = r.git(&["rev-parse", "--short=9", "HEAD"]);
    assert!(text.contains(&revert), "names the revert commit: {text}");
}

#[test]
fn a_reland_by_revert_of_revert_does_not_warn() {
    let r = reverted_base_repo(|r, m| {
        let rev = revert_merge(r, m);
        r.git(&["revert", "--no-edit", &rev]);
    });
    let (code, text) = r.check(&["--branch", "member", "--target", "main", "--no-generators"]);
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains(WARNING), "{text}");
}

#[test]
fn a_clean_member_is_silent() {
    let r = reverted_base_repo(|_, _| {});
    let (code, text) = r.check(&["--branch", "member", "--target", "main", "--no-generators"]);
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains(WARNING), "{text}");
}

#[test]
fn a_member_that_conflicts_with_the_revert_is_not_measured_and_names_the_reverted_merge() {
    // The case that started it: the member edits what the revert took out, so it does not merge at all.
    let r = reverted_base_repo(|r, m| {
        revert_merge(r, m);
    });
    r.git(&["checkout", "-q", "member"]);
    r.write("x.txt", "member edit\n");
    r.commit("feat: member edits x");
    r.git(&["checkout", "-q", "main"]);
    let (code, text) = r.check(&["--branch", "member", "--target", "main", "--no-generators"]);
    assert_eq!(code, 6, "{text}");
    assert!(text.contains("does not merge cleanly"), "{text}");
    assert_eq!(text.matches(WARNING).count(), 1, "{text}");
    assert!(text.contains("feat/x") && text.contains(NEXT), "{text}");
}

const BRANCH_ARGS: [&str; 5] = ["--branch", "member", "--target", "main", "--no-generators"];

fn warnings(text: &str) -> Vec<&str> {
    text.lines().filter(|l| l.contains(WARNING)).collect()
}

#[test]
fn a_member_forked_from_the_reverted_branch_itself_warns() {
    let r = reverted_base_from("x", |r, m| {
        revert_merge(r, m);
    });
    let (code, text) = r.check(&BRANCH_ARGS);
    assert_eq!(code, 0, "{text}");
    assert_eq!(warnings(&text).len(), 1, "{text}");
    assert!(text.contains("is built on feat/x, which main reverted"), "{text}");
}

#[test]
fn a_member_forked_before_the_merge_is_silent_although_the_target_reverted_it() {
    let r = reverted_base_from("base", |r, m| {
        revert_merge(r, m);
    });
    let (code, text) = r.check(&BRANCH_ARGS);
    assert_eq!(code, 0, "{text}");
    assert!(warnings(&text).is_empty(), "{text}");
}

#[test]
fn a_reverted_commit_that_is_not_a_merge_is_silent() {
    // The member holds a plain commit the target reverted: no merge, nothing brought in, so only a merge counts.
    let r = Repo::new();
    r.write("base.txt", "base\n");
    r.commit("base");
    r.write("p.txt", "p\n");
    let plain = r.commit("feat: plain");
    r.git(&["checkout", "-q", "-b", "member"]);
    r.write("member.txt", "member\n");
    r.commit("feat: member");
    r.git(&["checkout", "-q", "main"]);
    r.git(&["revert", "--no-edit", &plain]);
    let (code, text) = r.check(&BRANCH_ARGS);
    assert_eq!(code, 0, "{text}");
    assert!(warnings(&text).is_empty(), "{text}");
}

fn revert_by_subject(r: &Repo, m: &str, subject: &str) {
    r.git(&["revert", "-m", "1", "--no-commit", m]);
    r.commit(subject);
}

#[test]
fn a_pull_out_without_the_trailer_is_found_by_its_subject() {
    let r = reverted_base_repo(|r, m| revert_by_subject(r, m, "Revert \"Merge branch 'feat/x'\""));
    let (code, text) = r.check(&BRANCH_ARGS);
    assert_eq!(code, 0, "{text}");
    assert_eq!(warnings(&text).len(), 1, "{text}");
    assert!(text.contains("feat/x") && text.contains(NEXT), "{text}");
}

#[test]
fn a_revert_subject_that_names_another_commit_is_silent_and_one_that_names_none_is_not_measured() {
    let r = reverted_base_repo(|r, _| {
        r.write("other.txt", "again\n");
        r.commit("Revert \"feat: other\"");
    });
    let (_, text) = r.check(&BRANCH_ARGS);
    assert!(warnings(&text).is_empty(), "a plain commit was named: {text}");
    let r = reverted_base_repo(|r, _| {
        r.write("other.txt", "again\n");
        r.commit("Revert \"nothing has this subject\"");
    });
    let (code, text) = r.check(&BRANCH_ARGS);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("::warning::reverted-base check NOT MEASURED") && text.contains("nothing has this subject"),
        "{text}"
    );
}

#[test]
fn a_later_merge_of_the_same_branch_re_lands_it() {
    let r = reverted_base_repo(|r, m| {
        revert_merge(r, m);
        r.git(&["checkout", "-q", "feat/x"]);
        r.write("x2.txt", "x2\n");
        r.commit("feat: x again");
        r.git(&["checkout", "-q", "main"]);
        r.git(&["merge", "-q", "--no-ff", "-m", "Merge branch 'feat/x'", "feat/x"]);
    });
    let (code, text) = r.check(&BRANCH_ARGS);
    assert_eq!(code, 0, "{text}");
    assert!(warnings(&text).is_empty(), "{text}");
}

#[test]
fn an_abbreviated_sha_in_the_trailer_is_resolved() {
    let r = reverted_base_repo(|r, m| {
        r.git(&["revert", "-m", "1", "--no-commit", m]);
        r.commit(&format!("Revert x\n\n  This reverts commit {}, reversing\nchanges.\n", &m[..8]));
    });
    let (_, text) = r.check(&BRANCH_ARGS);
    assert_eq!(warnings(&text).len(), 1, "{text}");
}

#[test]
fn several_reverted_merges_of_one_branch_are_one_line() {
    let r = Repo::new();
    r.write("base.txt", "base\n");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "feat/x"]);
    r.write("x.txt", "x\n");
    r.commit("feat: x");
    r.git(&["checkout", "-q", "main"]);
    r.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "chore(train): merge feat/x (abc1234) into train 1",
        "feat/x",
    ]);
    let m1 = r.git(&["rev-parse", "HEAD"]);
    r.git(&["checkout", "-q", "feat/x"]);
    r.write("x2.txt", "x2\n");
    r.commit("feat: x again");
    r.git(&["checkout", "-q", "main"]);
    r.git(&["merge", "-q", "--no-ff", "-m", "chore(train): merge feat/x into train 1", "feat/x"]);
    let m2 = r.git(&["rev-parse", "HEAD"]);
    r.git(&["checkout", "-q", "-b", "member"]);
    r.write("member.txt", "member\n");
    r.commit("feat: member");
    r.git(&["checkout", "-q", "main"]);
    revert_merge(&r, &m2);
    revert_merge(&r, &m1);
    let (code, text) = r.check(&BRANCH_ARGS);
    assert_eq!(code, 0, "{text}");
    let lines = warnings(&text);
    assert_eq!(lines.len(), 1, "{text}");
    assert!(
        lines[0].contains("merge of feat/x, which") && !lines[0].contains("abc1234"),
        "{text}"
    );
    assert!(
        lines[0].contains(&m1[..9]) && lines[0].contains(&m2[..9]),
        "both merges are listed: {text}"
    );
}

#[test]
fn the_warning_names_the_target_not_a_sha_and_the_commit_form_says_to_build_and_test() {
    let r = reverted_base_repo(|r, m| {
        revert_merge(r, m);
    });
    let (_, text) = r.check(&BRANCH_ARGS);
    assert!(text.contains("re-base member on main"), "{text}");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge member", "member"]);
    let (code, text) = r.check(&["HEAD", "--no-generators"]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("::warning::") && text.contains("may depend on feat/x; build and test the merge"),
        "{text}"
    );
}

#[test]
fn a_shallow_clone_on_the_conflict_path_is_not_measured() {
    let r = reverted_base_repo(|r, m| {
        revert_merge(r, m);
    });
    r.git(&["checkout", "-q", "member"]);
    r.write("x.txt", "member edit\n");
    r.commit("feat: member edits x");
    r.git(&["checkout", "-q", "main"]);
    let clone = tempfile::tempdir().unwrap();
    let url = format!("file://{}", paddock_testkit::path::text(r.path()));
    r.git(&[
        "clone",
        "-q",
        "--depth",
        "1",
        "--no-single-branch",
        &url,
        clone.path().to_str().unwrap(),
    ]);
    let (code, text) = r.check_at(
        clone.path(),
        &["--branch", "origin/member", "--target", "origin/main", "--no-generators"],
    );
    assert_eq!(code, 6, "{text}");
    assert!(
        text.contains("::warning::reverted-base check NOT MEASURED") && text.contains("shallow"),
        "{text}"
    );
}

// ---- `--write-reverted`: the `Reverted:` lines of a re-land ----

/// A train that took work X (`shot.txt`, three lines replaced) and reverted it (revert S), plus one revert of something else
/// (`other.txt`, S2). The member starts at S2 and holds `L`, made by `reland`; main then moves on, so the merge is a real one.
/// Returns the repository and the revert S.
fn reland_repo(reland: impl Fn(&Repo, &str)) -> (Repo, String) {
    let r = Repo::new();
    r.write("shot.txt", "old1\nold2\nold3\n");
    r.write("other.txt", "o1\no2\no3\n");
    r.commit("base");
    r.write("shot.txt", "new1\nnew2\nnew3\n");
    let x = r.commit("feat: refresh screenshots");
    r.write("other.txt", "p1\np2\np3\n");
    let y = r.commit("feat: other refresh");
    r.git(&["revert", "--no-edit", &x]);
    let s = r.git(&["rev-parse", "HEAD"]);
    r.git(&["revert", "--no-edit", &y]);
    r.git(&["checkout", "-q", "-b", "member"]);
    reland(&r, &s);
    r.git(&["checkout", "-q", "main"]);
    r.write("unrelated.txt", "u\n");
    r.commit("feat: unrelated");
    (r, s)
}

fn write_reverted(r: &Repo) -> (i32, String, String) {
    let file = r
        .path()
        .join("..")
        .join(format!("msg-{}.txt", r.path().file_name().unwrap().to_string_lossy()));
    let (code, text) = r.check(&["--branch", "member", "--target", "main", "--write-reverted", file.to_str().unwrap()]);
    let written = fs::read_to_string(&file).unwrap_or_default();
    let _ = fs::remove_file(&file);
    (code, text, written)
}

/// git's own message for a revert of a revert, as `git revert` writes it, with a re-land subject.
fn reapply(r: &Repo, s: &str) {
    r.git(&["revert", "--no-edit", s]);
    let msg = format!("Reapply \"feat: refresh screenshots\"\n\nThis reverts commit {s}.\n");
    r.git(&["commit", "-q", "--amend", "-m", &msg]);
}

#[test]
fn a_reland_that_undoes_a_train_revert_gets_its_reverted_lines_and_the_rerun_passes() {
    let (r, s) = reland_repo(reapply);
    // Without the flag it is a finding (the control: the history really goes backwards).
    let (code, text) = r.check(&["--branch", "member", "--target", "main"]);
    assert_eq!(code, 5, "{text}");
    assert!(text.contains("[backwards] shot.txt"), "{text}");
    let (code, text, written) = write_reverted(&r);
    assert_eq!(code, 5, "the findings stay until the message carries the lines: {text}");
    assert!(
        written.contains("Reverted: shot.txt: ") && written.contains(&s[..9]) && written.starts_with("Merge branch 'member' into main"),
        "{written}\n{text}"
    );
    assert!(text.contains("1 of 1 finding(s) are the re-land's own"), "{text}");
    // The merge made with that message passes the check.
    let msg = r.path().join("..").join("msg-final.txt");
    fs::write(&msg, &written).unwrap();
    r.git(&["merge", "-q", "--no-ff", "-F", msg.to_str().unwrap(), "member"]);
    let (code, text) = r.check(&["HEAD"]);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn a_member_that_only_changes_the_file_back_without_being_a_reland_gets_no_line() {
    // The same lines go backwards, but the commit is a plain edit: no `Reapply` subject, no revert named.
    let (r, _) = reland_repo(|r, _| {
        r.write("shot.txt", "new1\nnew2\nnew3\n");
        r.commit("feat: new screenshots again");
    });
    let (code, text, written) = write_reverted(&r);
    assert_eq!(code, 5, "{text}");
    assert!(text.contains("[backwards] shot.txt"), "{text}");
    assert!(written.is_empty(), "nothing is written for a finding it cannot tie: {written}");
    assert!(text.contains("no finding ties to a re-land"), "{text}");
}

#[test]
fn a_reland_of_another_revert_does_not_answer_a_finding_in_a_file_it_never_touched() {
    // L re-applies the revert of `other.txt` (S2), while the backwards finding is in shot.txt: the member also edits shot.txt by
    // a plain commit, which is no re-land.
    let (r, _) = reland_repo(|r, _| {
        let s2 = r.git(&["rev-parse", "HEAD~0"]);
        r.git(&["revert", "--no-edit", &s2]);
        let msg = format!("Reapply \"feat: other refresh\"\n\nThis reverts commit {s2}.\n");
        r.git(&["commit", "-q", "--amend", "-m", &msg]);
        r.write("shot.txt", "new1\nnew2\nnew3\n");
        r.commit("feat: shot edit");
    });
    let (code, text, written) = write_reverted(&r);
    assert_eq!(code, 5, "{text}");
    assert!(text.contains("[backwards] shot.txt"), "{text}");
    assert!(!written.contains("shot.txt"), "shot.txt is not the re-land's file: {written}");
}

#[test]
fn a_reland_that_names_a_commit_that_is_not_a_revert_ties_nothing() {
    // `Reapply` that "reverts" X itself (the feature commit, not a revert of it): S must be a revert.
    let (r, _) = reland_repo(|r, _| {
        let x = r.git(&["log", "--format=%H", "-n", "1", "--grep", "^feat: refresh screenshots", "main"]);
        r.write("shot.txt", "new1\nnew2\nnew3\n");
        r.git(&["add", "--all"]);
        let msg = format!("Reapply \"feat: refresh screenshots\"\n\nThis reverts commit {x}.\n");
        r.git(&["commit", "-q", "-m", &msg]);
    });
    let (code, text, written) = write_reverted(&r);
    assert_eq!(code, 5, "{text}");
    assert!(written.is_empty(), "{written}");
}

// ---- `merge sync-member` (read-only) ----

/// `dev merge sync-member <train> <extra>` in this repository's checkout.
fn sync_member(r: &Repo, train: &str, extra: &[&str]) -> (i32, String) {
    let out = r
        .cmd(crate::dev_bin::PATH)
        .args(["merge", "sync-member", "--root"])
        .arg(r.path())
        .args(extra)
        .arg(train)
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap(), text)
}

/// The static rows of a sync tip need a repository with checks; the read-only tests of the revert listing opt out of them.
const NO_STATIC: [&str; 4] = [
    "--no-static",
    "fixture: only the revert listing is under test",
    "--no-trial",
    "fixture: only the revert listing is under test",
];

/// The train (`main`) merged `feat/x` and reverted the merge; main then moved on. HEAD ends on `feat/x` (the reverted branch itself).
/// Returns the repository and the revert commit.
fn reverted_own_branch() -> (Repo, String) {
    let r = Repo::new();
    r.write("base.txt", "base\n");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "feat/x"]);
    r.write("x.txt", "x\n");
    r.commit("feat: x");
    r.git(&["checkout", "-q", "main"]);
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge branch 'feat/x'", "feat/x"]);
    let m = r.git(&["rev-parse", "HEAD"]);
    let s = revert_merge(&r, &m);
    r.write("other.txt", "other\n");
    r.commit("feat: other");
    r.git(&["checkout", "-q", "feat/x"]);
    (r, s)
}

#[test]
fn sync_member_lists_the_git_revert_of_the_members_own_merge_and_runs_nothing() {
    let (r, s) = reverted_own_branch();
    r.write("scratch.txt", "dirty\n"); // read-only: a dirty tree is no reason to refuse
    let head = r.git(&["rev-parse", "HEAD"]);
    let (code, text) = sync_member(&r, "main", &[]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains(&format!("git revert {s}")) && text.contains("1 revert(s) of this member's own merges"),
        "{text}"
    );
    assert_eq!(r.git(&["rev-parse", "HEAD"]), head, "nothing committed");
    assert!(
        r.path().join("scratch.txt").exists() && !r.git(&["status", "--porcelain"]).is_empty(),
        "nothing touched"
    );
    // The control (ER-33): the plain merge of the train deletes the work; the printed command gives it back (git's howto).
    fs::remove_file(r.path().join("scratch.txt")).unwrap();
    r.git(&["merge", "-q", "--no-edit", "main"]);
    assert!(
        !r.path().join("x.txt").exists(),
        "control: the plain merge deletes the member's work"
    );
    r.git(&["revert", "--no-edit", &s]);
    assert!(r.path().join("x.txt").exists(), "the printed command re-applies it");
}

#[test]
fn sync_member_takes_the_member_name_with_or_without_the_remote_prefix_or_from_the_option() {
    let (r, s) = reverted_own_branch();
    for name in ["origin/feat/x", "feat/x"] {
        let (code, text) = sync_member(&r, "main", &["--member", name]);
        assert_eq!(code, 0, "{text}");
        assert!(text.contains(&format!("git revert {s}")), "{name}: {text}");
    }
    r.git(&["checkout", "-q", "--detach"]);
    let (code, text) = sync_member(&r, "main", &[]);
    assert_ne!(code, 0);
    assert!(
        text.contains("HEAD is not on a branch, so the member's name is unknown. Next step:"),
        "{text}"
    );
    let (code, text) = sync_member(&r, "main", &["--member", "feat/x"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("git revert "), "{text}");
}

#[test]
fn sync_member_warns_for_the_revert_of_another_branchs_merge_and_prints_no_command_for_it() {
    // `member` is forked from feat/x (it holds the merge's commits) but is not feat/x.
    let (r, _) = reverted_own_branch();
    r.git(&["checkout", "-q", "-b", "member", "feat/x"]);
    r.write("member.txt", "member\n");
    r.commit("feat: member");
    let (code, text) = sync_member(&r, "main", &[]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("::warning::") && text.contains("re-base on the train, or land feat/x first") && !text.contains("git revert "),
        "{text}"
    );
    assert!(
        text.contains("0 revert(s) of this member's own merges to re-apply, 1 of other branches"),
        "{text}"
    );
}

#[test]
fn sync_member_names_nothing_for_a_member_forked_before_the_reverted_merge() {
    let r = reverted_base_from("base", |r, m| {
        revert_merge(r, m);
    });
    r.git(&["checkout", "-q", "member"]);
    let (code, text) = sync_member(&r, "main", &[]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("0 revert(s) of this member's own merges to re-apply, 0 of other branches") && !text.contains("git revert "),
        "{text}"
    );
}

#[test]
fn sync_member_lists_other_branches_reverts_in_the_order_the_train_made_them() {
    // Four merges reverted one after another (c, a, d, b): the order is the history's, not the sha order.
    let r = Repo::new();
    r.write("base.txt", "base\n");
    r.commit("base");
    let mut merges = Vec::new();
    for b in ["c", "a", "d", "b"] {
        r.git(&["checkout", "-q", "-b", &format!("feat/{b}"), "main"]);
        r.write(&format!("{b}.txt"), "x\n");
        r.commit(&format!("feat: {b}"));
        r.git(&["checkout", "-q", "main"]);
        r.git(&[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            &format!("Merge branch 'feat/{b}'"),
            &format!("feat/{b}"),
        ]);
        merges.push(r.git(&["rev-parse", "HEAD"]));
    }
    // The member holds all four (forked from the train head), then the train reverts them in the order c, a, d, b.
    r.git(&["checkout", "-q", "-b", "member"]);
    r.write("member.txt", "m\n");
    r.commit("feat: member");
    r.git(&["checkout", "-q", "main"]);
    for m in &merges {
        revert_merge(&r, m);
    }
    r.git(&["checkout", "-q", "member"]);
    let (code, text) = sync_member(&r, "main", &[]);
    assert_eq!(code, 0, "{text}");
    let at = |b: &str| {
        text.find(&format!("of feat/{b} ("))
            .unwrap_or_else(|| panic!("feat/{b} missing: {text}"))
    };
    assert!(at("c") < at("a") && at("a") < at("d") && at("d") < at("b"), "{text}");
}

// ---- a member should contain the train's base (stale_base, blocking) ----

/// A train cut from main (m1) that merged `feat/a`; main then moved to m2. `merged_m2`: the train has merged main up to m2.
/// Branches `old` (built on m1) and `new` (built on m2) each add one file. HEAD is on the train; `origin/main` is main's tip.
fn train_on_main(merged_m2: bool) -> (Repo, String) {
    let r = Repo::new();
    r.write("base.txt", "base\n");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "old"]);
    r.write("old.txt", "old\n");
    r.commit("feat: old");
    r.git(&["checkout", "-q", "main"]);
    r.git(&["checkout", "-q", "-b", "train"]);
    r.git(&["checkout", "-q", "-b", "feat/a"]);
    r.write("a.txt", "a\n");
    r.commit("feat: a");
    r.git(&["checkout", "-q", "train"]);
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge feat/a (aaaaaaaaa) into train", "feat/a"]);
    r.git(&["checkout", "-q", "main"]);
    r.write("m2.txt", "m2\n");
    let m2 = r.commit("feat: m2 on main");
    r.git(&["update-ref", "refs/remotes/origin/main", &m2]);
    r.git(&["checkout", "-q", "-b", "new"]);
    r.write("new.txt", "new\n");
    r.commit("feat: new");
    r.git(&["checkout", "-q", "train"]);
    if merged_m2 {
        r.git(&["merge", "-q", "--no-ff", "-m", "chore(train): merge main into train", "main"]);
    }
    (r, m2)
}

fn check_member(r: &Repo, member: &str) -> (i32, String) {
    r.check(&["--branch", member, "--target", "train", "--no-generators"])
}

#[test]
fn a_member_built_before_the_commit_of_main_the_train_has_merged_is_refused_with_one_error_line() {
    let (r, m2) = train_on_main(true);
    let (code, text) = check_member(&r, "old");
    assert_ne!(code, 0, "the old base blocks: {text}");
    assert!(
        text.contains("::error::merge check: old is built on an old base: it does not contain ")
            && text.contains(&m2[..9])
            && text.contains("the newest commit of main that train has merged")
            && text.contains("Next step: merge origin/main into old and re-run its gate")
            && text.contains("--old-base-anyway")
            && !text.contains("report-only"),
        "{text}"
    );
    // The member built on that commit passes; the other member merged ahead of it (feat/a) is not required.
    let (code, text) = check_member(&r, "new");
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("old base"), "{text}");
}

#[test]
fn old_base_anyway_lets_the_old_member_through_and_prints_its_reason() {
    let (r, m2) = train_on_main(true);
    let (code, text) = r.check(&[
        "--branch",
        "old",
        "--target",
        "train",
        "--no-generators",
        "--old-base-anyway",
        "hotfix, unrelated to main's m2",
    ]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("::warning::merge check: old is built on an old base")
            && text.contains(&m2[..9])
            && text.contains("--old-base-anyway: hotfix, unrelated to main's m2")
            && !text.contains("::error::"),
        "{text}"
    );
}

#[test]
fn old_base_anyway_without_a_reason_is_a_usage_error_and_does_not_pass_the_member() {
    let (r, _) = train_on_main(true);
    let (code, text) = r.check(&["--branch", "old", "--target", "train", "--no-generators", "--old-base-anyway", " "]);
    assert_ne!(code, 0, "{text}");
    assert!(text.contains("--old-base-anyway needs a reason"), "{text}");
}

#[test]
fn the_same_member_passes_while_the_train_has_not_merged_the_newer_commit_of_main() {
    // The positive control of the block (deleting the check makes the refusal test fail; this one shows the check can say no): identical history without the train's merge of m2.
    let (r, _) = train_on_main(false);
    let (code, text) = check_member(&r, "old");
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("old base"), "{text}");
}

/// A train stacked on `train/prev`: prev has p1 and p2; `tr2` re-merged prev up to p1 with the given subject.
fn stacked_train(subject: impl Fn(&str) -> String) -> (Repo, String) {
    let r = Repo::new();
    r.write("base.txt", "base\n");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "before_p1"]);
    r.write("b.txt", "b\n");
    r.commit("feat: built before p1");
    r.git(&["checkout", "-q", "main"]);
    r.git(&["checkout", "-q", "-b", "train/prev"]);
    r.write("p1.txt", "p1\n");
    let p1 = r.commit("feat: p1");
    r.git(&["checkout", "-q", "-b", "on_p1"]);
    r.write("c.txt", "c\n");
    r.commit("feat: built on p1");
    r.git(&["checkout", "-q", "train/prev"]);
    r.write("p2.txt", "p2\n");
    r.commit("feat: p2");
    r.git(&["checkout", "-q", "-b", "on_p2"]);
    r.write("d.txt", "d\n");
    r.commit("feat: built on p2");
    r.git(&["checkout", "-q", "main"]);
    r.git(&["update-ref", "refs/remotes/origin/main", "main"]);
    r.git(&["checkout", "-q", "-b", "tr2"]);
    r.git(&["merge", "-q", "--no-ff", "-m", &subject(&p1[..9]), &p1]);
    (r, p1)
}

fn tr2(r: &Repo, member: &str) -> (i32, String) {
    r.check(&["--branch", member, "--target", "tr2", "--no-generators"])
}

#[test]
fn on_a_stacked_train_a_member_should_contain_the_previous_trains_commit_the_train_has_merged() {
    let (r, p1) = stacked_train(|h| format!("chore(train): merge train/prev ({h}) into train 2"));
    let (code, text) = tr2(&r, "before_p1");
    assert_ne!(code, 0, "{text}");
    assert!(
        text.contains(&p1[..9]) && text.contains("the newest commit of train/prev that tr2 has merged"),
        "{text}"
    );
    // Built on p1 (the merged commit) and built on p2 (newer, contains p1) get none.
    for member in ["on_p1", "on_p2"] {
        let (code, text) = tr2(&r, member);
        assert_eq!(code, 0, "{member}: {text}");
        assert!(!text.contains("old base"), "{member}: {text}");
    }
}

// The re-merge subject in every spelling the queue and git write. One test per spelling: the four in one test took 44 to 64 s on the
// Windows runner under the suite's load (ci.yml 8271 timed out at 60 s; windows-host-probe 8277 and 8280), each case a quarter of it.
fn remerge_subject_is_read(subject: fn(&str) -> String) {
    let (r, p1) = stacked_train(subject);
    let (code, text) = tr2(&r, "before_p1");
    assert_ne!(code, 0, "{text}");
    assert!(text.contains(&p1[..9]) && text.contains("old base"), "{text}");
}

#[test]
fn the_remerge_subject_is_read_in_upper_case() {
    remerge_subject_is_read(|h| format!("CHORE(TRAIN): MERGE TRAIN/PREV ({h}) INTO TRAIN 2"));
}

#[test]
fn the_remerge_subject_is_read_in_the_train_slash_spelling() {
    remerge_subject_is_read(|h| format!("Merge train/prev ({h}) into train/2"));
}

#[test]
fn the_remerge_subject_is_read_as_gits_merge_branch_message() {
    remerge_subject_is_read(|_| "Merge branch 'train/prev' into tr2".to_string());
}

#[test]
fn the_remerge_subject_is_read_as_gits_merge_remote_tracking_branch_message() {
    remerge_subject_is_read(|_| "Merge remote-tracking branch 'origin/train/prev' into tr2".to_string());
}

#[test]
fn a_re_merge_that_landed_on_main_is_not_this_trains_base_and_main_ahead_of_origin_is_no_train() {
    // main carries an older train's re-merge (as 0a78cae1f does); the train is cut from main afterwards and merges main up to m2.
    let r = Repo::new();
    r.write("base.txt", "base\n");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "train/old"]);
    r.write("o.txt", "o\n");
    let o = r.commit("feat: o");
    r.git(&["checkout", "-q", "main"]);
    r.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        &format!("chore(train): merge train/old ({}) into train 12", &o[..9]),
        &o,
    ]);
    r.git(&["checkout", "-q", "-b", "stale"]);
    r.write("stale.txt", "s\n");
    r.commit("feat: built on the old main");
    r.git(&["checkout", "-q", "main"]);
    r.git(&["checkout", "-q", "-b", "tr3"]);
    r.git(&["checkout", "-q", "main"]);
    r.write("m2.txt", "m2\n");
    let m2 = r.commit("feat: m2");
    r.git(&["update-ref", "refs/remotes/origin/main", &m2]);
    r.git(&["checkout", "-q", "tr3"]);
    r.git(&["merge", "-q", "--no-ff", "-m", "chore(train): merge main into train", "main"]);
    let (code, text) = r.check(&["--branch", "stale", "--target", "tr3", "--no-generators"]);
    assert_ne!(code, 0, "{text}");
    assert!(
        text.contains("the newest commit of main that tr3 has merged") && text.contains(&m2[..9]) && !text.contains("train/old that"),
        "the base is main's m2, not the re-merge that landed on main: {text}"
    );
    // Local main ahead of origin/main: a merge into main by name has no train, whatever the commits say.
    r.git(&["checkout", "-q", "main"]);
    r.write("m3.txt", "m3\n");
    r.commit("feat: m3, not pushed");
    let (code, text) = r.check(&["--branch", "stale", "--target", "main", "--no-generators"]);
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("old base"), "{text}");
}

#[test]
fn a_merge_into_main_itself_and_a_clone_without_origin_main_are_not_judged_and_the_second_says_so() {
    let (r, _) = train_on_main(false);
    let (code, text) = r.check(&["--branch", "old", "--target", "main", "--no-generators"]);
    assert_eq!(code, 0, "{text}");
    assert!(!text.contains("old base"), "{text}");
    // No origin/main and no re-merge commit: nothing declares the base, and the output says it did not judge.
    r.git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    let (code, text) = check_member(&r, "old");
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("stale-base check did not run"), "{text}");
}

#[test]
fn a_train_and_main_with_no_common_commit_are_not_judged_and_not_an_abort() {
    let (r, _) = train_on_main(false);
    r.git(&["checkout", "-q", "--orphan", "island"]);
    r.git(&["rm", "-rfq", "."]);
    r.write("island.txt", "i\n");
    r.commit("island");
    r.git(&["checkout", "-q", "-b", "island_member"]);
    r.write("member.txt", "m\n");
    r.commit("feat: member");
    r.git(&["checkout", "-q", "island"]);
    r.write("island2.txt", "i2\n");
    r.commit("island again");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge island_member into island", "island_member"]);
    let (code, text) = r.check(&["HEAD", "--no-generators"]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("stale-base check did not run") && text.contains("share no commit"),
        "{text}"
    );
    assert!(!text.contains("NOT MEASURED"), "no abort: {text}");
}

// ---- the chain: sync-member, the merge, then --write-reverted ----

fn written_lines(r: &Repo, args: &[&str]) -> (i32, String, String) {
    let file = r
        .path()
        .join("..")
        .join(format!("w-{}.txt", r.path().file_name().unwrap().to_string_lossy()));
    let mut all = args.to_vec();
    let f = file.to_str().unwrap().to_string();
    all.extend(["--write-reverted", &f]);
    let (code, text) = r.check(&all);
    let written = fs::read_to_string(&file).unwrap_or_default();
    let _ = fs::remove_file(&file);
    (code, text, written)
}

/// main took `feat/shots` (shot.txt: three lines replaced) as a merge and reverted it. With `z_fix` main then fixes the `foot`
/// line of the same file. Returns the repository and the revert S; HEAD is on `feat/shots`.
fn shots(z_fix: bool) -> (Repo, String) {
    let r = Repo::new();
    r.write("shot.txt", "old1\nold2\nold3\nkeep\nfoot\n");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "feat/shots"]);
    r.write("shot.txt", "new1\nnew2\nnew3\nkeep\nfoot\n");
    r.commit("feat: refresh screenshots");
    r.git(&["checkout", "-q", "main"]);
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge branch 'feat/shots'", "feat/shots"]);
    let m = r.git(&["rev-parse", "HEAD"]);
    let s = revert_merge(&r, &m);
    if z_fix {
        r.write("shot.txt", "old1\nold2\nold3\nkeep\nfoot-fixed\n");
        r.commit("fix: foot");
    } else {
        r.write("unrelated.txt", "u\n");
        r.commit("feat: unrelated");
    }
    r.git(&["checkout", "-q", "feat/shots"]);
    (r, s)
}

#[test]
fn the_chain_sync_member_then_the_merge_then_write_reverted_answers_the_train_side() {
    let (r, s) = shots(false);
    let (code, text) = sync_member(&r, "main", &[]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains(&format!("git revert {s}")), "{text}");
    r.git(&["merge", "-q", "--no-edit", "main"]);
    r.git(&["revert", "--no-edit", &s]); // git's default message: Revert "Revert ..."
    assert!(
        fs::read_to_string(r.path().join("shot.txt")).unwrap().starts_with("new1\n"),
        "the work is back"
    );
    r.git(&["checkout", "-q", "main"]);
    let (code, text, written) = written_lines(&r, &["--branch", "feat/shots", "--target", "main", "--no-generators"]);
    assert_eq!(code, 5, "{text}");
    assert!(written.contains("Reverted: shot.txt: "), "{written}\n{text}");
    let msg = r.path().join("..").join("chain-final.txt");
    fs::write(&msg, &written).unwrap();
    r.git(&["merge", "-q", "--no-ff", "-F", msg.to_str().unwrap(), "feat/shots"]);
    let (code, text) = r.check(&["HEAD", "--no-generators"]);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn a_merge_that_lost_a_fix_the_train_made_after_the_revert_gets_no_reverted_line() {
    // The reviewer's case: the train fixed `foot` after the revert (S), the re-land is merged taking the member's whole file, and
    // the fix is lost. The findings must stay, with no `Reverted:` line for the file.
    let (r, s) = shots(true);
    r.git(&["checkout", "-q", "-b", "reland", &s]);
    r.git(&["revert", "--no-edit", &s]);
    r.git(&["checkout", "-q", "main"]);
    r.git(&["merge", "-q", "--no-ff", "--no-commit", "reland"]);
    r.git(&["checkout", "reland", "--", "shot.txt"]);
    r.git(&["commit", "-q", "-m", "Merge branch 'reland'"]);
    assert!(
        !fs::read_to_string(r.path().join("shot.txt")).unwrap().contains("foot-fixed"),
        "setup: the fix is lost"
    );
    let (code, text, written) = written_lines(&r, &["HEAD", "--no-generators"]);
    assert_eq!(code, 5, "{text}");
    assert!(text.contains("shot.txt"), "the finding stays: {text}");
    assert!(
        !written.contains("Reverted: shot.txt"),
        "no line for a file the merge lost a fix in: {written}"
    );
}

// ---- the r3d shape: a one-line fix of the other side dropped by a hand-edited resolution, no re-land ----

/// Base `shot.txt` has six lines; the member changes the first three, the train changes line 6 (`Z-FIX`). The merge is made by hand
/// from the member's file plus one extra line, with `keep_fix` saying whether the train's line stays. Returns the repository (HEAD is
/// the merge).
fn hand_merged(keep_fix: bool) -> Repo {
    let r = Repo::new();
    r.write("shot.txt", "old1\nold2\nold3\nk4\nk5\nk6\n");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "member"]);
    r.write("shot.txt", "new1\nnew2\nnew3\nk4\nk5\nk6\n");
    r.commit("feat: member");
    r.git(&["checkout", "-q", "main"]);
    r.write("shot.txt", "old1\nold2\nold3\nk4\nk5\nZ-FIX\n");
    r.commit("fix: z edits line 6");
    r.git(&["merge", "-q", "--no-ff", "--no-commit", "member"]);
    let last = if keep_fix { "Z-FIX" } else { "k6" };
    r.write("shot.txt", &format!("new1\nnew2\nnew3\nk4\nk5\n{last}\nextra\n"));
    r.git(&["add", "shot.txt"]);
    r.git(&["commit", "-q", "-m", "Merge branch 'member'"]);
    r
}

#[test]
fn a_hand_edited_merge_that_drops_the_other_sides_one_line_fix_is_a_dropped_hunk() {
    let r = hand_merged(false);
    let (code, text) = r.check(&["HEAD", "--no-generators"]);
    assert_eq!(code, 5, "{text}");
    assert!(text.contains("[dropped-hunk] shot.txt") && text.contains("Z-FIX"), "{text}");
    // The answer is the usual one: a reason in the message.
    let r = hand_merged(false);
    r.git(&[
        "commit",
        "-q",
        "--amend",
        "-m",
        "Merge branch 'member'\n\nHunks-dropped: shot.txt: the fix is not wanted",
    ]);
    let (code, text) = r.check(&["HEAD", "--no-generators"]);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn the_same_hand_edit_that_keeps_the_fix_is_clean() {
    // The control (ER-33): identical history, the train's line kept.
    let (code, text) = hand_merged(true).check(&["HEAD", "--no-generators"]);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn a_one_line_change_that_is_merely_absent_and_not_undone_is_not_flagged() {
    // The train replaced k6 by Z-FIX; the resolution has neither k6 nor Z-FIX (a rewritten line): not the small-hunk rule's case.
    let r = hand_merged(true);
    r.write("shot.txt", "new1\nnew2\nnew3\nk4\nk5\nsomething else\nextra\n");
    r.git(&["commit", "-q", "-a", "--amend", "-m", "Merge branch 'member'"]);
    let (code, text) = r.check(&["HEAD", "--no-generators"]);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn the_diff_guard_alone_stops_a_reverted_line_that_hides_a_lost_train_change() {
    // The re-land member (L undoes the revert S) is merged by hand: the member's file plus one extra line, and the train's later
    // change (a single added line after S, which no hunk detector counts) is dropped. The only finding is `[backwards]`; only the
    // guard `git diff S <target> -- path` keeps `--write-reverted` from writing a `Reverted:` line that would hide the loss. The
    // whole-file / dropped-hunk guard is defence in depth: the diff guard covers whenever the member contains S.
    let r = Repo::new();
    r.write("shot.txt", "old1\nold2\nold3\nk4\nk5\nk6\n");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "feat/shots"]);
    r.write("shot.txt", "new1\nnew2\nnew3\nk4\nk5\nk6\n");
    r.commit("feat: refresh screenshots");
    r.git(&["checkout", "-q", "main"]);
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge branch 'feat/shots'", "feat/shots"]);
    let m = r.git(&["rev-parse", "HEAD"]);
    let s = revert_merge(&r, &m);
    r.git(&["checkout", "-q", "-b", "reland", &s]);
    r.git(&["revert", "--no-edit", &s]);
    r.git(&["checkout", "-q", "main"]);
    r.write("shot.txt", "old1\nold2\nold3\nk4\nk5\nk6\nz-added-line\n");
    r.commit("fix: z adds a line after the revert");
    r.git(&["merge", "-q", "--no-ff", "--no-commit", "reland"]);
    r.write("shot.txt", "new1\nnew2\nnew3\nk4\nk5\nk6\nextra\n");
    r.git(&["add", "shot.txt"]);
    r.git(&["commit", "-q", "-m", "Merge branch 'reland'"]);
    let (code, text, written) = written_lines(&r, &["HEAD", "--no-generators"]);
    assert_eq!(code, 5, "{text}");
    assert!(
        text.contains("[backwards] shot.txt") && !text.contains("[dropped-hunk]") && !text.contains("[whole-file]"),
        "the only finding: {text}"
    );
    assert!(
        !written.contains("Reverted: shot.txt"),
        "no line that would hide the lost change: {written}"
    );
}

#[test]
fn sync_member_run_after_the_merge_reads_the_members_pre_merge_tip_and_leaves_out_what_is_done() {
    let (r, s) = reverted_own_branch();
    r.write("x2.txt", "x2\n"); // a commit of the member's own after the train merged it, so the train's merge is a real merge
    r.commit("feat: x2");
    r.git(&["merge", "-q", "--no-edit", "main"]);
    let (code, text) = sync_member(&r, "main", &NO_STATIC);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains(&format!("git revert {s}"))
            && text.contains("The train is already merged")
            && text.contains("1 revert(s) of this member's own merges"),
        "{text}"
    );
    // Run what it printed: nothing is left, and it says that one was already re-applied.
    r.git(&["revert", "--no-edit", &s]);
    let (code, text) = sync_member(&r, "main", &NO_STATIC);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("0 revert(s) of this member's own merges") && text.contains("1 already re-applied") && !text.contains("git revert "),
        "{text}"
    );
}

#[test]
fn the_commit_form_with_a_local_main_ahead_of_origin_does_not_take_the_re_merge_on_that_main_as_the_base() {
    // Local main carries a re-merge of train/old that origin/main lacks; the train is cut from it. In the commit form the target
    // is a sha (no name says it is main), so the walk must stop at the local main too.
    let r = Repo::new();
    r.write("base.txt", "base\n");
    let c0 = r.commit("base");
    r.git(&["checkout", "-q", "-b", "stale"]);
    r.write("stale.txt", "s\n");
    r.commit("feat: built before m1");
    r.git(&["checkout", "-q", "main"]);
    r.write("m1.txt", "m1\n");
    let m1 = r.commit("feat: m1");
    r.git(&["update-ref", "refs/remotes/origin/main", &m1]);
    r.git(&["checkout", "-q", "-b", "train/old", &c0]);
    r.write("o.txt", "o\n");
    let o = r.commit("feat: o");
    r.git(&["checkout", "-q", "main"]);
    r.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        &format!("chore(train): merge train/old ({}) into train 12", &o[..9]),
        &o,
    ]);
    r.git(&["checkout", "-q", "-b", "tr4"]);
    r.write("t.txt", "t\n");
    r.commit("feat: a on the train");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge stale into train", "stale"]);
    let (code, text) = r.check(&["HEAD", "--no-generators"]);
    assert_ne!(code, 0, "the member lacks m1, so the old base blocks: {text}");
    assert!(
        text.contains("the newest commit of main that") && text.contains(&m1[..9]) && !text.contains("train/old"),
        "the base is origin/main's m1, not the re-merge on the local main: {text}"
    );
}

#[test]
fn an_undone_indentation_only_change_is_not_a_dropped_hunk_and_a_real_one_line_fix_still_is() {
    // The train re-indented a line (whitespace only); the member appended a comment to it; the hand-made result keeps the base's
    // indentation. The one-line rule must not call that a lost fix (its excerpt, trimmed, would read like the base line).
    let build = |fixed: &str| {
        let r = Repo::new();
        r.write("f.rs", "fn a() {\n    let v = compute(1);\n    other();\n}\n");
        r.commit("base");
        r.git(&["checkout", "-q", "-b", "member"]);
        r.write("f.rs", "fn a() {\n    let v = compute(1); // m\n    other();\n}\n");
        r.commit("feat: member");
        r.git(&["checkout", "-q", "main"]);
        r.write("f.rs", &format!("fn a() {{\n{fixed}\n    other();\n}}\n"));
        r.commit("fix: main");
        r.git(&["merge", "-q", "-s", "ours", "--no-ff", "--no-commit", "member"]);
        r.write(
            "f.rs",
            "fn a() {\n    let v = compute(1);\n    let v = compute(1); // m\n    other();\n}\n",
        );
        r.git(&["add", "f.rs"]);
        r.git(&["commit", "-q", "-m", "Merge branch 'member'"]);
        r
    };
    // The control (ER-33): the same shape with a real change (not whitespace) is flagged.
    let (code, text) = build("    let v = compute(2);").check(&["HEAD", "--no-generators"]);
    assert_eq!(code, 5, "{text}");
    assert!(text.contains("[dropped-hunk] f.rs"), "{text}");
    // The whitespace-only change is not.
    let (code, text) = build("  let v = compute(1);").check(&["HEAD", "--no-generators"]);
    assert!(!text.contains("[dropped-hunk]"), "{text}");
    let _ = code;
}

/// A registry of two generators that check with `true`: one marked by the lockfile, one by the docs.
const TWO_GENERATORS: &str = r#"version = 1

[[generator]]
id = "lock-gen"
what = "the lockfile"
check = ["true"]
regenerate = "none: invented"
paths = ["crates/Cargo.lock"]
outputs = []
cost = "fast"

[[generator]]
id = "docs-gen"
what = "the docs"
check = ["true"]
regenerate = "none: invented"
paths = ["docs/**"]
outputs = []
cost = "fast"
"#;

/// `dev merge check HEAD` with the generator step on, its build folder outside the repository (as the tool requires).
fn check_with_generators(r: &Repo) -> (i32, String) {
    let builds = tempfile::tempdir().unwrap();
    let out = r
        .dev_cmd()
        .args(["merge", "check", "--root"])
        .arg(r.path())
        .arg("HEAD")
        .env("PADDOCK_DEV_TARGET_DIR", builds.path())
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), text)
}

/// The train moved on (a lockfile change) after the member was cut; the member then changes `member_file` and is merged.
fn member_cut_from_an_old_base(member_file: &str) -> Repo {
    let r = Repo::new();
    r.write("docs/generators.toml", TWO_GENERATORS);
    r.write("crates/Cargo.lock", "1\n");
    r.write("docs/a.md", "a\n");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "member"]);
    r.write(member_file, "member\n");
    r.commit("docs: member");
    r.git(&["checkout", "-q", "main"]);
    r.write("crates/Cargo.lock", "2\n");
    r.commit("chore: the train moved on");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge member", "member"]);
    r
}

#[test]
fn a_member_cut_from_an_old_base_makes_due_only_what_it_changed_from_the_first_parent() {
    let r = member_cut_from_an_old_base("docs/adr.md");
    let (code, text) = check_with_generators(&r);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("gen check: 1 of 2 due: docs-gen"),
        "the lockfile change was the train's, not the member's: {text}"
    );
    assert!(!text.contains("lock-gen"), "{text}");
}

#[test]
fn a_member_that_changes_the_lockfile_makes_its_generator_due() {
    // The positive control (ER-33): the same history with a member that really changes the lockfile.
    let r = Repo::new();
    r.write("docs/generators.toml", TWO_GENERATORS);
    r.write("crates/Cargo.lock", "1\n");
    r.write("docs/a.md", "a\n");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "member"]);
    r.write("crates/Cargo.lock", "2\n");
    r.commit("chore: member lock");
    r.git(&["checkout", "-q", "main"]);
    r.write("other.txt", "b\n");
    r.commit("chore: the train moved on");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge member", "member"]);
    let (code, text) = check_with_generators(&r);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("gen check: 1 of 2 due: lock-gen"), "{text}");
}

// `merge sync-member` runs the fast static rows on the sync tip (the train merged into HEAD). A stand-in for `dev check --pre-push`
// plays them: it logs its argv, prints the rows the test gives it and exits with GATES_EXIT.
#[cfg(unix)]
mod sync_static {
    use super::*;

    fn fake_rows(dir: &Path, rows: &str) {
        let log = Actions::new().appends_with("{env:GATES_LOG}", "{args}\n");
        StandIn::new(dir, "rows")
            .answers(
                log.stdout(&format!(r#"{{"ok":true,"gates":{rows}}}"#))
                    .exits_with_env("GATES_EXIT", 0),
            )
            .install()
            .unwrap();
    }

    const GREEN: &str = r#"[{"id":"conformance","verdict":"passed"},{"id":"gates-register","verdict":"passed"},{"id":"clippy crates","verdict":"passed"},{"id":"clippy-windows crates","verdict":"passed"}]"#;

    /// The member has merged the train: HEAD is the sync tip.
    fn merged() -> Repo {
        let (r, _) = reverted_own_branch();
        r.write("x2.txt", "x2\n");
        r.commit("feat: x2");
        r.git(&["merge", "-q", "--no-edit", "main"]);
        r
    }

    fn run(r: &Repo, rows: &str, exit: &str, extra: &[&str]) -> (i32, String, String) {
        let dir = tempfile::tempdir().unwrap();
        fake_rows(dir.path(), rows);
        let log = dir.path().join("log");
        let out = r
            .dev_cmd()
            .env("PADDOCK_MERGE_CHECK_GATES", dir.path().join("rows"))
            .env("GATES_LOG", &log)
            .env("GATES_EXIT", exit)
            .args([
                "merge",
                "sync-member",
                "--no-trial",
                "fixture: the static rows are under test",
                "--root",
            ])
            .arg(r.path())
            .args(extra)
            .arg("main")
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap(), text, fs::read_to_string(&log).unwrap_or_default())
    }

    #[test]
    fn after_the_merge_it_runs_the_asked_rows_against_the_train_and_prints_each() {
        let r = merged();
        let (code, text, log) = run(&r, GREEN, "0", &[]);
        assert_eq!(code, 0, "{text}");
        for row in ["conformance", "gates-register", "clippy crates", "clippy-windows crates"] {
            assert!(text.contains(row), "{row}: {text}");
        }
        assert!(text.contains("static rows: passed"), "{text}");
        let main = r.git(&["rev-parse", "main"]);
        assert!(
            log.contains("--only conformance,gates-register,clippy,clippy-windows") && log.contains(&format!("--base {main}")),
            "{log}"
        );
    }

    #[test]
    fn a_failing_row_ends_non_zero_with_one_error_and_the_next_step() {
        let r = merged();
        let rows = r#"[{"id":"conformance","verdict":"failed"},{"id":"gates-register","verdict":"passed"}]"#;
        let (code, text, _) = run(&r, rows, "5", &[]);
        assert_eq!(code, 5, "{text}");
        assert_eq!(text.matches("::error::").count(), 1, "{text}");
        assert!(
            text.contains("conformance") && text.contains("fix on the member, then re-sync"),
            "{text}"
        );
    }

    /// Positive control (ER-33): a stand-in that prints no rows (the signal hidden) is not a pass.
    #[test]
    fn rows_that_were_not_printed_are_not_a_pass() {
        let r = merged();
        let (code, text, _) = run(&r, "[]", "0", &[]);
        assert_eq!(code, 6, "{text}");
        assert!(text.contains("NOT MEASURED") && !text.contains("static rows: passed"), "{text}");
    }

    #[test]
    fn no_static_skips_the_rows_and_logs_the_reason() {
        let r = merged();
        let (code, text, log) = run(&r, "[]", "5", &["--no-static", "docs-only sync"]);
        assert_eq!(code, 0, "{text}");
        assert!(text.contains("static rows NOT run (--no-static: docs-only sync)"), "{text}");
        assert_eq!(log, "", "the rows were not started");
        let (code, text, _) = run(&r, GREEN, "0", &["--no-static", " "]);
        assert_eq!(code, 2, "{text}");
        assert!(text.contains("--no-static needs a reason"), "{text}");
    }

    #[test]
    fn before_the_merge_there_is_no_sync_tip_and_the_rows_are_not_started() {
        let (r, _) = reverted_own_branch();
        let (code, text, log) = run(&r, GREEN, "0", &[]);
        assert_eq!(code, 0, "{text}");
        assert!(text.contains("static rows not run") && log.is_empty(), "{text}");
    }
}

// After the static rows pass, `merge sync-member` dispatches the member's own CI trial (`ci dispatch ci.yml --trial ...`), and a later
// `merge check --gates` reads it. Stand-ins play the static rows, the dispatch and the trial reader (`ci trial-verdict`).
#[cfg(unix)]
mod member_trial {
    use super::*;

    const GREEN: &str = r#"[{"id":"conformance","verdict":"passed"},{"id":"gates-register","verdict":"passed"}]"#;
    const RED: &str = r#"[{"id":"conformance","verdict":"failed"},{"id":"gates-register","verdict":"passed"}]"#;

    /// The member (feat/x) has merged the train (main) and has a bare `origin`.
    fn pushed() -> Repo {
        let (r, _) = reverted_own_branch();
        r.write("x2.txt", "x2\n");
        r.commit("feat: x2");
        r.git(&["merge", "-q", "--no-edit", "main"]);
        let bare = r.path().join("..").join(format!("origin-{}.git", std::process::id()));
        r.git(&["init", "-q", "--bare", bare.to_str().unwrap()]);
        r.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
        r.git(&["push", "-q", "origin", "feat/x"]);
        r
    }

    fn stand_ins(dir: &Path, rows: &str) {
        let log = Actions::new().appends_with("{env:GATES_LOG}", "{args}\n");
        StandIn::new(dir, "rows")
            .answers(log.clone().stdout(&format!(r#"{{"ok":true,"gates":{rows}}}"#)))
            .install()
            .unwrap();
        // The dispatch stand-in also logs the trial folder it was handed: `ci dispatch --trial` reads it from its environment.
        let dispatch_log = Actions::new().appends_with("{env:GATES_LOG}", "{args} trial_dir={env:PADDOCK_TRIAL_DIR}\n");
        StandIn::new(dir, "dispatch")
            .answers(dispatch_log.stdout_with("{env:DISPATCH_OUT}").exits_with_env("DISPATCH_EXIT", 0))
            .install()
            .unwrap();
    }

    fn sync(r: &Repo, rows: &str, dispatch_out: &str, dispatch_exit: &str, extra: &[&str]) -> (i32, String, String) {
        let dir = tempfile::tempdir().unwrap();
        sync_in(
            r,
            &dir,
            &[("PADDOCK_TRIAL_DIR", dir.path().join("trial"))],
            rows,
            dispatch_out,
            dispatch_exit,
            extra,
        )
    }

    /// `sync` in a given stand-in folder (its log accumulates) and with or without `PADDOCK_TRIAL_DIR`.
    fn sync_in(
        r: &Repo,
        dir: &tempfile::TempDir,
        envs: &[(&str, PathBuf)],
        rows: &str,
        dispatch_out: &str,
        dispatch_exit: &str,
        extra: &[&str],
    ) -> (i32, String, String) {
        stand_ins(dir.path(), rows);
        let log = dir.path().join("log");
        let mut cmd = r.dev_cmd();
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let out = cmd
            .env("PADDOCK_MERGE_CHECK_GATES", dir.path().join("rows"))
            .env("PADDOCK_SYNC_TRIAL_DISPATCH", dir.path().join("dispatch"))
            .env("GATES_LOG", &log)
            .env("DISPATCH_OUT", dispatch_out)
            .env("DISPATCH_EXIT", dispatch_exit)
            .args(["merge", "sync-member", "--root"])
            .arg(r.path())
            .args(extra)
            .arg("main")
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap(), text, fs::read_to_string(&log).unwrap_or_default())
    }

    #[test]
    fn once_the_static_rows_pass_the_members_own_trial_is_dispatched_and_the_run_id_printed() {
        let r = pushed();
        let (code, text, log) = sync(&r, GREEN, "run 4242", "0", &[]);
        assert_eq!(code, 0, "{text}");
        assert!(text.contains("CI trial dispatched: run 4242"), "{text}");
        assert!(
            log.contains(&format!(
                "ci dispatch ci.yml --trial sync/feat/x/{} --onto main --max 30m",
                short_head(&r)
            )),
            "{log}"
        );
    }

    /// The first nine characters of HEAD: the ref's name (`sync/<member>/<short sha>`).
    fn short_head(r: &Repo) -> String {
        r.git(&["rev-parse", "HEAD"])[..9].to_string()
    }

    /// `ci dispatch --trial` needs `PADDOCK_TRIAL_DIR`: the dispatch child is handed the folder this command was given, and a command that
    /// has none says so before it pushes anything.
    #[test]
    fn the_dispatch_gets_the_trial_dir_and_a_missing_one_is_refused_before_the_push() {
        let r = pushed();
        let (code, text, log) = sync(&r, GREEN, "run 4242", "0", &[]);
        assert_eq!(code, 0, "{text}");
        assert!(log.contains("trial_dir=") && !log.contains("trial_dir=\n"), "{log}");
        assert!(log.lines().any(|l| l.contains("--trial") && l.ends_with("/trial")), "{log}");
        let dir = tempfile::tempdir().unwrap();
        let (code, text, log) = sync_in(&r, &dir, &[], GREEN, "run 4242", "0", &[]);
        assert_eq!(code, 6, "{text}");
        assert!(text.contains("cannot be resolved") && !log.contains("ci dispatch"), "{text}\n{log}");
        let origin = r.git(&["remote", "get-url", "origin"]);
        let refs = r.git(&["--git-dir", &origin, "for-each-ref", "refs/heads/sync/"]);
        assert!(refs.lines().count() == 1, "only the first sync pushed a ref: {refs}");
    }

    /// With no `PADDOCK_TRIAL_DIR`, the one resolver's default is handed to the dispatch child: `<dev-targets>/trials`.
    #[test]
    fn without_the_variable_the_trial_dir_defaults_below_the_dev_targets_folder() {
        let r = pushed();
        let dir = tempfile::tempdir().unwrap();
        let dt = dir.path().join("dt");
        let (code, text, log) = sync_in(&r, &dir, &[("PADDOCK_DEV_TARGETS", dt.clone())], GREEN, "run 4242", "0", &[]);
        assert_eq!(code, 0, "{text}");
        assert!(log.contains(&format!("trial_dir={}", dt.join("trials").display())), "{log}");
    }

    /// A re-sync is a new commit and pushes a second, new ref: a plain push, no force and no lease, and the first ref is untouched.
    #[test]
    fn a_re_sync_pushes_a_second_new_ref_with_no_force() {
        let r = pushed();
        let first = r.git(&["rev-parse", "HEAD"]);
        let (code, text, _) = sync(&r, GREEN, "run 4242", "0", &[]);
        assert_eq!(code, 0, "{text}");
        r.write("x3.txt", "x3\n");
        r.commit("feat: x3");
        let second = r.git(&["rev-parse", "HEAD"]);
        let (code, text, log) = sync(&r, GREEN, "run 4243", "0", &[]);
        assert_eq!(code, 0, "{text}");
        assert!(log.contains(&format!("--trial sync/feat/x/{} ", &second[..9])), "{log}");
        let origin = r.git(&["remote", "get-url", "origin"]);
        let on = |sha: &str| r.git(&["--git-dir", &origin, "rev-parse", &format!("refs/heads/sync/feat/x/{}", &sha[..9])]);
        assert_eq!((on(&first), on(&second)), (first.clone(), second.clone()));
        // The same commit again is the same ref: nothing to replace, so nothing to force.
        let (code, text, _) = sync(&r, GREEN, "run 4244", "0", &[]);
        assert_eq!(code, 0, "{text}");
    }

    /// An https origin needs the caller's token: without GITEA_TOKEN the push is not attempted and the message names it.
    #[test]
    fn an_https_origin_without_a_token_is_not_pushed_and_names_the_token() {
        let r = pushed();
        r.git(&["remote", "set-url", "origin", "https://127.0.0.1:9/decryp7/paddock.git"]);
        let (code, text, log) = sync(&r, GREEN, "run 4242", "0", &[]);
        assert_eq!(code, 6, "{text}");
        assert!(
            text.contains("GITEA_TOKEN is not set") && !log.contains("ci dispatch"),
            "{text}\n{log}"
        );
    }

    #[test]
    fn a_failing_static_row_dispatches_no_trial() {
        let r = pushed();
        let (code, text, log) = sync(&r, RED, "run 4242", "0", &[]);
        assert_eq!(code, 5, "{text}");
        assert!(!log.contains("ci dispatch") && !text.contains("trial dispatched"), "{text}\n{log}");
    }

    #[test]
    fn the_sync_tip_is_pushed_as_a_new_ref_before_the_dispatch_and_a_failed_push_dispatches_nothing() {
        let r = pushed();
        let (code, _, _) = sync(&r, GREEN, "run 4242", "0", &[]);
        assert_eq!(code, 0);
        let name = format!("refs/heads/sync/feat/x/{}", short_head(&r));
        let on_origin = r.git(&["ls-remote", "origin", &name]);
        assert!(on_origin.starts_with(&r.git(&["rev-parse", "HEAD"])), "{on_origin}");
        // The name holds another commit (a short-sha clash): the plain push is refused, nothing is overwritten and nothing is dispatched.
        let bare = r.git(&["remote", "get-url", "origin"]);
        let moved = r.git(&["commit-tree", "HEAD^{tree}", "-m", "another commit under the same name"]);
        r.git(&["push", "-q", "origin", &format!("{moved}:refs/heads/elsewhere")]);
        r.git(&["--git-dir", &bare, "update-ref", &name, &moved]);
        let (code, text, log) = sync(&r, GREEN, "run 4242", "0", &[]);
        assert_eq!(code, 6, "{text}");
        assert!(
            text.contains("could not be pushed") && !log.contains("ci dispatch"),
            "{text}\n{log}"
        );
        assert!(
            r.git(&["--git-dir", &bare, "rev-parse", &name]) == moved,
            "the moved ref was overwritten"
        );
        r.git(&["remote", "set-url", "origin", "/nonexistent/origin.git"]);
        let (code, text, log) = sync(&r, GREEN, "run 4242", "0", &[]);
        assert_eq!(code, 6, "{text}");
        assert!(
            text.contains("could not be pushed") && !log.contains("ci dispatch"),
            "{text}\n{log}"
        );
    }

    /// Positive control (ER-33): a dispatch that exits 0 without a run line, and one that fails, are not a started trial.
    #[test]
    fn a_dispatch_that_gave_no_run_or_failed_is_not_a_started_trial() {
        let r = pushed();
        for (out, exit) in [("guards ok", "0"), ("::error::the runner is busy", "5")] {
            let (code, text, _) = sync(&r, GREEN, out, exit, &[]);
            assert_eq!(code, 6, "{text}");
            assert!(text.contains("NOT DISPATCHED") && !text.contains("trial dispatched"), "{text}");
        }
    }

    #[test]
    fn no_trial_skips_the_dispatch_and_logs_the_reason() {
        let r = pushed();
        let (code, text, log) = sync(&r, GREEN, "run 4242", "0", &["--no-trial", "docs-only member"]);
        assert_eq!(code, 0, "{text}");
        assert!(
            text.contains("no CI trial dispatched (--no-trial: docs-only member)") && !log.contains("ci dispatch"),
            "{text}\n{log}"
        );
        let (code, text, _) = sync(&r, GREEN, "run 4242", "0", &["--no-trial", " "]);
        assert_eq!(code, 2, "{text}");
    }

    // ---- `merge check --gates` reads the trial ----

    /// A train with member `a` merged under git's own subject, so the member's branch name is known; the gates stand-in passes.
    fn merged_member() -> (Repo, tempfile::TempDir) {
        let repo = Repo::new();
        repo.write("base.txt", "base\n");
        repo.commit("base");
        repo.git(&["checkout", "-q", "-b", "feat/a", "main"]);
        repo.write("a.txt", "a\n");
        repo.commit("feat: a");
        repo.git(&["checkout", "-q", "main"]);
        repo.git(&["merge", "-q", "--no-ff", "-m", "Merge branch 'feat/a'", "feat/a"]);
        let dir = tempfile::tempdir().unwrap();
        let ev = r#"{{"check":"pre-push","subject":{{"kind":"commit","sha":"{run:git|rev-parse|HEAD}","tree":"{run:git|rev-parse|HEAD:}"}},"tool":"fake","at":1}}"#;
        StandIn::new(dir.path(), "gates")
            .answers(Actions::new().stdout_with(&format!(r#"{{{{"outcome":{{{{"status":"clean","evidence":{ev}}}}}}}}}"#)))
            .install()
            .unwrap();
        StandIn::new(dir.path(), "trial")
            .answers(
                Actions::new()
                    .appends_with("{env:GATES_LOG}", "{args}\n")
                    .stdout_with(r#"{{"ok":true,"state":"{env:TRIAL_STATE}","run":77}}"#)
                    .exits_with_env("TRIAL_EXIT", 0),
            )
            .install()
            .unwrap();
        (repo, dir)
    }

    fn check(r: &Repo, dir: &tempfile::TempDir, state: &str, exit: &str, extra: &[&str]) -> (i32, String, String) {
        let log = dir.path().join("log");
        let out = r
            .dev_cmd()
            .env("PADDOCK_MERGE_CHECK_GATES", dir.path().join("gates"))
            .env("PADDOCK_MERGE_CHECK_TRIAL", dir.path().join("trial"))
            .env("PADDOCK_MERGE_CHECK_CARGO", "/nonexistent/cargo")
            .env("GATES_LOG", &log)
            .env("TRIAL_STATE", state)
            .env("TRIAL_EXIT", exit)
            .args(["merge", "check", "--gates", "--root"])
            .arg(r.path())
            .args(extra)
            .arg("HEAD")
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap(), text, fs::read_to_string(&log).unwrap_or_default())
    }

    #[test]
    fn a_green_trial_passes_and_the_reader_is_asked_about_the_members_branch_and_head() {
        let (r, dir) = merged_member();
        let (code, text, log) = check(&r, &dir, "green", "0", &[]);
        assert_eq!(code, 0, "{text}");
        let head = r.git(&["rev-parse", "feat/a"]);
        let synced = format!("sync/feat/a/{}", &head[..9]);
        assert!(text.contains(&format!("run 77 of {synced} is green")), "{text}");
        assert!(
            log.contains(&format!("ci trial-verdict --branch {synced} --member-sha")) && log.contains(&head),
            "{log}"
        );
    }

    /// A member merged as its own sync ref already carries the trial's name: it is not prefixed twice.
    #[test]
    fn a_member_merged_as_its_sync_ref_is_read_under_that_name() {
        let (r, dir) = merged_member();
        let head = r.git(&["rev-parse", "feat/a"]);
        let synced = format!("sync/feat/a/{}", &head[..9]);
        r.git(&["branch", &synced, "feat/a"]);
        r.git(&["reset", "-q", "--hard", "main~1"]);
        r.git(&["merge", "-q", "--no-ff", "-m", &format!("Merge branch '{synced}'"), &synced]);
        let (code, text, log) = check(&r, &dir, "green", "0", &[]);
        assert_eq!(code, 0, "{text}");
        assert!(log.contains(&format!("--branch {synced} ")) && !log.contains("sync/sync"), "{log}");
    }

    #[test]
    fn a_red_trial_blocks_the_member_with_one_error() {
        let (r, dir) = merged_member();
        let (code, text, _) = check(&r, &dir, "red", "5", &[]);
        assert_eq!(code, 5, "{text}");
        assert_eq!(
            text.matches("is red. Next step: fix on the member, then re-sync").count(),
            1,
            "{text}"
        );
    }

    /// Positive control (ER-33): a running trial prints "still running" and is not a pass.
    #[test]
    fn a_running_trial_is_still_running_and_not_a_pass() {
        let (r, dir) = merged_member();
        let (code, text, _) = check(&r, &dir, "running", "6", &[]);
        assert_eq!(code, 6, "{text}");
        assert!(text.contains("still running") && text.contains("NOT a pass"), "{text}");
    }

    #[test]
    fn no_trial_skips_the_read_and_logs_the_reason() {
        let (r, dir) = merged_member();
        let (code, text, log) = check(&r, &dir, "red", "5", &["--no-trial", "flaky job, rerun by hand"]);
        assert_eq!(code, 0, "{text}");
        assert!(
            text.contains("--no-trial: flaky job, rerun by hand") && log.is_empty(),
            "{text}\n{log}"
        );
        let (code, text, _) = check(&r, &dir, "green", "0", &["--no-trial", " "]);
        assert_eq!(code, 2, "{text}");
    }
}

/// A fast generator marked by the docs and a heavy one marked by the lockfile; the heavy one's check always fails, so a run of it
/// shows (a STALE finding) and a skipped one shows (exit 0): the stand-in for a build that costs minutes (ER-33: the control sees it).
const FAST_AND_HEAVY: &str = r#"version = 1

[[generator]]
id = "docs-gen"
what = "the docs"
check = ["true"]
regenerate = "none: invented"
paths = ["docs/**"]
outputs = []
cost = "fast"

[[generator]]
id = "heavy-gen"
what = "a workspace build"
check = ["false"]
regenerate = "none: invented"
paths = ["crates/Cargo.lock"]
outputs = []
cost = "heavy"
"#;

/// A member that changes `member_file`, merged into a train that moved on; the registry is [`FAST_AND_HEAVY`].
fn member_changing(member_file: &str) -> Repo {
    let r = Repo::new();
    r.write("docs/generators.toml", FAST_AND_HEAVY);
    r.write("crates/Cargo.lock", "1\n");
    r.write("docs/a.md", "a\n");
    r.commit("base");
    r.git(&["checkout", "-q", "-b", "member"]);
    r.write(member_file, "member\n");
    r.commit("chore: member");
    r.git(&["checkout", "-q", "main"]);
    r.write("other.txt", "b\n");
    r.commit("chore: the train moved on");
    r.git(&["merge", "-q", "--no-ff", "-m", "Merge member", "member"]);
    r
}

fn check_with_extra(r: &Repo, extra: &[&str]) -> (i32, String) {
    let builds = tempfile::tempdir().unwrap();
    let out = r
        .dev_cmd()
        .args(["merge", "check", "--root"])
        .arg(r.path())
        .arg("HEAD")
        .args(extra)
        .env("PADDOCK_DEV_TARGET_DIR", builds.path())
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), text)
}

#[test]
fn a_merge_that_changes_only_a_heavy_generators_input_starts_no_generator_and_names_it_deferred() {
    let r = member_changing("crates/Cargo.lock");
    let (code, text) = check_with_extra(&r, &[]);
    assert_eq!(code, 0, "the heavy check (which always fails) must not have run: {text}");
    assert!(
        text.contains("deferred") && text.contains("heavy-gen") && text.contains("gen check --base origin/main"),
        "a skipped check says so and names the one that runs it, never silence (ER-33): {text}"
    );
    assert!(!text.contains("gen heavy-gen: running"), "{text}");
}

#[test]
fn the_flag_runs_the_heavy_generators_a_merge_made_due() {
    // The positive control: the same merge, with the heavy step asked for, starts the heavy check.
    let r = member_changing("crates/Cargo.lock");
    let (code, text) = check_with_extra(&r, &["--heavy-generators"]);
    // In this bare repository the heavy slot cannot be made (NOT MEASURED, exit 6); either way the check was started and is not a pass.
    assert!(matches!(code, 5 | 6), "the heavy check must have been started: {code} {text}");
    assert!(text.contains("gen heavy-gen") && !text.contains("deferred"), "{text}");
}

#[test]
fn a_fast_generator_still_runs_in_every_merge_check() {
    let r = member_changing("docs/adr.md");
    let (code, text) = check_with_extra(&r, &[]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("gen check: 1 of 2 due: docs-gen"), "{text}");
    assert!(
        text.contains("gen docs-gen: running") || text.contains("gen docs-gen: current"),
        "{text}"
    );
    assert!(!text.contains("deferred"), "nothing heavy was due: {text}");
}

/// `./dev merge member` (user decision 100, 2026-11-27): the train merge is made by the tool, with a fixed subject and trailers, and
/// every reader reads the trailers first and the subject only as the fallback for a merge made by hand.
mod merge_member {
    use super::*;

    /// `(exit code, output)` of `dev merge member <args>` in `r`.
    fn member(r: &Repo, args: &[&str]) -> (i32, String) {
        let out = r
            .dev_cmd()
            .args(["merge", "member", "--root"])
            .arg(r.path())
            .args(args)
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap(), text)
    }

    /// git's own parse of the trailers of `rev`.
    fn trailers(r: &Repo, rev: &str) -> String {
        r.git(&["show", "-s", "--format=%(trailers:only,unfold)", rev])
    }

    /// `main` is the train; `feat/x` is a member that adds `x.txt`.
    fn train_and_member() -> Repo {
        let r = Repo::new();
        r.write("base.txt", "base\n");
        r.commit("base");
        r.git(&["checkout", "-q", "-b", "feat/x"]);
        r.write("x.txt", "x\n");
        r.commit("feat: x");
        r.git(&["checkout", "-q", "main"]);
        r.write("train.txt", "train\n");
        r.commit("train: moves on");
        r
    }

    #[test]
    fn a_member_is_merged_with_a_fixed_subject_and_the_trailers_and_the_check_passes() {
        let r = train_and_member();
        let sha = r.git(&["rev-parse", "feat/x"]);
        let (code, text) = member(&r, &["feat/x"]);
        assert_eq!(code, 0, "{text}");
        assert_eq!(r.git(&["show", "-s", "--format=%s", "HEAD"]), "Merge feat/x into main");
        let t = trailers(&r, "HEAD");
        assert!(
            t.contains("Paddock-Member: feat/x") && t.contains(&format!("Paddock-Sync-Sha: {sha}")),
            "{t}"
        );
        assert!(!t.contains("Paddock-Sync-Ref"), "a branch is no sync ref: {t}");
        assert_eq!(
            r.git(&["rev-list", "--parents", "-n", "1", "HEAD"]).split_whitespace().count(),
            3,
            "a real merge"
        );
        assert!(text.contains("merge check"), "the check ran on the new commit: {text}");
    }

    #[test]
    fn a_sync_ref_is_merged_under_its_member_and_names_itself() {
        let r = train_and_member();
        let sha = r.git(&["rev-parse", "feat/x"]);
        let synced = format!("sync/feat/x/{}", &sha[..9]);
        r.git(&["branch", &synced, "feat/x"]);
        let (code, text) = member(&r, &[&synced]);
        assert_eq!(code, 0, "{text}");
        assert_eq!(r.git(&["show", "-s", "--format=%s", "HEAD"]), "Merge feat/x into main");
        let t = trailers(&r, "HEAD");
        assert!(
            t.contains("Paddock-Member: feat/x")
                && t.contains(&format!("Paddock-Sync-Ref: {synced}"))
                && t.contains(&format!("Paddock-Sync-Sha: {sha}")),
            "{t}"
        );
    }

    /// The control: it is the trailer that names the member, so a subject that names nothing still finds it.
    #[test]
    fn sync_member_reads_the_member_from_the_trailer_not_the_subject() {
        let make = |trailer: &str| {
            let r = Repo::new();
            r.write("base.txt", "base\n");
            r.commit("base");
            r.git(&["checkout", "-q", "-b", "feat/x"]);
            r.write("x.txt", "x\n");
            r.commit("feat: x");
            r.git(&["checkout", "-q", "main"]);
            r.git(&[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                &format!("chore: take the member{trailer}"),
                "feat/x",
            ]);
            let m = r.git(&["rev-parse", "HEAD"]);
            revert_merge(&r, &m);
            r.write("other.txt", "other\n");
            r.commit("feat: other");
            r.git(&["checkout", "-q", "feat/x"]);
            r
        };
        let (code, text) = sync_member(&make("\n\nPaddock-Member: feat/x\n"), "main", &[]);
        assert_eq!(code, 0, "{text}");
        assert!(
            text.contains("1 revert(s) of this member's own merges") && text.contains("0 of other branches"),
            "{text}"
        );
        let (code, text) = sync_member(&make(""), "main", &[]);
        assert_eq!(code, 0, "{text}");
        assert!(
            text.contains("0 revert(s) of this member's own merges"),
            "control: no trailer, no own merge: {text}"
        );
    }

    /// The trial reader: the trailer names the member and its sync ref where the subject names neither.
    #[cfg(unix)]
    #[test]
    fn the_trial_reader_reads_the_member_and_sync_ref_from_the_trailers() {
        let make = |trailers: &dyn Fn(&str) -> String| {
            let repo = Repo::new();
            repo.write("base.txt", "base\n");
            repo.commit("base");
            repo.git(&["checkout", "-q", "-b", "feat/a", "main"]);
            repo.write("a.txt", "a\n");
            let head = repo.commit("feat: a");
            repo.git(&["checkout", "-q", "main"]);
            repo.git(&[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                &format!("chore: take the member{}", trailers(&head)),
                "feat/a",
            ]);
            let dir = tempfile::tempdir().unwrap();
            let ev = r#"{{"check":"pre-push","subject":{{"kind":"commit","sha":"{run:git|rev-parse|HEAD}","tree":"{run:git|rev-parse|HEAD:}"}},"tool":"fake","at":1}}"#;
            StandIn::new(dir.path(), "gates")
                .answers(Actions::new().stdout_with(&format!(r#"{{{{"outcome":{{{{"status":"clean","evidence":{ev}}}}}}}}}"#)))
                .install()
                .unwrap();
            StandIn::new(dir.path(), "trial")
                .answers(
                    Actions::new()
                        .appends_with("{env:GATES_LOG}", "{args}\n")
                        .stdout_with(r#"{{"ok":true,"state":"green","run":77}}"#),
                )
                .install()
                .unwrap();
            let log = dir.path().join("log");
            let out = repo
                .dev_cmd()
                .env("PADDOCK_MERGE_CHECK_GATES", dir.path().join("gates"))
                .env("PADDOCK_MERGE_CHECK_TRIAL", dir.path().join("trial"))
                .env("PADDOCK_MERGE_CHECK_CARGO", "/nonexistent/cargo")
                .env("GATES_LOG", &log)
                .args(["merge", "check", "--gates", "--root"])
                .arg(repo.path())
                .arg("HEAD")
                .output()
                .unwrap();
            let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            (head, text, fs::read_to_string(&log).unwrap_or_default())
        };
        // Paddock-Member alone: the ref is built from the member's head.
        let (head, text, log) = make(&|_| "\n\nPaddock-Member: feat/a\n".to_string());
        assert!(!text.contains("names no member branch"), "{text}");
        assert!(log.contains(&format!("--branch sync/feat/a/{} ", &head[..9])), "{log}\n{text}");
        // Paddock-Sync-Ref: used as it is.
        let (head, text, log) = make(&|h| {
            format!(
                "\n\nPaddock-Member: feat/a\nPaddock-Sync-Ref: sync/feat/a/{}\nPaddock-Sync-Sha: {h}\n",
                &h[..9]
            )
        });
        assert!(log.contains(&format!("--branch sync/feat/a/{} ", &head[..9])), "{log}\n{text}");
        // The control: no trailer and a subject that names nothing is NOT read, and says so.
        let (_, text, log) = make(&|_| String::new());
        assert!(text.contains("names no member branch") && log.is_empty(), "{text}\n{log}");
    }

    /// A conflict stops the command with the merge in progress, the message prepared, and the next step printed; nothing is resolved.
    #[test]
    fn a_conflict_leaves_the_merge_in_progress_with_the_message_prepared() {
        let r = Repo::new();
        r.write("f.txt", "base\n");
        r.commit("base");
        r.git(&["checkout", "-q", "-b", "feat/x"]);
        r.write("f.txt", "member\n");
        r.commit("feat: x");
        r.git(&["checkout", "-q", "main"]);
        r.write("f.txt", "train\n");
        let before = r.commit("train: f");
        let (code, text) = member(&r, &["feat/x"]);
        assert_ne!(code, 0, "{text}");
        assert_eq!(r.git(&["rev-parse", "HEAD"]), before, "no commit was made");
        let merge_head = r.git(&["rev-parse", "--git-path", "MERGE_HEAD"]);
        assert!(r.path().join(&merge_head).exists(), "the merge is in progress: {text}");
        let msg = fs::read_to_string(r.path().join(r.git(&["rev-parse", "--git-path", "MERGE_MSG"]))).unwrap();
        assert!(
            msg.starts_with("Merge feat/x into main") && msg.contains("Paddock-Member: feat/x"),
            "{msg}"
        );
        assert!(
            text.contains("::error::") && text.contains("git commit") && text.contains("merge check"),
            "{text}"
        );
        assert!(
            fs::read_to_string(r.path().join("f.txt")).unwrap().contains("<<<<<<<"),
            "left for a person"
        );
    }

    /// `--reland <revert>`: the revert is named in a trailer and its files get the `Reverted:` lines the merge check wants.
    #[test]
    fn a_reland_is_named_in_a_trailer_with_its_reverted_lines() {
        let r = Repo::new();
        r.write("base.txt", "base\n");
        r.commit("base");
        r.git(&["checkout", "-q", "-b", "feat/r"]);
        r.write("r.txt", "r\n");
        r.commit("feat: r");
        r.git(&["checkout", "-q", "main"]);
        r.write("old.txt", "old\n");
        let old = r.commit("feat: old");
        r.git(&["revert", "--no-edit", &old]);
        let revert = r.git(&["rev-parse", "HEAD"]);
        let (code, text) = member(&r, &["feat/r", "--reland", &revert]);
        assert_eq!(code, 0, "{text}");
        let t = trailers(&r, "HEAD");
        assert!(
            t.contains(&format!("Paddock-Reland: {revert}")) && t.contains("Reverted: old.txt: "),
            "{t}"
        );
        assert!(t.contains("Paddock-Member: feat/r"), "one trailer block: {t}");
        let (code, text) = member(&r, &["feat/r", "--reland", "not-a-commit"]);
        assert_eq!(code, 2, "{text}");
    }

    #[test]
    fn a_dirty_tree_or_an_unknown_member_is_refused_before_anything_is_merged() {
        let r = train_and_member();
        let head = r.git(&["rev-parse", "HEAD"]);
        let (code, text) = member(&r, &["feat/none"]);
        assert_ne!(code, 0, "{text}");
        assert!(text.contains("::error::") && text.contains("feat/none"), "{text}");
        r.write("base.txt", "dirty\n");
        let (code, text) = member(&r, &["feat/x"]);
        assert_ne!(code, 0, "{text}");
        assert!(text.contains("::error::") && text.contains("uncommitted"), "{text}");
        assert_eq!(r.git(&["rev-parse", "HEAD"]), head);
    }
}
