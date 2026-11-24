//! Shrink-only baselines by text (lesson 2026-11-04, C71's baseline): a baseline file lists what is known to be wrong today,
//! one entry per line, keyed by the text of the site (a hash or a snippet), never by a line number that moves. The file only
//! ever shrinks: an entry the code no longer produces is stale and is removed; an entry never appears by itself. `--write`
//! style tools use [`Baseline::shrunk`] (it cannot add) and [`ensure_shrink_only`] (it refuses a file that has grown).
//!
//! A check's list has to land on the target branch while the check is still report-only: a list that does not exist at the merge base is an empty
//! base, so every entry of a list created on a branch is growth (ER-46) and the branch gets one finding per file until the list is on the target branch.
//!
//! Blank lines and `#` comment lines are kept as they are and are not entries.

use super::finding::Finding;
use super::jsre::is_js_space;
use paddock_proc::{Limits, run_bounded};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::Path;
use std::time::Duration;

/// Whether `key` can be an entry: one line, no surrounding white space, not empty, not starting with `#` (that is a comment). A key
/// that cannot is never in a baseline, so a site with such a key always counts as new (it cannot be baselined by mistake).
pub fn is_storable(key: &str) -> bool {
    !key.is_empty() && key == key.trim() && !key.starts_with('#') && !key.contains('\n') && !key.contains('\r')
}

/// A parsed baseline file.
#[derive(Clone, Debug, PartialEq)]
pub struct Baseline {
    lines: Vec<String>,
    entries: BTreeSet<String>,
}

/// The comparison of a baseline with what a run produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judgement {
    /// Produced now and not in the baseline: a regression (an error).
    pub new: Vec<String>,
    /// In the baseline and not produced now: stale, to be removed.
    pub stale: Vec<String>,
}

fn entry_of(line: &str) -> Option<&str> {
    let t = line.trim();
    (!t.is_empty() && !t.starts_with('#')).then_some(t)
}

impl Baseline {
    /// Reads the file's text.
    pub fn parse(text: &str) -> Baseline {
        let lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
        let entries = lines.iter().filter_map(|l| entry_of(l)).map(str::to_string).collect();
        Baseline { lines, entries }
    }

    /// The entries.
    pub fn entries(&self) -> &BTreeSet<String> {
        &self.entries
    }

    /// Compares the baseline with the keys a run produced (sorted, without duplicates in the result).
    pub fn judge(&self, current: &[String]) -> Judgement {
        let now: BTreeSet<&str> = current.iter().map(String::as_str).collect();
        Judgement {
            new: now.iter().filter(|k| !self.entries.contains(**k)).map(|k| k.to_string()).collect(),
            stale: self.entries.iter().filter(|e| !now.contains(e.as_str())).cloned().collect(),
        }
    }

    /// The file's text without its stale entries, everything else byte for byte (line endings included). It never adds a line.
    pub fn shrunk(&self, current: &[String]) -> String {
        let now: BTreeSet<&str> = current.iter().map(String::as_str).collect();
        let mut out = String::new();
        for l in &self.lines {
            if entry_of(l).is_some_and(|e| !now.contains(e)) {
                continue;
            }
            out.push_str(l);
        }
        out
    }
}

/// Refuses a baseline text that has an entry the old one did not (a baseline grows only by a human decision recorded
/// elsewhere). Returns the added entries.
pub fn ensure_shrink_only(old: &str, new: &str) -> Result<(), Vec<String>> {
    let old = Baseline::parse(old);
    let added: Vec<String> = Baseline::parse(new).entries.difference(&old.entries).cloned().collect();
    if added.is_empty() { Ok(()) } else { Err(added) }
}

/// The result of [`reconcile`], as indexes: the current items with no entry, the entries with no current item, and the moved pairs
/// (current index, entry index).
#[derive(Debug, PartialEq, Eq)]
pub struct Reconciled {
    /// Current items the baseline does not cover: real additions.
    pub fresh: Vec<usize>,
    /// Entries nothing matches any more, in baseline order.
    pub stale: Vec<usize>,
    /// Pairs (current, entry) whose loose keys agree but whose exact keys do not: the same text in another place.
    pub moved: Vec<(usize, usize)>,
}

