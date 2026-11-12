//! `pr plan-status`: plan rows' State cells checked against the ledger (user, 2026-11-12: plan status is derived from the
//! pull requests, not kept by hand).
//!
//! A plan row is a row of a Markdown table with a `State` (or `Status`) column in `docs/plans/*.md` or
//! `apps/*/docs/plans/*.md`. Its references are the code spans of the State cell that name a branch (a family prefix such as
//! `feat/`, `fix/`, `docs/`, `ci/` or `train/`, then a name that does not end in a file extension) or a commit (7 to 40 hex
//! digits); when the State cell names none, those of a `Branch` or `PR` column. A row with no reference is not a ledger row
//! and is skipped.
//!
//! The row *claims done* when its State text (without `*`) starts with `done`, `landed`, `merged` or `on main`. The ledger
//! says *landed* when every reference is: a branch whose newest pull request is merged or labelled `state/landed` (or, with no
//! pull request, whose remote head is on main), or a commit on main. A row disagrees when the two differ; a reference that
//! neither the server nor this clone knows makes the row *not measured*, never agreeing (ER-33).

use super::gitea::Pull;
use super::ledger::Git;
use regex::Regex;
use serde_json::{Value, json};
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file, checked by the tests below.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

static CODE: LazyLock<Regex> = LazyLock::new(|| re(r"`([^`\s]+)`"));
static COMMIT: LazyLock<Regex> = LazyLock::new(|| re(r"^[0-9a-f]{7,40}$"));
/// A branch: one of the family's prefixes (Conventional Commit types, trains, stewards, wip) and a name.
static BRANCH: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"^(feat|fix|docs|chore|ci|test|refactor|perf|style|build|train|steward|wip|release|review|spike)/[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)*$",
    )
});
/// A path that a branch pattern would also match (`docs/plans/x.md`).
static FILE_LIKE: LazyLock<Regex> = LazyLock::new(|| re(r"\.[A-Za-z]{1,5}$"));
static SEPARATOR: LazyLock<Regex> = LazyLock::new(|| re(r"^\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)*\|?\s*$"));
static DONE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(done|landed|merged|on main)\b"));

/// One reference of a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ref {
    /// A branch name.
    Branch(String),
    /// A commit id.
    Commit(String),
}

/// What the ledger says about a reference or a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ledger {
    /// On main, through this commit (the merge commit, or the commit itself).
    Landed(String),
    /// Not on main; the text says where (`#12 state/approved`, `closed without merging`...).
    Open(String),
    /// Neither the server nor this clone knows it.
    Unknown(String),
}

/// A table row of a plan with references.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The line number (1-based).
    pub line: usize,
    /// The State column's index.
    pub state_col: usize,
    /// The State cell's text, trimmed.
    pub state: String,
    /// Its references.
    pub refs: Vec<Ref>,
}

impl Row {
    /// Whether the State text says the work is on main.
    pub fn claims_done(&self) -> bool {
        DONE.is_match(self.state.trim_start_matches(['*', '_', ' ']))
    }
}

/// Splits a table line into its cells (unescaped `|` only; the outer pipes are not cells).
pub fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = if t.ends_with('|') && !t.ends_with("\\|") {
        &t[..t.len() - 1]
    } else {
        t
    };
    let mut out = vec![String::new()];
    let mut esc = false;
    for c in t.chars() {
        if c == '|' && !esc {
            out.push(String::new());
        } else if let Some(last) = out.last_mut() {
            last.push(c);
        }
        esc = c == '\\' && !esc;
    }
    out
}

