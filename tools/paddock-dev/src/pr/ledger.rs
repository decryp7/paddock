//! `pr open`, `pr label`, `pr list` and `pr sync`: the branch ledger on Gitea pull requests. Pure over the [`Api`] and a
//! [`Git`] reader, so the tests run against an in-memory server.

use super::gitea::{Api, CommitRun, Pull};
use super::labels::{self, Change, LabelSpec};
use serde_json::{Value, json};

/// What the ledger reads from git.
pub trait Git {
    /// The commit of `origin/<branch>`, `None` when the remote branch is not known here.
    fn remote_head(&self, branch: &str) -> Result<Option<String>, String>;
    /// The commits of `origin/<branch>` that `origin/<base>` lacks, oldest first: `(short id, subject)`.
    fn commits(&self, base: &str, branch: &str) -> Result<Vec<(String, String)>, String>;
    /// Whether `commit` is on `origin/<base>`; `None` when the commit is not in this clone.
    fn on_base(&self, commit: &str, base: &str) -> Result<Option<bool>, String>;
    /// Fetches these branches from origin into `refs/remotes/origin/`, so the commits read next are the server's.
    fn fetch(&self, branches: &[&str]) -> Result<(), String>;
}

/// What `pr sync` says when the repository does not mark fast-forwarded pull requests as merged.
pub const AUTODETECT_OFF: &str = "the repository's \"autodetect manual merge\" is off: a pull request whose head reaches main by a \
    fast-forward push stays open on the server. The ledger still sees it as landed (plan-status checks the head against main), \
    but turn the setting on (repository settings, pull requests) so the server agrees.";

/// The longest commit list a pull request body carries.
const MAX_BODY_COMMITS: usize = 200;

/// Refuses a branch name git would read as an option or a range.
pub fn check_branch(branch: &str) -> Result<(), String> {
    let bad = branch.is_empty()
        || branch.starts_with('-')
        || branch.contains("..")
        || branch.ends_with('/')
        || branch
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c));
    if bad {
        return Err(format!("`{branch}` is not a branch name"));
    }
    Ok(())
}

/// The body of a new pull request: what it is and its commits.
pub fn body(branch: &str, base: &str, commits: &[(String, String)]) -> String {
    let mut s = format!(
        "Branch `{branch}` against `{base}`, {} commit(s). Opened by `./dev pr open`: this pull request is the branch's \
         ledger entry (its labels, from .gitea/labels.toml, say where it is; docs/plans/merge-queue.md). Opening it starts no CI \
         run: no workflow listens on pull requests (the contract test no_workflow_runs_on_a_pull_request).\n\n",
        commits.len()
    );
    for (id, subject) in commits.iter().take(MAX_BODY_COMMITS) {
        s.push_str(&format!("- {id} {subject}\n"));
    }
    if commits.len() > MAX_BODY_COMMITS {
        s.push_str(&format!("- ... and {} more\n", commits.len() - MAX_BODY_COMMITS));
    }
    s
}

fn ids_of(api: &mut Api, names: &[String]) -> Result<Vec<u64>, String> {
    let server = api.labels()?;
    names
        .iter()
        .map(|n| {
            server
                .iter()
                .find(|l| &l.name == n)
                .map(|l| l.id)
                .ok_or_else(|| format!("the server has no label `{n}`; run `./dev pr sync` first"))
        })
        .collect()
}

fn open_pull(api: &mut Api, branch: &str) -> Result<Option<Pull>, String> {
    let mut found: Vec<Pull> = api.pulls("open")?.into_iter().filter(|p| p.branch == branch).collect();
    if found.len() > 1 {
        let n: Vec<String> = found.iter().map(|p| format!("#{}", p.number)).collect();
        return Err(format!(
            "{} open pull requests for `{branch}` ({}); close all but one",
            found.len(),
            n.join(", ")
        ));
    }
    Ok(found.pop())
}