/// Matches `current` against `base` entries as `reconcile` in `conformance/src/util/baseline.mjs` does, in two passes: first by the
/// `exact` key (for example file and hash), then the leftovers by the `loose` key (for example the hash alone), where `can_move
/// (current index, entry index)` allows the pairing (a site check lets only an entry whose file is gone move; the growth check
/// lets any). A pass-two match is a MOVED entry: neither new nor stale. With no `loose` nothing moves. Counts are per key, so two
/// identical lines in two files never hide a real addition.
pub fn reconcile<T>(
    current: &[T],
    base: &[T],
    exact: &dyn Fn(&T) -> String,
    loose: Option<&dyn Fn(&T) -> String>,
    can_move: &dyn Fn(usize, usize) -> bool,
) -> Reconciled {
    let mut buckets: HashMap<String, VecDeque<usize>> = HashMap::new();
    for (i, e) in base.iter().enumerate() {
        buckets.entry(exact(e)).or_default().push_back(i);
    }
    let mut unmatched: BTreeSet<usize> = (0..base.len()).collect();
    let mut rest = Vec::new();
    for (n, c) in current.iter().enumerate() {
        match buckets.get_mut(&exact(c)).and_then(VecDeque::pop_front) {
            Some(i) => {
                unmatched.remove(&i);
            }
            None => rest.push(n),
        }
    }
    let mut pool: HashMap<String, Vec<usize>> = HashMap::new();
    if let Some(loose) = loose {
        for &i in &unmatched {
            pool.entry(loose(&base[i])).or_default().push(i);
        }
    }
    let (mut fresh, mut moved) = (Vec::new(), Vec::new());
    for n in rest {
        let hit = loose.and_then(|loose| {
            let list = pool.get_mut(&loose(&current[n]))?;
            let at = list.iter().position(|&i| can_move(n, i))?;
            Some(list.remove(at))
        });
        match hit {
            Some(i) => {
                unmatched.remove(&i);
                moved.push((n, i));
            }
            None => fresh.push(n),
        }
    }
    Reconciled {
        fresh,
        stale: unmatched.into_iter().collect(),
        moved,
    }
}

/// `git args` in `root`: standard output, or the error line the JavaScript reports (`Command failed: git ...`) and the standard error.
fn git(root: &Path, args: &[&str]) -> Result<String, (String, String)> {
    let mut cmd = crate::git_env::git();
    cmd.arg("-C").arg(root).args(args);
    let failed = |e: String| (format!("Command failed: git {}", args.join(" ")), e);
    match run_bounded(cmd, Limits::new(Duration::from_secs(120)).max_output(1 << 26)) {
        Ok(out) if out.success() => Ok(out.stdout_text()),
        Ok(out) => Err(failed(out.stderr_text())),
        Err(e) => Err(failed(e.to_string())),
    }
}

/// The ref every baseline's and allow list's growth is measured against: `explicit` (the run's own), else `PADDOCK_BASE_REF`, else
/// `origin/main`; an empty value is unset. The ONE place that reads `PADDOCK_BASE_REF` (ER-21: C53, C71, C115, C104 and the shared
/// allow-list reader each read it or forgot it, and a member branch of a train was measured against main, so the train's own entries
/// counted as its growth). CI's conformance job and `./dev check --pre-push` set it to the branch's target (the train
/// before, for a train member). A deliberate change of meaning for the blocking baselines (C53, C71, C104, C115): the target branch
/// instead of `origin/main` (ADR 0098, amendment). JavaScript host: `targetRef` in `conformance/src/util/baseline.mjs`.
pub fn target_ref(explicit: Option<&str>) -> String {
    explicit
        .filter(|r| !r.is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("PADDOCK_BASE_REF").ok().filter(|r| !r.is_empty()))
        .unwrap_or_else(|| "origin/main".into())
}

/// Whether the environment says this is a CI run (`CI`, `GITEA_ACTIONS` or `GITHUB_ACTIONS` set and not false/0): a target that cannot
/// be seen is then not measured. JavaScript host: `isCi`.
pub fn is_ci(get: &dyn Fn(&str) -> Option<String>) -> bool {
    ["CI", "GITEA_ACTIONS", "GITHUB_ACTIONS"]
        .iter()
        .any(|k| get(k).is_some_and(|v| !v.is_empty() && !v.eq_ignore_ascii_case("false") && v != "0"))
}

const FETCH_FIX: &str =
    "Fetch the target branch (`git fetch origin main`, full history), or set PADDOCK_BASE_REF to the ref to compare with.";
const PARSE_FIX: &str = "The list at the base is in an older format (an entry without a `site`), so growth cannot be measured against it; nothing to fix here: it is measured once the new format is on the base branch.";