fn refs_in(text: &str) -> Vec<Ref> {
    CODE.captures_iter(text)
        .filter_map(|c| {
            let s = c[1].trim_start_matches("origin/");
            if COMMIT.is_match(s) {
                Some(Ref::Commit(s.to_string()))
            } else if BRANCH.is_match(s) && !s.contains("..") && !FILE_LIKE.is_match(s) {
                Some(Ref::Branch(s.to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// The ledger rows of one plan file.
pub fn rows(text: &str) -> Vec<Row> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        let is_table = lines[i].trim_start().starts_with('|') && SEPARATOR.is_match(lines[i + 1].trim());
        if !is_table {
            i += 1;
            continue;
        }
        let header: Vec<String> = cells(lines[i])
            .iter()
            .map(|c| c.trim().trim_matches('*').trim().to_ascii_lowercase())
            .collect();
        let col = |names: &[&str]| header.iter().position(|h| names.contains(&h.as_str()));
        let state_col = col(&["state", "status"]);
        let other_col = col(&["branch", "pr", "branches"]);
        i += 2;
        while i < lines.len() && lines[i].trim_start().starts_with('|') {
            if let Some(sc) = state_col {
                let c = cells(lines[i]);
                let state = c.get(sc).map(|s| s.trim().to_string()).unwrap_or_default();
                let mut refs = refs_in(&state);
                if refs.is_empty() {
                    refs = other_col.and_then(|o| c.get(o)).map(|s| refs_in(s)).unwrap_or_default();
                }
                if !refs.is_empty() {
                    out.push(Row {
                        line: i + 1,
                        state_col: sc,
                        state,
                        refs,
                    });
                }
            }
            i += 1;
        }
    }
    out
}

/// What the ledger says about one reference.
pub fn ledger_of(r: &Ref, pulls: &[Pull], git: &dyn Git, base: &str) -> Result<Ledger, String> {
    match r {
        Ref::Commit(c) => Ok(match git.on_base(c, base)? {
            Some(true) => Ledger::Landed(c.clone()),
            Some(false) => Ledger::Open(format!("{c} is not on {base}")),
            None => Ledger::Unknown(format!("commit {c} is not in this clone")),
        }),
        Ref::Branch(b) => {
            // The newest pull request of the branch decides.
            if let Some(p) = pulls.iter().filter(|p| &p.branch == b).max_by_key(|p| p.number) {
                if p.merged || p.labels.iter().any(|l| l == "state/landed") {
                    return Ok(Ledger::Landed(p.merge_commit.clone().unwrap_or_else(|| p.head.clone())));
                }
                if p.state == "closed" {
                    return Ok(Ledger::Open(format!("#{} closed without merging", p.number)));
                }
                let state = p
                    .labels
                    .iter()
                    .find(|l| l.starts_with("state/"))
                    .cloned()
                    .unwrap_or_else(|| "no state label".into());
                return Ok(Ledger::Open(format!("#{} {state}", p.number)));
            }
            match git.remote_head(b)? {
                Some(head) => Ok(match git.on_base(&head, base)? {
                    Some(true) => Ledger::Landed(head),
                    _ => Ledger::Open(format!("{b} has no pull request and is not on {base}")),
                }),
                None => Ok(Ledger::Unknown(format!("{b}: no pull request and no remote branch"))),
            }
        }
    }
}

/// A row's verdict from its references' (all landed: landed; any unknown: unknown; else open, naming the first open one).
pub fn ledger_of_row(each: &[Ledger]) -> Ledger {
    if let Some(u) = each.iter().find(|l| matches!(l, Ledger::Unknown(_))) {
        return u.clone();
    }
    if let Some(o) = each.iter().find(|l| matches!(l, Ledger::Open(_))) {
        return o.clone();
    }
    each.last().cloned().unwrap_or(Ledger::Unknown("no reference".into()))
}

/// The State text `--write` puts in a landed row.
pub fn done_text(commit: &str) -> String {
    format!("done (`{}`)", &commit[..commit.len().min(10)])
}

/// Replaces cell `col` of a table line with ` text `, keeping the other cells byte for byte.
pub fn replace_cell(line: &str, col: usize, text: &str) -> Option<String> {
    // Byte offsets of the unescaped pipes.
    let mut pipes = Vec::new();
    let mut esc = false;
    for (i, c) in line.char_indices() {
        if c == '|' && !esc {
            pipes.push(i);
        }
        esc = c == '\\' && !esc;
    }
    let lead = line.trim_start().starts_with('|');
    let (start, end) = if lead {
        (*pipes.get(col)? + 1, *pipes.get(col + 1)?)
    } else {
        (if col == 0 { 0 } else { *pipes.get(col - 1)? + 1 }, *pipes.get(col)?)
    };
    Some(format!("{} {text} {}", &line[..start], &line[end..]))
}

/// One finding of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// The row.
    pub row: Row,
    /// The ledger's verdict.
    pub ledger: Ledger,
    /// Whether `--write` changed the cell.
    pub written: bool,
}

/// Checks one plan's rows; with `write`, returns the new text with landed rows set to `done (<commit>)`.
pub fn check_file(text: &str, pulls: &[Pull], git: &dyn Git, base: &str, write: bool) -> Result<(Vec<Finding>, Option<String>), String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut findings = Vec::new();
    let mut changed = false;
    for row in rows(text) {
        let each = row
            .refs
            .iter()
            .map(|r| ledger_of(r, pulls, git, base))
            .collect::<Result<Vec<_>, _>>()?;
        let ledger = ledger_of_row(&each);
        let disagrees = match &ledger {
            Ledger::Landed(_) => !row.claims_done(),
            Ledger::Open(_) => row.claims_done(),
            Ledger::Unknown(_) => true,
        };
        if !disagrees {
            continue;
        }
        let mut written = false;
        if let (true, Ledger::Landed(c)) = (write, &ledger)
            && let Some(new) = replace_cell(&lines[row.line - 1], row.state_col, &done_text(c))
        {
            lines[row.line - 1] = new;
            written = true;
            changed = true;
        }
        findings.push(Finding { row, ledger, written });
    }
    let new_text = changed.then(|| {
        let mut t = lines.join("\n");
        if text.ends_with('\n') {
            t.push('\n');
        }
        t
    });
    Ok((findings, new_text))
}

/// The JSON of a finding.
pub fn finding_json(file: &str, f: &Finding) -> Value {
    let (verdict, ledger) = match &f.ledger {
        Ledger::Landed(c) => ("landed", c.clone()),
        Ledger::Open(t) => ("open", t.clone()),
        Ledger::Unknown(t) => ("not measured", t.clone()),
    };
    let refs: Vec<String> = f
        .row
        .refs
        .iter()
        .map(|r| match r {
            Ref::Branch(b) => b.clone(),
            Ref::Commit(c) => c.clone(),
        })
        .collect();
    json!({"file": file, "line": f.row.line, "state": f.row.state, "refs": refs, "ledger": verdict, "detail": ledger, "written": f.written})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeGit {
        on_main: HashMap<String, bool>,
        heads: HashMap<String, String>,
    }

    impl Git for FakeGit {
        fn remote_head(&self, branch: &str) -> Result<Option<String>, String> {
            Ok(self.heads.get(branch).cloned())
        }
        fn commits(&self, _: &str, _: &str) -> Result<Vec<(String, String)>, String> {
            Ok(vec![])
        }
        fn on_base(&self, commit: &str, _: &str) -> Result<Option<bool>, String> {
            Ok(self.on_main.get(commit).copied())
        }
    }

    fn pull(number: u64, branch: &str, state: &str, merged: Option<&str>, labels: &[&str]) -> Pull {
        Pull {
            number,
            title: String::new(),
            state: state.into(),
            draft: true,
            merged: merged.is_some(),
            merge_commit: merged.map(str::to_string),
            branch: branch.into(),
            head: "feedfacecafe".into(),
            base: "main".into(),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            url: String::new(),
        }
    }

    const PLAN: &str = include_str!("../../tests/fixtures/plan-status/plan.md");
    const WRITTEN: &str = include_str!("../../tests/fixtures/plan-status/plan.written.md");

    fn world() -> (Vec<Pull>, FakeGit) {
        let pulls = vec![
            pull(1, "feat/merged", "closed", Some("1234567890abcdef"), &["state/landed"]),
            pull(2, "feat/approved", "open", None, &["state/approved", "app/dyno"]),
            pull(3, "feat/landed-label", "open", None, &["state/landed"]),
            pull(4, "feat/dropped", "closed", None, &["state/fixes"]),
            pull(5, "feat/done-ok", "closed", Some("abcdef1234567"), &[]),
        ];
        let git = FakeGit {
            on_main: HashMap::from(
                [
                    ("aaaaaaa", true),
                    ("bbbbbbb", false),
                    ("c0ffee0", true),
                    ("1234567890", true),
                    ("feedfaceca", true),
                ]
                .map(|(c, on)| (c.to_string(), on)),
            ),
            heads: HashMap::from([("fix/no-pr".to_string(), "c0ffee0".to_string())]),
        };
        (pulls, git)
    }

    #[test]
    fn rows_find_the_state_column_and_its_references_or_the_branch_columns() {
        let r = rows(PLAN);
        let found: Vec<(usize, Vec<Ref>)> = r.iter().map(|r| (r.line, r.refs.clone())).collect();
        assert_eq!(found[0], (7, vec![Ref::Branch("feat/merged".into())]));
        assert!(found.iter().any(|(_, refs)| refs == &vec![Ref::Commit("aaaaaaa".into())]));
        assert!(
            found.iter().any(|(_, refs)| refs == &vec![Ref::Branch("fix/no-pr".into())]),
            "the Branch column: {found:?}"
        );
        assert!(
            !r.iter().any(|r| r.state.contains("no reference")),
            "a row without a reference is not a ledger row"
        );
        assert!(
            !r.iter().any(|r| r.refs.contains(&Ref::Branch("docs/plans/x.md".into()))),
            "a path is not a branch"
        );
    }

    #[test]
    fn disagreements_are_found_and_write_sets_only_landed_rows_to_done() {
        let (pulls, git) = world();
        let (findings, _) = check_file(PLAN, &pulls, &git, "main", false).unwrap();
        let got: Vec<(usize, &str)> = findings
            .iter()
            .map(|f| {
                (
                    f.row.line,
                    match f.ledger {
                        Ledger::Landed(_) => "landed",
                        Ledger::Open(_) => "open",
                        Ledger::Unknown(_) => "unknown",
                    },
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (7, "landed"),
                (9, "landed"),
                (10, "open"),
                (11, "open"),
                (13, "landed"),
                (14, "unknown"),
                (15, "open"),
                (21, "landed")
            ],
            "{findings:#?}"
        );
        let (findings, text) = check_file(PLAN, &pulls, &git, "main", true).unwrap();
        assert_eq!(text.as_deref(), Some(WRITTEN));
        assert_eq!(findings.iter().filter(|f| f.written).count(), 4);
        // After the write only the rows a person must judge remain.
        let (left, none) = check_file(WRITTEN, &pulls, &git, "main", true).unwrap();
        assert!(none.is_none());
        assert_eq!(left.iter().map(|f| f.row.line).collect::<Vec<_>>(), [10, 11, 14, 15]);
    }

    #[test]
    fn cells_respect_escaped_pipes_and_replace_keeps_the_other_cells() {
        assert_eq!(
            cells("| a | b \\| c | d |"),
            ["a", " b \\| c ", " d "].map(|s| if s == "a" { " a " } else { s })
        );
        assert_eq!(
            replace_cell("| a | open `x/y` | c |", 1, "done (`1234567890`)").unwrap(),
            "| a | done (`1234567890`) | c |"
        );
        assert_eq!(replace_cell("a | open | c", 1, "done").unwrap(), "a | done | c");
        assert!(replace_cell("| a |", 3, "x").is_none());
    }
}
