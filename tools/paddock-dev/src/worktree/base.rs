//! The default base of `./dev worktree add`: the newest open train, else `origin/main`.
//!
//! A branch that starts from an old `main` while a train is open clashes with the train when it is merged (four branches did on
//! 2026-11-22/23). An *open* train is an `origin/train/YYYY-MM-DD-N` branch with commits `origin/main` lacks; a landed train is an
//! ancestor of main and is ignored. Cost (ER-20): two git calls however many trains exist, one `git fetch origin` and one
//! `git for-each-ref --no-merged=origin/main`, which lets git do the ancestor test for every ref at once.

use super::git::GitError;
use super::primary::REMOTE_MAIN;
use paddock_cli::CliError;

/// The prefix of the open-train refs.
const TRAIN_REFS: &str = "refs/remotes/origin/train/";

/// What `add` starts from, and why (the line the tool prints).
#[derive(Debug, PartialEq, Eq)]
pub struct Base {
    /// The commit-ish to start from, for example `origin/train/2026-11-22-20`.
    pub name: String,
    /// One sentence for the report: what was chosen and how to choose otherwise.
    pub why: String,
}

/// Runs git with the given arguments and returns standard output. The real one is `Git::text`; tests pass a fake.
pub type RunGit<'a> = &'a dyn Fn(&[&str]) -> Result<String, GitError>;

/// (year, month, day, number): trains compare by date, then by number.
type TrainKey = (u32, u32, u32, u32);

/// `train/YYYY-MM-DD-N` as a sortable key, or `None` for any other name.
fn train_key(name: &str) -> Option<TrainKey> {
    let rest = name.strip_prefix("train/")?;
    let mut parts = rest.split('-');
    let mut next = |len: Option<usize>| -> Option<u32> {
        let p = parts.next()?;
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || len.is_some_and(|l| p.len() != l) {
            return None;
        }
        p.parse().ok()
    };
    let key = (next(Some(4))?, next(Some(2))?, next(Some(2))?, next(None)?);
    parts.next().is_none().then_some(key)
}

/// Picks the base for a new branch: the newest open train, else `origin/main`; `fetched` says whether the fetch worked.
///
/// `listing` is the output of `for-each-ref --no-merged=origin/main --format='%(refname:strip=3) %(objectname:short)'`.
fn choose(listing: &str, fetched: Result<(), String>) -> Result<Base, CliError> {
    let stale = match &fetched {
        Ok(()) => String::new(),
        Err(why) => format!(" Not measured against the remote: `git fetch origin` failed ({why}); the refs are as of the last fetch."),
    };
    let mut open: Vec<(TrainKey, &str, &str)> = Vec::new();
    let mut ignored: Vec<&str> = Vec::new();
    for line in listing.lines().filter(|l| !l.trim().is_empty()) {
        let (name, sha) = line.split_once(' ').unwrap_or((line, ""));
        match train_key(name) {
            Some(k) => open.push((k, name, sha)),
            None => ignored.push(name),
        }
    }
    let skipped = if ignored.is_empty() {
        String::new()
    } else {
        format!(" Ignored (not named train/YYYY-MM-DD-N): {}.", ignored.join(", "))
    };
    let Some(newest) = open.iter().map(|o| o.0).max() else {
        return Ok(Base {
            name: REMOTE_MAIN.to_string(),
            why: format!("based on {REMOTE_MAIN}: no train is open.{skipped}{stale}"),
        });
    };
    let top: Vec<&(TrainKey, &str, &str)> = open.iter().filter(|o| o.0 == newest).collect();
    if top.len() > 1 {
        let names: Vec<String> = top.iter().map(|o| format!("origin/{}", o.1)).collect();
        return Err(CliError::usage(format!(
            "Several open trains rank the same ({}), so there is no newest one.",
            names.join(", ")
        ))
        .with_hint("Pass --base with the one you mean (or --base origin/main)."));
    }
    let (_, name, sha) = *top[0];
    Ok(Base {
        name: format!("origin/{name}"),
        why: format!("based on origin/{name} ({sha}), the open train; pass --base {REMOTE_MAIN} to start from main.{skipped}{stale}"),
    })
}