/// The finding for a growth check that could not measure: a warning with the next step, except that in CI (`ci`) a target that cannot be
/// resolved is a not-measured error no waiver covers (ADR 0089: not measured never passes; a blocking baseline must not turn into a
/// warning because the target ref is missing). `what` names the list (`the baseline`). The fix follows the typed reason, never its text.
pub fn unmeasured_finding(ci: bool, id: &str, rule: u32, file: &str, what: &str, why: &Unmeasured) -> Finding {
    if ci && matches!(why, Unmeasured::NoBase(_)) {
        return Finding::error(
            id,
            rule,
            file,
            1,
            format!(
                "not measured: {what}'s shrink-only rule cannot be checked, the target ref is not usable: {}",
                why.reason()
            ),
            format!("{FETCH_FIX} A gate that cannot see its target does not pass (ADR 0089)."),
        )
        .unwaivable()
        .with_outcome(super::outcome::Outcome::NotMeasured);
    }
    let fix = if matches!(why, Unmeasured::BaselineUnparsed(_)) {
        PARSE_FIX
    } else {
        FETCH_FIX
    };
    Finding::warn(
        id,
        rule,
        file,
        1,
        format!("{what}'s shrink-only rule is not enforced here: {}", why.reason()),
        fix,
    )
}

/// Whether `root` is a git work tree git itself accepts (a folder with a stray `.git` file, as a fixture has, is not).
pub fn is_work_tree(root: &Path) -> bool {
    git(root, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|t| t.trim_matches(is_js_space) == "true")
}

/// A current entry whose loose key is in the baseline at the merge base but whose exact key is not.
#[derive(Debug, PartialEq, Eq)]
pub struct Moved<T> {
    /// Index into the current entries.
    pub current: usize,
    /// The merge base's entry it was paired with.
    pub from: T,
}

/// What the baseline at the merge base says about the current entries (`baselineGrowth`).
#[derive(Debug, PartialEq, Eq)]
pub enum Growth<T> {
    /// Measured: the merge base, how many current entries it lacks, and which it holds under another place.
    Added {
        /// The merge base's commit.
        base: String,
        /// Entries the merge base's baseline does not have.
        added: usize,
        /// Entries it has under another exact key.
        moved: Vec<Moved<T>>,
    },
    /// Not measurable here: a warning (an error in CI for [`Unmeasured::NoBase`]), never silence.
    Unmeasured(Unmeasured),
}

/// Why growth could not be measured, typed so the finding's fix never depends on the message's words (the JavaScript host's `kind`).
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Unmeasured {
    /// No usable target ref or merge base (no git, a ref that does not resolve).
    NoBase(String),
    /// The list at the merge base cannot be read.
    BaselineUnreadable(String),
    /// The list at the merge base is in a format this version cannot parse (an older one).
    BaselineUnparsed(String),
}

impl Unmeasured {
    /// The reason as the findings print it.
    pub fn reason(&self) -> &str {
        match self {
            Unmeasured::NoBase(r) | Unmeasured::BaselineUnreadable(r) | Unmeasured::BaselineUnparsed(r) => r,
        }
    }
}

