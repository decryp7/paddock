//! The overlap warning of `worktree add`: two agents on the same files lose work when their branches meet (the 2026-11-25 pulls and
//! the double landings). Before it makes a worktree, `add` reads every other locked worktree and prints a WARNING (never a
//! refusal) naming it, its owner (from the lock note) and the files both sides touch, or the topic its branch name shares.
//!
//! A locked worktree's files are its uncommitted ones (`git status --porcelain`) and its unlanded ones (commits not on
//! `origin/main`, `git diff --name-only origin/main...HEAD`). The new work's files are those its base carries that main lacks
//! (`origin/main...<base>`): a base that is main itself carries none, and then only a shared topic stem warns. The stem of
//! `feat/worktree-overlap-warning` is its first two words, `worktree-overlap`; two branches with one stem are probably one job.
//! The pure part (`overlaps`, `render`) takes the facts; `gather` reads them through an injected git runner so a test counts calls.

use super::git::WorktreeInfo;
use super::note::Note;
use std::collections::BTreeSet;
use std::path::Path;

/// The branch every unlanded change is measured from.
pub const MAIN: &str = "origin/main";
/// The most files a warning names before it counts the rest.
const SHOWN: usize = 10;

/// One other locked worktree and what it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Other {
    /// Its folder.
    pub path: String,
    /// `<role>:<task>` from its lock note, or `unknown owner` for a lock with other text.
    pub owner: String,
    /// Its branch, empty when detached.
    pub branch: String,
    /// Uncommitted and unlanded files.
    pub files: BTreeSet<String>,
}

/// Why one other worktree warns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlap {
    /// The other worktree.
    pub other: Other,
    /// The files both sides touch.
    pub shared: Vec<String>,
    /// Its branch has the topic stem of the new one.
    pub same_topic: bool,
}

/// The topic of a branch name: the words after the type prefix (`feat/`), the first two of them. `None` for a name with no topic.
pub fn topic_stem(branch: &str) -> Option<String> {
    let topic = branch.split_once('/').map_or(branch, |(_, t)| t);
    let words: Vec<&str> = topic.split('-').filter(|w| !w.is_empty()).take(2).collect();
    (!words.is_empty()).then(|| words.join("-"))
}

/// The other worktrees that overlap the new one: a shared file or a shared topic stem. Sorted by path.
pub fn overlaps(new_branch: Option<&str>, new_files: &BTreeSet<String>, others: &[Other]) -> Vec<Overlap> {
    let stem = new_branch.and_then(topic_stem);
    let mut out: Vec<Overlap> = others
        .iter()
        .filter_map(|o| {
            let shared: Vec<String> = o.files.intersection(new_files).cloned().collect();
            let same_topic = stem.is_some() && stem == topic_stem(&o.branch);
            (!shared.is_empty() || same_topic).then(|| Overlap {
                other: o.clone(),
                shared,
                same_topic,
            })
        })
        .collect();
    out.sort_by(|a, b| a.other.path.cmp(&b.other.path));
    out
}

/// The text of the warning; empty when nothing overlaps.
pub fn render(found: &[Overlap]) -> String {
    let mut text = String::new();
    for o in found {
        let mut why = Vec::new();
        if !o.shared.is_empty() {
            let mut names = o.shared.iter().take(SHOWN).cloned().collect::<Vec<_>>().join(", ");
            if o.shared.len() > SHOWN {
                names.push_str(&format!(" and {} more", o.shared.len() - SHOWN));
            }
            why.push(format!("it changes files this base carries too: {names}"));
        }
        if o.same_topic {
            why.push(format!("its branch `{}` has the same topic as this one", o.other.branch));
        }
        text.push_str(&format!(
            "\n::warning::worktree add: {} (owner {}) may overlap this work: {}. Next step: ask that owner before you start, or pick another task; nothing was refused.",
            o.other.path,
            o.other.owner,
            why.join("; ")
        ));
    }
    text
}

