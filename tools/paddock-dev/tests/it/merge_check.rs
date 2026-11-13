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
    // Main touches the file too, so that both sides' versions of it matter and it is read.
    r.write("crates/x/src/lib.rs", "// a main note\n#[test]\nfn ok() {}\n");
    r.commit("fix: main note");
    // The result is not git's own merge (it drops main's note), so the file is read.
    r.git(&["merge", "-q", "-s", "ours", "--no-commit", "feature"]);
    r.write("crates/x/src/lib.rs", "#[test]\nfn renamed( {\n");
    r.git(&["add", "--all"]);
    r.git(&["commit", "-q", "-m", "Merge feature"]);
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

#[test]
fn generated_files_are_skipped_by_every_detector() {
    // Hunks main added to regenerated files are not "dropped" when the merge regenerates them.
    for file in [
        "Cargo.lock",
        "THIRD_PARTY_NOTICES.md",
        "docs/x.drawio",
        "crates/x/widgets/generated.rs",
        "gen/out.gen.txt",
    ] {
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
    assert_eq!(code, 1, "{out}");
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
    assert_eq!(code, 1, "{out}");
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
    assert_eq!(code, 1, "{out}");
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
    use std::os::unix::fs::PermissionsExt;

    const FAKE_CARGO: &str = r#"#!/bin/sh
# argv: hakari generate --diff, run in <tree>/crates. Like cargo-hakari 0.9.39, it prints the diff on STANDARD ERROR.
[ "$1 $2 $3" = "hakari generate --diff" ] || { echo "unexpected: $*" >&2; exit 2; }
if grep -q base64 x/Cargo.toml && ! grep -q base64 paddock-workspace-hack/Cargo.toml; then
  printf 'info: \n--- original\n+++ modified\n+base64 = { version = "0.23" }\n' >&2
  exit 1
fi
exit 0
"#;

    fn fake_cargo(dir: &Path) -> PathBuf {
        let p = dir.join("cargo");
        fs::write(&p, FAKE_CARGO).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
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
            .cmd(env!("CARGO_BIN_EXE_paddock-dev"))
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
            .cmd(env!("CARGO_BIN_EXE_paddock-dev"))
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
            .cmd(env!("CARGO_BIN_EXE_paddock-dev"))
            .env("PADDOCK_MERGE_CHECK_CARGO", "/nonexistent/cargo")
            .args(["merge", "check", "--root"])
            .arg(r.path())
            .arg("HEAD")
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert_ne!(out.status.code(), Some(0), "{text}");
        assert!(text.contains("NOT MEASURED"), "{text}");
        assert_eq!(text.lines().filter(|l| l.contains("::error::")).count(), 1, "{text}");
    }
}

// ADR 0098 WP-1: the member gate (`merge check --gates`). A stand-in for `dev check --pre-push` plays the fast gates: it fails
// (exit 5) when the checkout it is run in holds both a.txt and b.txt, the shape of a pair of members that only fail together
// (train 8: a lint list of one branch meeting new code of another).
#[cfg(unix)]
mod member_gate {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const FAKE_GATES: &str = r#"#!/bin/sh
# argv: check --pre-push --base <base> --root <root>; run in <root>. Logs its argv and the checkout's HEAD.
echo "$* HEAD=$(git rev-parse HEAD)" >> "$GATES_LOG"
if [ -f a.txt ] && [ -f b.txt ]; then
  echo "clippy FAILED: a.txt and b.txt together"
  exit "${GATES_EXIT:-5}"
fi
echo "fmt ok; clippy ok"
exit 0
"#;

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
                .cmd(env!("CARGO_BIN_EXE_paddock-dev"))
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
        let p = gates.path().join("gates");
        fs::write(&p, FAKE_GATES).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        Train { repo, gates }
    }

    #[test]
    fn a_member_that_passes_the_gates_on_the_combined_tree_is_reported_passed() {
        let t = train(false);
        let (code, out) = t.run(&["--gates", "HEAD"], "5");
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("member gate: passed"), "{out}");
        assert!(!out.contains("FAILED"), "{out}");
    }

    #[test]
    fn two_members_that_only_fail_together_are_named_at_the_merge_that_joins_them_and_stay_report_only() {
        let t = train(true);
        let (code, out) = t.run(&["--gates", "HEAD"], "5");
        // Report-only until the first green run (ER-41, ADR 0098 D3 (a)): the rows and the warning are printed, the exit is not.
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("clippy FAILED: a.txt and b.txt together"), "{out}");
        assert!(out.contains("::warning::member gate FAILED"), "{out}");
        assert!(out.contains("REPORT-ONLY"), "{out}");
        assert!(out.contains("take the member out") && out.contains("send it back"), "{out}");
    }

    #[test]
    fn the_gates_run_on_the_merge_with_the_train_before_it_as_the_base() {
        let t = train(true);
        let first_parent = t.repo.git(&["rev-parse", "HEAD^1"]);
        let head = t.repo.git(&["rev-parse", "HEAD"]);
        t.run(&["--gates", "HEAD"], "5");
        let log = fs::read_to_string(t.log()).unwrap();
        assert!(
            log.contains(&format!("check --pre-push --base {first_parent} --root")) && log.contains(&format!("HEAD={head}")),
            "{log}"
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
        assert_eq!(code, 0, "report-only: {out}");
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
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("member gate NOT MEASURED"), "{out}");
        assert!(!out.contains("member gate: passed"), "{out}");
    }

    #[test]
    fn json_carries_the_gate_result_under_its_own_key() {
        let t = train(true);
        let (code, out) = t.run(&["--gates", "HEAD", "--json"], "5");
        assert_eq!(code, 0, "{out}");
        let v: serde_json::Value = serde_json::from_str(out.lines().find(|l| l.starts_with('{')).unwrap()).unwrap();
        assert_eq!(v["gates"]["outcome"], "failed", "{out}");
        assert_eq!(v["gates"]["enforced"], false, "{out}");
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