/// Fetches, lists the open trains in one call and chooses. Two git calls in all.
pub fn open_train_or_main(run: RunGit<'_>) -> Result<Base, CliError> {
    let fetched = run(&["fetch", "--quiet", "origin"])
        .map(|_| ())
        .map_err(|e| e.stderr.trim().to_string());
    let listing = run(&[
        "for-each-ref",
        &format!("--no-merged={REMOTE_MAIN}"),
        "--format=%(refname:strip=3) %(objectname:short=9)",
        TRAIN_REFS,
    ])
    .map_err(|e| CliError::failure(e.to_string()).with_hint("Pass --base <ref> to name the base yourself."))?;
    choose(&listing, fetched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn ok() -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn no_open_train_gives_main() {
        let b = choose("", ok()).unwrap();
        assert_eq!(b.name, "origin/main");
        assert!(b.why.contains("no train is open"), "{}", b.why);
    }

    #[test]
    fn one_open_train_gives_that_train_and_says_why_and_how_to_override() {
        let b = choose("train/2026-11-22-20 803aed12d\n", ok()).unwrap();
        assert_eq!(b.name, "origin/train/2026-11-22-20");
        assert!(
            b.why.contains("803aed12d") && b.why.contains("the open train") && b.why.contains("--base origin/main"),
            "{}",
            b.why
        );
    }

    #[test]
    fn the_newest_by_date_then_number_wins_and_numbers_compare_as_numbers() {
        let b = choose("train/2026-11-22-9 a\ntrain/2026-11-21-30 b\ntrain/2026-11-22-20 c\n", ok()).unwrap();
        assert_eq!(b.name, "origin/train/2026-11-22-20");
    }

    #[test]
    fn a_tie_fails_naming_both_and_the_flag() {
        let e = choose("train/2026-11-22-20 a\ntrain/2026-11-22-020 b\n", ok()).unwrap_err();
        let text = format!("{e:?}");
        assert!(
            text.contains("train/2026-11-22-20") && text.contains("train/2026-11-22-020"),
            "{text}"
        );
        assert!(text.contains("--base"), "{text}");
    }

    #[test]
    fn a_name_that_is_not_a_train_name_is_ignored_and_listed() {
        let b = choose("train/scratch x\ntrain/2026-11-22-1 y\n", ok()).unwrap();
        assert_eq!(b.name, "origin/train/2026-11-22-1");
        assert!(b.why.contains("train/scratch"), "{}", b.why);
        for bad in [
            "train/2026-11-22",
            "train/2026-1-22-3",
            "train/2026-11-22-x",
            "train/2026-11-22-1-2",
            "feat/2026-11-22-1",
        ] {
            assert_eq!(train_key(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_failed_fetch_is_said_and_never_hidden() {
        let b = choose("", Err("could not resolve host".into())).unwrap();
        assert!(
            b.why.contains("Not measured") && b.why.contains("could not resolve host"),
            "{}",
            b.why
        );
    }

    /// ER-20: the lookup is two git calls however many trains there are (positive control: the fake lists 50).
    #[test]
    fn the_lookup_is_two_git_calls_however_many_trains_exist() {
        let calls: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let listing: String = (1..=50).map(|n| format!("train/2026-11-22-{n} sha{n}\n")).collect();
        let run = |args: &[&str]| -> Result<String, GitError> {
            calls.borrow_mut().push(args.join(" "));
            Ok(if args[0] == "for-each-ref" {
                listing.clone()
            } else {
                String::new()
            })
        };
        let b = open_train_or_main(&run).unwrap();
        assert_eq!(b.name, "origin/train/2026-11-22-50");
        let calls = calls.borrow();
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert!(
            calls[0].starts_with("fetch") && calls[1].contains("--no-merged=origin/main"),
            "{calls:?}"
        );
    }
}