/// `pr open`: a draft pull request of `branch` against `base`, labelled `extra` (plus `state/built` when no `state/` label is
/// given). Refuses a branch that is not pushed, has nothing to merge, or already has an open pull request.
pub fn open(api: &mut Api, git: &dyn Git, known: &[LabelSpec], branch: &str, base: &str, extra: &[String]) -> Result<Value, String> {
    check_branch(branch)?;
    check_branch(base)?;
    git.fetch(&[base, branch])?;
    let head = git
        .remote_head(branch)?
        .ok_or_else(|| format!("origin/{branch} is not known here; push the branch (git push -u origin {branch}) and fetch"))?;
    let commits = git.commits(base, branch)?;
    if commits.is_empty() {
        return Err(format!(
            "origin/{branch} has no commit that origin/{base} lacks; there is nothing to merge"
        ));
    }
    if let Some(p) = open_pull(api, branch)? {
        return Err(format!("`{branch}` already has an open pull request: #{} {}", p.number, p.url));
    }
    let mut changes: Vec<Change> = extra.iter().map(|n| Change::Add(n.clone())).collect();
    if !extra.iter().any(|n| labels::scope_of(n) == Some("state")) {
        changes.insert(0, Change::Add("state/built".into()));
    }
    let names = labels::apply(&[], &changes, known)?;
    let ids = ids_of(api, &names)?;
    let title = format!("WIP: {branch}");
    let pr = api.create_pull(branch, base, &title, &body(branch, base, &commits), &ids)?;
    Ok(json!({
        "number": pr.number, "url": pr.url, "branch": branch, "head": head, "draft": pr.draft,
        "labels": names, "commits": commits.len(),
    }))
}

/// `pr label`: applies `+x -y` to the open pull request of `branch`.
pub fn label(api: &mut Api, known: &[LabelSpec], branch: &str, words: &[String]) -> Result<Value, String> {
    check_branch(branch)?;
    let changes = labels::parse_changes(words)?;
    let pr =
        open_pull(api, branch)?.ok_or_else(|| format!("`{branch}` has no open pull request; open one with `./dev pr open {branch}`"))?;
    let after = labels::apply(&pr.labels, &changes, known)?;
    let ids = ids_of(api, &after)?;
    let now = api.set_labels(pr.number, &ids)?;
    Ok(json!({"number": pr.number, "branch": branch, "before": pr.labels, "labels": now}))
}