/// A runner of git in a folder: its output, or why it failed.
pub type RunGit<'a> = &'a dyn Fn(&Path, &[&str]) -> Result<String, String>;

/// The `-z` entries of `git status --porcelain -z`: the path of each, the old name of a rename or copy skipped.
fn status_paths(out: &str) -> impl Iterator<Item = &str> {
    let mut skip = false;
    out.split('\0').filter(|t| !t.is_empty()).filter_map(move |t| {
        if std::mem::take(&mut skip) {
            return None;
        }
        let (code, path) = (t.get(..2)?, t.get(3..)?);
        skip = code.contains('R') || code.contains('C');
        Some(path)
    })
}

fn names(out: &str) -> impl Iterator<Item = String> + '_ {
    out.split('\0').filter(|t| !t.is_empty()).map(str::to_string)
}

/// The files the new work's base carries that main lacks: one git call.
pub fn base_files(run: RunGit<'_>, repo: &Path, base: &str) -> Result<BTreeSet<String>, String> {
    Ok(names(&run(repo, &["diff", "--name-only", "-z", &format!("{MAIN}...{base}")])?).collect())
}

/// The other locked worktrees with their files: two git calls each (one status, one diff list), the primary and `skip` left out.
/// An unlocked worktree is not an agent's and is ignored. A worktree that cannot be read is named in the second result, never
/// taken for clean (ER-33).
pub fn gather(run: RunGit<'_>, list: &[WorktreeInfo], skip: &str) -> (Vec<Other>, Vec<String>) {
    let (mut others, mut unread) = (Vec::new(), Vec::new());
    // per-item: `git status` reads one worktree's index, so it runs inside that folder: two calls per locked worktree, checked by
    // the budget test `gathering_costs_two_git_calls_per_locked_worktree`.
    for w in list.iter().skip(1).filter(|w| !w.prunable && w.path != skip) {
        let Some(reason) = &w.locked else { continue };
        let dir = Path::new(&w.path);
        let read = || -> Result<BTreeSet<String>, String> {
            let mut files: BTreeSet<String> = status_paths(&run(dir, &["status", "--porcelain", "-z", "--untracked-files=all"])?)
                .map(str::to_string)
                .collect();
            files.extend(names(&run(dir, &["diff", "--name-only", "-z", &format!("{MAIN}...HEAD")])?));
            Ok(files)
        };
        match read() {
            Ok(files) => others.push(Other {
                path: w.path.clone(),
                owner: Note::parse(reason).map_or_else(|| "unknown owner".into(), |n| format!("{}:{}", n.role, n.task)),
                branch: w.branch.clone().unwrap_or_default(),
                files,
            }),
            Err(why) => unread.push(format!("{}: {why}", w.path)),
        }
    }
    (others, unread)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn set(files: &[&str]) -> BTreeSet<String> {
        files.iter().map(|f| (*f).to_string()).collect()
    }

    fn other(path: &str, branch: &str, files: &[&str]) -> Other {
        Other {
            path: path.into(),
            owner: "implementer:T-1".into(),
            branch: branch.into(),
            files: set(files),
        }
    }

    #[test]
    fn the_topic_stem_is_the_first_two_words_after_the_type_prefix() {
        assert_eq!(topic_stem("feat/worktree-overlap-warning").as_deref(), Some("worktree-overlap"));
        assert_eq!(topic_stem("fix/worktree-overlap").as_deref(), Some("worktree-overlap"));
        assert_eq!(topic_stem("ci/old-base-block").as_deref(), Some("old-base"));
        assert_eq!(topic_stem("solo").as_deref(), Some("solo"));
        assert_eq!(topic_stem(""), None);
    }

    #[test]
    fn a_shared_file_or_a_shared_topic_warns_naming_the_worktree_and_its_owner_and_nothing_else_does() {
        let others = [
            other("/wt/a", "feat/other-job", &["x.rs", "y.rs"]),
            other("/wt/b", "feat/worktree-overlap-v2", &[]),
            other("/wt/c", "feat/unrelated", &["z.rs"]),
        ];
        let found = overlaps(Some("feat/worktree-overlap-warning"), &set(&["x.rs", "q.rs"]), &others);
        assert_eq!(found.iter().map(|o| o.other.path.as_str()).collect::<Vec<_>>(), ["/wt/a", "/wt/b"]);
        assert_eq!(found[0].shared, ["x.rs"]);
        assert!(found[1].same_topic && found[1].shared.is_empty());
        let text = render(&found);
        assert!(
            text.contains("::warning::worktree add: /wt/a (owner implementer:T-1)")
                && text.contains("x.rs")
                && text.contains("/wt/b")
                && text.contains("same topic")
                && !text.contains("/wt/c")
                && text.contains("nothing was refused"),
            "{text}"
        );
        assert!(
            overlaps(None, &BTreeSet::new(), &others).is_empty(),
            "a detached copy with no files overlaps nothing"
        );
        assert_eq!(render(&[]), "");
    }

    #[test]
    fn the_warning_counts_the_files_it_does_not_name() {
        let many: Vec<String> = (0..13).map(|n| format!("f{n:02}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        let found = overlaps(Some("feat/a"), &set(&refs), &[other("/wt/a", "feat/b", &refs)]);
        let text = render(&found);
        assert!(text.contains("f09 and 3 more") && !text.contains("f10"), "{text}");
    }

    fn info(path: &str, branch: &str, locked: Option<&str>) -> WorktreeInfo {
        WorktreeInfo {
            path: path.into(),
            head: "h".into(),
            branch: Some(branch.into()),
            detached: false,
            locked: locked.map(str::to_string),
            prunable: false,
        }
    }

    #[test]
    fn gathering_costs_two_git_calls_per_locked_worktree_and_ignores_the_primary_unlocked_and_the_new_one() {
        let list = [
            info("/primary", "main", None),
            info(
                "/wt/a",
                "feat/a",
                Some("owner=implementer:T-9; branch=feat/a; base=abc; created=2026-11-25T00:00:00Z; by=coordinator"),
            ),
            info("/wt/b", "feat/b", Some("free text from a person")),
            info("/wt/unlocked", "feat/u", None),
            info("/wt/new", "feat/n", Some("owner=implementer:T-1")),
        ];
        let calls = RefCell::new(Vec::new());
        let run = |dir: &Path, args: &[&str]| -> Result<String, String> {
            calls.borrow_mut().push(format!("{} {}", dir.display(), args[0]));
            Ok(if args[0] == "status" {
                " M src/a.rs\0?? new.txt\0R  to.rs\0from.rs\0".into()
            } else {
                "src/b.rs\0".into()
            })
        };
        let (others, unread) = gather(&run, &list, "/wt/new");
        assert!(unread.is_empty());
        assert_eq!(
            calls.borrow().len(),
            4,
            "2 locked worktrees x (one status + one diff list): {:?}",
            calls.borrow()
        );
        assert_eq!(others[0].owner, "implementer:T-9");
        assert_eq!(others[1].owner, "unknown owner", "a lock with free text has no owner");
        assert_eq!(
            others[0].files,
            set(&["src/a.rs", "new.txt", "to.rs", "src/b.rs"]),
            "a rename's old name is skipped"
        );
    }

    #[test]
    fn a_worktree_that_cannot_be_read_is_named_never_taken_for_clean() {
        let list = [info("/primary", "main", None), info("/wt/a", "feat/a", Some("x"))];
        let run = |_: &Path, _: &[&str]| -> Result<String, String> { Err("`git status` failed: not a repository".into()) };
        let (others, unread) = gather(&run, &list, "/wt/new");
        assert!(others.is_empty());
        assert_eq!(unread.len(), 1);
        assert!(unread[0].starts_with("/wt/a: "), "{unread:?}");
    }
}