/// The entries of `current` that the baseline file `path` at the merge base with `base_ref` does not have, or the reason that
/// cannot be measured (`baselineGrowth`). On the target branch itself the commit is compared with its first parent. A baseline
/// absent at the merge base is an empty base: every current entry is growth (ER-46). `entries_of` reads the entries out of the file's text.
pub fn growth<T: Clone>(
    root: &Path,
    base_ref: &str,
    path: &str,
    current: &[T],
    entries_of: &dyn Fn(&str) -> Option<Vec<T>>,
    exact: &dyn Fn(&T) -> String,
    loose: Option<&dyn Fn(&T) -> String>,
) -> Growth<T> {
    let lookup = || -> Result<String, (String, String)> {
        let head = git(root, &["rev-parse", "HEAD"])?.trim_matches(is_js_space).to_string();
        let mut base = git(root, &["merge-base", "HEAD", base_ref])?.trim_matches(is_js_space).to_string();
        if base == head {
            base = git(root, &["rev-parse", "HEAD^1"])?.trim_matches(is_js_space).to_string();
        }
        Ok(base)
    };
    let base = match lookup() {
        Ok(b) => b,
        Err((msg, _)) => {
            let first: String = msg.split('\n').next().unwrap_or("").chars().take(120).collect();
            return Growth::Unmeasured(Unmeasured::NoBase(format!("no merge base ({first})")));
        }
    };
    let short = || base.chars().take(9).collect::<String>();
    // A list absent at the merge base is an empty base: every current entry is growth (ER-46). Treating it as the "first introduction" let a new list
    // with any number of entries in silently (a new apps/<app>/app/eslint-suppressions.json with count 5, the second time this class was found).
    let old: Vec<T> = match git(root, &["show", &format!("{base}:{path}")]) {
        Ok(text) => {
            let Some(old) = entries_of(&text) else {
                return Growth::Unmeasured(Unmeasured::BaselineUnparsed(format!(
                    "the baseline at {} cannot be parsed",
                    short()
                )));
            };
            old
        }
        Err((_, stderr)) => {
            let missing = ["does not exist", "exists on disk, but not in", "not in"]
                .iter()
                .any(|p| stderr.contains(p));
            if !missing {
                return Growth::Unmeasured(Unmeasured::BaselineUnreadable(format!(
                    "the baseline at {} cannot be read",
                    short()
                )));
            }
            Vec::new()
        }
    };
    let r = reconcile(current, &old, exact, loose, &|_, _| true);
    Growth::Added {
        base,
        added: r.fresh.len(),
        moved: r
            .moved
            .iter()
            .map(|&(n, i)| Moved {
                current: n,
                from: old[i].clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_ref_is_the_runs_own_then_the_variable_then_main_and_this_is_the_one_reader() {
        let mut env = paddock_testkit::env_lock();
        env.remove("PADDOCK_BASE_REF");
        assert_eq!(target_ref(None), "origin/main");
        env.set("PADDOCK_BASE_REF", "origin/train/x");
        assert_eq!(target_ref(None), "origin/train/x");
        assert_eq!(target_ref(Some("topic")), "topic");
        // An empty value is unset, in the variable and in the run's own ref.
        env.set("PADDOCK_BASE_REF", "");
        assert_eq!(target_ref(None), "origin/main");
        assert_eq!(target_ref(Some("")), "origin/main");
        env.set("PADDOCK_BASE_REF", "origin/train/x");
        assert_eq!(target_ref(Some("")), "origin/train/x");
    }

    #[test]
    fn ci_is_what_the_environment_says_and_false_or_zero_is_not() {
        let with = |k: &'static str, v: &'static str| move |n: &str| (n == k).then(|| v.to_string());
        assert!(is_ci(&with("CI", "true")) && is_ci(&with("GITEA_ACTIONS", "true")) && is_ci(&with("GITHUB_ACTIONS", "1")));
        assert!(!is_ci(&with("CI", "false")) && !is_ci(&with("CI", "0")) && !is_ci(&with("CI", "")) && !is_ci(&|_| None));
    }

    #[test]
    fn a_target_that_cannot_be_seen_is_a_warning_locally_and_a_not_measured_error_in_ci_and_the_fix_follows_the_kind() {
        let no_base = Unmeasured::NoBase("no merge base (x)".into());
        let local = unmeasured_finding(false, "CX", 21, "f.json", "the baseline", &no_base);
        assert_eq!((local.severity, local.unwaivable), (super::super::finding::Severity::Warn, false));
        let ci = unmeasured_finding(true, "CX", 21, "f.json", "the baseline", &no_base);
        assert_eq!(
            (ci.severity, ci.unwaivable, ci.outcome),
            (
                super::super::finding::Severity::Error,
                true,
                Some(super::super::outcome::Outcome::NotMeasured)
            )
        );
        assert!(
            ci.message
                .starts_with("not measured: the baseline's shrink-only rule cannot be checked")
                && ci.fix.contains("ADR 0089")
        );
        // An older format at the base is not a missing target: a warning with its own fix, in CI too.
        let old = Unmeasured::BaselineUnparsed("the baseline at abc cannot be parsed".into());
        let f = unmeasured_finding(true, "CX", 21, "f.json", "the baseline", &old);
        assert!(f.fix.contains("older format") && f.severity == super::super::finding::Severity::Warn);
        // The words of the reason do not choose the fix: a NoBase reason that says "cannot be parsed" still has the fetch fix.
        let tricky = Unmeasured::NoBase("cannot be parsed".into());
        assert!(
            unmeasured_finding(false, "CX", 21, "f.json", "x", &tricky)
                .fix
                .contains("Fetch the target branch")
        );
    }

    const FILE: &str = "# header\nabc\n\n  def  \n# note\nghi\n";

    fn keys(k: &[&str]) -> Vec<String> {
        k.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn judges_new_and_stale_by_text() {
        let b = Baseline::parse(FILE);
        let j = b.judge(&keys(&["abc", "zzz", "ghi", "zzz"]));
        assert_eq!(j.new, ["zzz"]);
        assert_eq!(j.stale, ["def"]);
    }

    #[test]
    fn shrunk_drops_only_stale_entries_and_keeps_comments() {
        let b = Baseline::parse(FILE);
        assert_eq!(b.shrunk(&keys(&["abc", "ghi", "new"])), "# header\nabc\n\n# note\nghi\n");
    }

    #[test]
    fn shrinking_never_adds_whatever_the_run_produced() {
        let b = Baseline::parse(FILE);
        // A small deterministic sweep over subsets of known and unknown keys.
        let pool = ["abc", "def", "ghi", "new1", "new2"];
        for mask in 0u32..32 {
            let current: Vec<String> = pool
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, k)| k.to_string())
                .collect();
            let shrunk = b.shrunk(&current);
            assert!(ensure_shrink_only(FILE, &shrunk).is_ok(), "mask {mask}");
            let again = Baseline::parse(&shrunk);
            assert!(again.judge(&current).stale.is_empty(), "mask {mask}: nothing stale is left");
            assert!(again.entries().is_subset(b.entries()));
        }
    }

    #[test]
    fn line_endings_and_a_missing_final_newline_are_kept() {
        let b = Baseline::parse("# h\r\nabc\r\ndef\r\nghi");
        assert_eq!(b.shrunk(&keys(&["abc", "ghi"])), "# h\r\nabc\r\nghi");
        assert_eq!(b.shrunk(&keys(&["abc", "def", "ghi"])), "# h\r\nabc\r\ndef\r\nghi");
        assert!(
            b.judge(&keys(&["abc", "def", "ghi"])).stale.is_empty(),
            "a CR is not part of the entry"
        );
    }

    #[test]
    fn keys_that_cannot_be_stored_are_always_new() {
        assert!(
            is_storable("a b")
                && !is_storable("#x")
                && !is_storable(" x")
                && !is_storable("x ")
                && !is_storable("")
                && !is_storable("a\nb")
        );
        let b = Baseline::parse("abc\n");
        let j = b.judge(&keys(&["#abc", " abc", "abc "]));
        assert_eq!(j.new.len(), 3);
        assert_eq!(j.stale, ["abc"], "none of those keys is the entry `abc`");
        assert_eq!(b.shrunk(&keys(&["#abc"])), "");
    }

    #[test]
    fn a_grown_file_is_refused_with_the_added_entries() {
        assert_eq!(ensure_shrink_only(FILE, "abc\nplanted\n"), Err(vec!["planted".to_string()]));
        assert!(ensure_shrink_only(FILE, "abc\n").is_ok());
    }

    fn git(root: &std::path::Path, args: &[&str]) {
        // The one way this tool starts git (git_env.rs: a hook's GIT_DIR must not reach a `git -C <folder>`).
        let out = crate::git_env::git()
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    /// ER-46: a list that is absent at the merge base is an empty base, so every entry of it is growth (a new list with any number of entries
    /// went in silently when this was "the first introduction"). Present at the base: the usual comparison.
    #[test]
    fn a_list_absent_at_the_merge_base_is_an_empty_base_and_every_entry_is_growth() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("keep"), "x").unwrap();
        git(d.path(), &["init", "-q", "-b", "main"]);
        git(d.path(), &["add", "."]);
        git(d.path(), &["commit", "-q", "-m", "base: no list"]);
        git(d.path(), &["checkout", "-q", "-b", "topic"]);
        std::fs::write(d.path().join("l.txt"), "a\nb\nb\n").unwrap();
        git(d.path(), &["add", "."]);
        git(d.path(), &["commit", "-q", "-m", "the list appears"]);
        let of = |t: &str| Some(t.lines().map(str::to_string).collect::<Vec<_>>());
        let exact = |k: &String| k.clone();
        let current = keys(&["a", "b", "b"]);
        assert!(matches!(
            growth(d.path(), "main", "l.txt", &current, &of, &exact, None),
            Growth::Added { added: 3, .. }
        ));
        assert!(
            matches!(
                growth(d.path(), "main", "l.txt", &[], &of, &exact, None),
                Growth::Added { added: 0, .. }
            ),
            "a new empty list adds nothing"
        );
        // On the base itself the commit is compared with its first parent, where the list is absent too.
        git(d.path(), &["checkout", "-q", "main"]);
        git(d.path(), &["merge", "-q", "--ff-only", "topic"]);
        assert!(matches!(
            growth(d.path(), "main", "l.txt", &current, &of, &exact, None),
            Growth::Added { added: 3, .. }
        ));
    }
}