/// The short CI line of a commit's runs.
pub fn ci_summary(runs: &[CommitRun]) -> String {
    if runs.is_empty() {
        return "none".into();
    }
    runs.iter()
        .map(|r| format!("{} {} {}", r.workflow.trim_end_matches(".yml"), r.id, r.result))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `pr list`: the open pull requests (only those labelled `state/<state>` when given), with the CI result of each head.
pub fn list(api: &mut Api, state: Option<&str>, with_ci: bool) -> Result<Vec<Value>, String> {
    let want = state.map(|s| {
        if s.starts_with("state/") {
            s.to_string()
        } else {
            format!("state/{s}")
        }
    });
    let mut rows = Vec::new();
    for p in api.pulls("open")? {
        if want.as_ref().is_some_and(|w| !p.labels.contains(w)) {
            continue;
        }
        let ci = if with_ci && !p.head.is_empty() {
            ci_summary(&api.commit_runs(&p.head)?)
        } else {
            "not read".into()
        };
        rows.push(json!({
            "number": p.number, "branch": p.branch, "head": p.head, "labels": p.labels, "ci": ci, "draft": p.draft, "url": p.url,
        }));
    }
    Ok(rows)
}

/// A padded text table of `pr list`'s rows.
pub fn list_text(rows: &[Value]) -> String {
    let cells: Vec<[String; 5]> = rows
        .iter()
        .map(|r| {
            let labels: Vec<&str> = r["labels"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            [
                format!("#{}", r["number"]),
                r["branch"].as_str().unwrap_or("").to_string(),
                r["head"].as_str().unwrap_or("").chars().take(10).collect(),
                labels.join(","),
                r["ci"].as_str().unwrap_or("").to_string(),
            ]
        })
        .collect();
    let head = ["PR", "BRANCH", "HEAD", "LABELS", "LAST CI"];
    let mut width = head.map(str::len);
    for c in &cells {
        for (w, v) in width.iter_mut().zip(c) {
            *w = (*w).max(v.chars().count());
        }
    }
    let line = |c: &[String]| {
        let mut s = String::new();
        for (i, v) in c.iter().enumerate() {
            if i + 1 == c.len() {
                s.push_str(v);
            } else {
                s.push_str(&format!("{v:<w$}  ", w = width[i]));
            }
        }
        s.trim_end().to_string() + "\n"
    };
    let mut out = line(&head.map(str::to_string));
    for c in &cells {
        out.push_str(&line(c));
    }
    if cells.is_empty() {
        out.push_str("(no open pull requests match)\n");
    }
    out
}

/// `pr sync`: creates the labels the server lacks and updates a changed colour, description or exclusivity. Never deletes;
/// labels the file does not list are reported as `extra`.
pub fn sync(api: &mut Api, known: &[LabelSpec], dry_run: bool) -> Result<Value, String> {
    let autodetect = api.autodetects_manual_merge()?;
    let warning = match autodetect {
        Some(true) => None,
        Some(false) => Some(AUTODETECT_OFF.to_string()),
        None => Some("the server did not say whether \"autodetect manual merge\" is on (not measured)".to_string()),
    };
    let server = api.labels()?;
    let (mut created, mut updated) = (Vec::new(), Vec::new());
    for k in known {
        match server.iter().find(|s| s.name == k.name) {
            None => {
                if !dry_run {
                    api.create_label(&k.name, &k.color, &k.description, k.exclusive)?;
                }
                created.push(k.name.clone());
            }
            Some(s) if s.color != k.color || s.description != k.description || s.exclusive != k.exclusive => {
                if !dry_run {
                    api.edit_label(s.id, &k.color, &k.description, k.exclusive)?;
                }
                updated.push(k.name.clone());
            }
            Some(_) => {}
        }
    }
    let extra: Vec<String> = server
        .iter()
        .filter(|s| !known.iter().any(|k| k.name == s.name))
        .map(|s| s.name.clone())
        .collect();
    Ok(
        json!({"dry_run": dry_run, "created": created, "updated": updated, "extra": extra, "unchanged": known.len() - created.len() - updated.len(), "autodetect_manual_merge": autodetect, "warning": warning}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pr::fake::FakeGitea;
    use std::collections::HashMap;

    struct FakeGit {
        heads: HashMap<String, String>,
        commits: Vec<(String, String)>,
        fetched: std::cell::RefCell<Vec<String>>,
    }

    impl Git for FakeGit {
        fn remote_head(&self, branch: &str) -> Result<Option<String>, String> {
            Ok(self.heads.get(branch).cloned())
        }
        fn commits(&self, _base: &str, _branch: &str) -> Result<Vec<(String, String)>, String> {
            Ok(self.commits.clone())
        }
        fn on_base(&self, _commit: &str, _base: &str) -> Result<Option<bool>, String> {
            Ok(None)
        }
        fn fetch(&self, branches: &[&str]) -> Result<(), String> {
            self.fetched.borrow_mut().extend(branches.iter().map(|b| b.to_string()));
            Ok(())
        }
    }

    fn known() -> Vec<LabelSpec> {
        let file = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.gitea/labels.toml")).unwrap();
        labels::parse(&file, &["dyno".into()]).unwrap()
    }

    fn synced() -> FakeGitea {
        let mut f = FakeGitea::default();
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        sync(&mut api, &known(), false).unwrap();
        f
    }

    fn git() -> FakeGit {
        FakeGit {
            heads: HashMap::from([("feat/x".to_string(), "abc1234def".to_string())]),
            commits: vec![("abc1234".into(), "feat: one".into()), ("def5678".into(), "fix: two".into())],
            fetched: Default::default(),
        }
    }

    #[test]
    fn sync_creates_the_missing_labels_updates_changed_ones_and_never_deletes() {
        let mut f = FakeGitea::default();
        f.label("state/built", "#ffffff", true); // wrong colour
        f.label("legacy", "#000000", false);
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        let dry = sync(&mut api, &known(), true).unwrap();
        assert_eq!(dry["updated"], json!(["state/built"]));
        assert_eq!(dry["extra"], json!(["legacy"]));
        assert!(
            f.requests.iter().all(|r| r.starts_with("GET ")),
            "a dry run writes nothing: {:?}",
            f.requests
        );
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        let done = sync(&mut api, &known(), false).unwrap();
        assert_eq!(done["created"].as_array().unwrap().len(), known().len() - 1);
        assert!(
            f.requests
                .iter()
                .any(|r| r == "PATCH https://gitea.invalid/api/v1/repos/o/r/labels/1")
        );
        assert!(!f.requests.iter().any(|r| r.starts_with("DELETE")));
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        let again = sync(&mut api, &known(), false).unwrap();
        assert_eq!((again["created"].clone(), again["updated"].clone()), (json!([]), json!([])));
    }

    #[test]
    fn open_makes_a_draft_against_main_with_the_commits_and_state_built() {
        let mut f = synced();
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        let v = open(
            &mut api,
            &git(),
            &known(),
            "feat/x",
            "main",
            &["owes/windows-tests".into(), "app/dyno".into()],
        )
        .unwrap();
        assert_eq!(v["labels"], json!(["state/built", "owes/windows-tests", "app/dyno"]));
        assert_eq!(v["draft"], true);
        let req = f.bodies.last().unwrap();
        assert_eq!((req["head"].as_str(), req["base"].as_str()), (Some("feat/x"), Some("main")));
        assert!(req["title"].as_str().unwrap().starts_with("WIP: "), "Gitea's draft prefix");
        let body = req["body"].as_str().unwrap();
        assert!(
            body.contains("- abc1234 feat: one\n- def5678 fix: two\n") && body.contains("no_workflow_runs_on_a_pull_request"),
            "{body}"
        );
    }

    #[test]
    fn open_with_a_state_label_does_not_add_state_built() {
        let mut f = synced();
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        let v = open(&mut api, &git(), &known(), "feat/x", "main", &["state/approved".into()]).unwrap();
        assert_eq!(v["labels"], json!(["state/approved"]));
    }

    #[test]
    fn open_refuses_an_unpushed_branch_an_empty_one_a_second_pr_a_bad_name_and_unsynced_labels() {
        let mut f = synced();
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        assert!(
            open(&mut api, &git(), &known(), "feat/y", "main", &[])
                .unwrap_err()
                .contains("git push -u origin feat/y")
        );
        assert!(
            open(&mut api, &git(), &known(), "--upload-pack=x", "main", &[])
                .unwrap_err()
                .contains("not a branch name")
        );
        let empty = FakeGit { commits: vec![], ..git() };
        assert!(
            open(&mut api, &empty, &known(), "feat/x", "main", &[])
                .unwrap_err()
                .contains("nothing to merge")
        );
        open(&mut api, &git(), &known(), "feat/x", "main", &[]).unwrap();
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        assert!(
            open(&mut api, &git(), &known(), "feat/x", "main", &[])
                .unwrap_err()
                .contains("already has an open pull request: #1")
        );
        let mut bare = FakeGitea::default();
        let mut api = Api::new(&mut bare, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        assert!(
            open(&mut api, &git(), &known(), "feat/x", "main", &[])
                .unwrap_err()
                .contains("./dev pr sync")
        );
        assert!(
            !bare.requests.iter().any(|r| r.starts_with("POST")),
            "nothing is created before the labels resolve"
        );
    }

    #[test]
    fn label_moves_the_state_and_replaces_the_set() {
        let mut f = synced();
        f.pull(
            7,
            "feat/x",
            "abc",
            "open",
            None,
            &["state/in-review", "owes/windows-tests", "review-round/1"],
        );
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        let words: Vec<String> = ["+state/approved", "-owes/windows-tests", "+review-round/2"]
            .map(String::from)
            .to_vec();
        let v = label(&mut api, &known(), "feat/x", &words).unwrap();
        assert_eq!(v["labels"], json!(["state/approved", "review-round/2"]));
        assert_eq!(v["before"], json!(["state/in-review", "owes/windows-tests", "review-round/1"]));
        assert!(
            f.requests
                .last()
                .unwrap()
                .starts_with("PUT https://gitea.invalid/api/v1/repos/o/r/issues/7/labels")
        );
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        assert!(
            label(&mut api, &known(), "feat/none", &words)
                .unwrap_err()
                .contains("./dev pr open feat/none")
        );
    }

    #[test]
    fn list_filters_by_state_and_reads_the_newest_run_per_workflow() {
        let mut f = synced();
        f.pull(1, "feat/a", "aaaa1111", "open", None, &["state/approved"]);
        f.pull(2, "feat/b", "bbbb2222", "open", None, &["state/built"]);
        f.pull(3, "feat/c", "cccc3333", "closed", Some("cccc3333"), &["state/landed"]);
        f.runs.push((
            "aaaa1111".into(),
            vec![
                json!({"id": 9, "path": "ci.yml@refs/heads/feat/a", "status": "completed", "conclusion": "failure"}),
                json!({"id": 8, "path": "ci.yml@refs/heads/feat/a", "status": "completed", "conclusion": "success"}),
                json!({"id": 7, "path": "debrief.yml@refs/heads/feat/a", "status": "running", "conclusion": ""}),
            ],
        ));
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        let rows = list(&mut api, Some("approved"), true).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["ci"], "ci 9 failure, debrief 7 running");
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        let all = list(&mut api, None, true).unwrap();
        assert_eq!(all.len(), 2, "closed pull requests are not listed");
        assert_eq!(all[1]["ci"], "none");
        let text = list_text(&all);
        assert!(text.starts_with("PR  BRANCH  HEAD"), "{text}");
        assert!(text.contains("#1  feat/a  aaaa1111  state/approved  ci 9 failure"), "{text}");
    }

    #[test]
    fn a_long_listing_is_paged_and_bounded() {
        let mut f = synced();
        for n in 1..=120 {
            f.pull(n, &format!("feat/{n}"), "ab", "open", None, &[]);
        }
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        assert_eq!(list(&mut api, None, false).unwrap().len(), 120);
        assert!(f.requests.iter().any(|r| r.ends_with("pulls?state=open&limit=50&page=3")));
    }

    #[test]
    fn sync_warns_when_the_server_does_not_detect_fast_forward_merges() {
        for (setting, warns) in [(Some(true), false), (Some(false), true), (None, true)] {
            let mut f = FakeGitea {
                autodetect: setting,
                ..Default::default()
            };
            let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
            let v = sync(&mut api, &known(), true).unwrap();
            assert_eq!(v["warning"].is_string(), warns, "{setting:?}: {v}");
            assert_eq!(v["autodetect_manual_merge"], json!(setting));
        }
    }

    #[test]
    fn open_fetches_the_base_and_the_branch_before_reading_them() {
        let mut f = synced();
        let g = git();
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        open(&mut api, &g, &known(), "feat/x", "main", &[]).unwrap();
        assert_eq!(*g.fetched.borrow(), ["main", "feat/x"]);
    }

    #[test]
    fn a_server_with_a_smaller_page_size_is_read_to_the_empty_page() {
        let mut f = synced();
        f.max_page = Some(30);
        for n in 1..=70 {
            f.pull(n, &format!("feat/{n}"), "ab", "open", None, &[]);
        }
        let mut api = Api::new(&mut f, FakeGitea::SERVER, FakeGitea::REPO, "secret");
        assert_eq!(
            list(&mut api, None, false).unwrap().len(),
            70,
            "a page shorter than asked is not the last one"
        );
    }
}
