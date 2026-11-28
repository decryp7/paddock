//! The member's own CI trial, dispatched by `./dev merge sync-member` once the static rows passed (user decision 90, 2026-11-26).
//!
//! The failure it prevents (three trains): members that passed on the Mac failed CI only on Windows or Linux, found at the train's
//! run. The trial is the member's narrowed run on the train, through the command that already exists for it: `./dev ci dispatch
//! ci.yml --trial <member> --onto <train> --max <duration>` merges `origin/<member>` onto the train in the tool's own clone, runs the
//! merge check on it, pushes `trial/<member>` and dispatches ci.yml with `base` = the train tip, so the plan's affected jobs run
//! (and only them), on the legs the plan names. It prints `run <id>`; this step prints it too. `./dev merge check --gates` reads the
//! run later ([`super::member_trial`]).
//!
//! The trial merges what origin holds, so the sync tip is first pushed there as a NEW scratch ref named by the synced commit,
//! `sync/<member>/<short sha>` (the integrator's role: the integrator runs sync-member and may push shared branches; the member's own
//! branch stays the author's) and the trial is of that ref (`trial/sync/<member>/<short sha>`). A re-sync is a new commit, so a new ref:
//! every push is a plain push of a ref that does not exist yet, never a force or a lease (the guard hook refuses both). The push runs
//! through the one runner of pushes that need the caller's credentials ([`crate::queue::real::GitTrain`], `Child::Authed`, as `ci
//! dispatch --trial` pushes `trial/<branch>`): the token reaches that one child through the scoped askpass program, and the merge
//! check's scrubbed git reader is never used for a push. The dispatch needs `PADDOCK_TRIAL_DIR` (the trial's scratch folder, which
//! `ci dispatch --trial` reads from the environment); it is read once, before the push, and handed to the dispatch child. A push that
//! fails, or a missing `PADDOCK_TRIAL_DIR` or `GITEA_TOKEN`, is NOT MEASURED, nothing dispatched.
//!
//! `--no-trial "<reason>"` skips it and logs the reason, like `--old-base-anyway`.

use super::gates::GateRun;
use crate::git::Git;
use crate::queue::real::{Child, GitTrain};
use crate::trailers::split_sync_ref;
use paddock_cli::Exit;
use paddock_proc::{Limits, command, run_bounded};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The default of `--trial-max`: the longest the dispatch waits for a busy runner.
pub const DEFAULT_MAX: &str = "30m";

/// The dispatch waits for the runner at most `--trial-max` and then ends itself; this is the outer hang detector, wide beyond any
/// sensible `--trial-max`, not a speed limit.
const TIMEOUT: Duration = Duration::from_secs(4 * 60 * 60);

/// Starts the trial.
pub trait Dispatcher {
    /// `ci dispatch ci.yml --trial <member> --onto <train> --max <max>` in `root`, with `PADDOCK_TRIAL_DIR` = `trial_dir` and one
    /// `--reland <revert>` for each of `relands` (the full shas of the train's reverts of this member's own merges that the sync tip
    /// re-applies: the trial's merge carries `Reverted:` lines for them); an error is a failure to start.
    fn dispatch(&self, root: &Path, trial_dir: &Path, member: &str, train: &str, max: &str, relands: &[String]) -> Result<GateRun, String>;
}

/// Pushes the sync tip.
pub trait Pusher {
    /// `git push origin HEAD:refs/heads/<reference>` in `root` with the caller's credentials; `trial_dir` is the scratch folder of the
    /// askpass program and `hold` the longest the push waits while the server is down. An error is git's own words.
    fn push(&self, root: &Path, trial_dir: &Path, reference: &str, hold: &str) -> Result<(), String>;

    /// The refs on origin that would clash with `sync/<member>/<sha>` and `trial/sync/<member>/<sha>`: an old flat `sync/<member>` or
    /// `trial/sync/<member>` (git cannot hold a ref and a folder of the same name). Empty when none.
    fn flat_refs(&self, root: &Path, trial_dir: &Path, member: &str) -> Result<Vec<String>, String>;
}

/// The real pusher: the runner `ci dispatch --trial` pushes with.
pub struct RealPush;

impl Pusher for RealPush {
    fn push(&self, root: &Path, trial_dir: &Path, reference: &str, hold: &str) -> Result<(), String> {
        let (url, token) = origin_and_token(root)?;
        push_with(root, &url, trial_dir, reference, hold, token)
    }

    fn flat_refs(&self, root: &Path, trial_dir: &Path, member: &str) -> Result<Vec<String>, String> {
        let (url, token) = origin_and_token(root)?;
        flat_refs_with(root, &url, trial_dir, member, token)
    }
}

/// The origin's url and the token, for a runner of the caller's own credentials.
fn origin_and_token(root: &Path) -> Result<(String, Option<String>), String> {
    let url = Git::new(root)
        .text(&["remote", "get-url", "origin"])
        .map(|t| t.trim().to_string())?;
    Ok((url, std::env::var("GITEA_TOKEN").ok().filter(|t| !t.is_empty())))
}

/// [`RealPush::flat_refs`] with the origin's url and the token given: one `ls-remote` through the shared runner.
fn flat_refs_with(root: &Path, url: &str, trial_dir: &Path, member: &str, token: Option<String>) -> Result<Vec<String>, String> {
    if url.contains("://") && token.is_none() {
        return Err("GITEA_TOKEN is not set (reading origin needs the token `ci dispatch --trial` uses)".to_string());
    }
    std::fs::create_dir_all(trial_dir).map_err(|e| format!("cannot create {}: {e}", trial_dir.display()))?;
    let g = GitTrain::for_checkout(root, url, trial_dir, token);
    let (sync, trial) = (format!("refs/heads/sync/{member}"), format!("refs/heads/trial/sync/{member}"));
    let (ok, out) = g.run_git(Child::Authed, &["ls-remote", "--heads", "origin", &sync, &trial])?;
    if !ok {
        return Err(out.trim().to_string());
    }
    Ok(out
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .filter(|r| *r == sync || *r == trial)
        .filter_map(|r| r.strip_prefix("refs/heads/"))
        .map(str::to_string)
        .collect())
}

/// [`RealPush::push`] with the token and the origin's url given.
fn push_with(root: &Path, url: &str, trial_dir: &Path, reference: &str, hold: &str, token: Option<String>) -> Result<(), String> {
    if url.contains("://") && token.is_none() {
        return Err("GITEA_TOKEN is not set (the push to origin needs the token `ci dispatch --trial` uses)".to_string());
    }
    std::fs::create_dir_all(trial_dir).map_err(|e| format!("cannot create {}: {e}", trial_dir.display()))?;
    let mut g = GitTrain::for_checkout(root, url, trial_dir, token);
    g.push_hold = crate::wait::parse_duration(hold).ok();
    let (ok, out) = g.run_git(
        Child::Authed,
        &["push", "--quiet", "origin", &format!("HEAD:refs/heads/{reference}")],
    )?;
    if ok { Ok(()) } else { Err(out.trim().to_string()) }
}

/// The trial's scratch folder, through the one resolver `ci dispatch --trial` uses ([`crate::places::trial_dir`]): resolved here first
/// so an unresolvable one is refused before anything is pushed, and handed to the dispatch child.
pub fn trial_dir(root: &Path) -> Result<PathBuf, String> {
    crate::places::trial_dir(root, None, &|k| std::env::var(k).ok()).map_err(|e| format!("{} {}", e.message, e.hint))
}

/// The real dispatcher (this tool, or `PADDOCK_SYNC_TRIAL_DISPATCH`: the tests' stand-in).
pub struct Real;

impl Dispatcher for Real {
    fn dispatch(&self, root: &Path, trial_dir: &Path, member: &str, train: &str, max: &str, relands: &[String]) -> Result<GateRun, String> {
        let program = match std::env::var_os("PADDOCK_SYNC_TRIAL_DISPATCH") {
            Some(p) => p.into(),
            None => crate::tool_cache::self_exe().map_err(|e| format!("this program cannot be found: {e}"))?,
        };
        let mut cmd = command(program);
        cmd.args([
            "ci", "dispatch", "ci.yml", "--trial", member, "--onto", train, "--max", max, "--root",
        ])
        .arg(root);
        for r in relands {
            cmd.args(["--reland", r]);
        }
        cmd.env("PADDOCK_TRIAL_DIR", trial_dir).current_dir(root);
        let out = run_bounded(cmd, Limits::new(TIMEOUT)).map_err(|e| format!("`ci dispatch` could not run: {e}"))?;
        let code = match out.outcome {
            paddock_proc::Outcome::Exited(st) => st.code(),
            _ => None,
        };
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        Ok(GateRun {
            code,
            text: format!("{}\n{}", stdout, out.stderr_text()).trim().to_string(),
            wire: None,
        })
    }
}

/// What a step of the trial says: lines, data, exit.
pub struct Step {
    /// The lines.
    pub text: Vec<String>,
    /// The structured answer.
    pub data: serde_json::Value,
    /// `Ok`, or `NotMeasured` (a trial that was not started is not a pass).
    pub exit: Exit,
}

/// The line printed when `--no-trial` skips the trial.
pub fn skipped(reason: &str) -> Step {
    Step {
        text: vec![format!(
            "::warning::merge sync-member: no CI trial dispatched (--no-trial: {reason})"
        )],
        data: json!({"outcome": "skipped", "reason": reason}),
        exit: Exit::Ok,
    }
}

/// Said out loud when there is nothing to dispatch yet; not a failure.
pub fn not_run(why: &str) -> Step {
    Step {
        text: vec![format!("CI trial not dispatched ({why})")],
        data: json!({"outcome": "not run", "why": why}),
        exit: Exit::Ok,
    }
}

/// The member a branch name stands for: without a remote prefix, and for a sync branch (`sync/<member>`, `sync/<member>/<sha>`) the
/// member it carries. The integrator works on `sync/<member>`; the train's merge subjects name `<member>`.
pub fn member_key(name: &str) -> &str {
    let n = name.strip_prefix("origin/").unwrap_or(name);
    match split_sync_ref(n) {
        Some((member, _)) => member,
        None => n.strip_prefix("sync/").unwrap_or(n),
    }
}

/// An old flat ref on origin blocks the new one: nothing is pushed.
pub fn flat_ref_blocks(old: &[String], synced: &str) -> Step {
    let why = format!(
        "origin holds the old flat ref {} and git cannot hold {synced} beside it",
        old.join(", ")
    );
    Step {
        text: vec![format!(
            "::error::merge sync-member: CI trial NOT DISPATCHED: {why}. Next step: delete the old ref(s) once their member has landed (`./dev ci trial-prune` deletes the flat refs of landed members; `git push origin --delete {}` otherwise), then run this again; or --no-trial \"<reason>\".",
            old.join(" ")
        )],
        data: json!({"outcome": "not measured", "why": why, "refs": old}),
        exit: Exit::NotMeasured,
    }
}

/// The sync tip could not be pushed: nothing is dispatched.
pub fn not_pushed(synced: &str, why: &str) -> Step {
    let why = format!("the sync tip could not be pushed to origin as {synced}: {}", why.trim());
    Step {
        text: vec![format!(
            "::error::merge sync-member: CI trial NOT DISPATCHED: {why}. Next step: fix the push (the integrator role pushes the new ref `sync/<member>/<short sha>`, a plain push: a refusal as `rejected` means that name already holds another commit, so `git fetch origin` and look; otherwise check GITEA_TOKEN, the remote and your access), then run this again; or --no-trial \"<reason>\"."
        )],
        data: json!({"outcome": "not measured", "why": why}),
        exit: Exit::NotMeasured,
    }
}

/// The run id a dispatch printed: the last line that is exactly `run <digits>`.
pub fn run_id(text: &str) -> Option<u64> {
    text.lines()
        .rev()
        .filter_map(|l| l.trim().strip_prefix("run "))
        .find_map(|n| n.trim().parse().ok())
}

/// The step for what the dispatch answered: the run id on a dispatch that exited 0 and printed one; anything else is not a started trial.
pub fn interpret(answer: Result<GateRun, String>, member: &str, train: &str) -> Step {
    let fail = |why: String, tail: &str| Step {
        text: vec![
            tail.to_string(),
            format!(
                "::error::merge sync-member: CI trial NOT DISPATCHED: {why}. Next step: read the lines above, fix that on the member, push and re-sync; or run `./dev ci dispatch ci.yml --trial {member} --onto {train} --max {DEFAULT_MAX}` yourself; or --no-trial \"<reason>\". A member with no trial is not a member that passed."
            ),
        ],
        data: json!({"outcome": "not measured", "why": why}),
        exit: Exit::NotMeasured,
    };
    let run = match answer {
        Err(why) => return fail(why, ""),
        Ok(r) => r,
    };
    match (run.code, run_id(&run.text)) {
        (Some(0), Some(id)) => Step {
            text: vec![format!(
                "CI trial dispatched: run {id} (the affected jobs of {member} on {train}); read it with `./dev merge check --gates` or `./dev wait --ci-run {id} --max <duration>`"
            )],
            data: json!({"outcome": "dispatched", "run": id}),
            exit: Exit::Ok,
        },
        (Some(0), None) => fail(
            "`ci dispatch` exited 0 but printed no `run <id>` line (an exit code alone proves nothing)".into(),
            &run.text,
        ),
        (code, _) => fail(
            code.map_or("`ci dispatch` was ended before it finished".to_string(), |c| {
                format!("`ci dispatch` exited {c}")
            }),
            &run.text,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trailers::sync_ref;

    fn ran(code: i32, text: &str) -> Result<GateRun, String> {
        Ok(GateRun {
            code: Some(code),
            text: text.into(),
            wire: None,
        })
    }

    #[test]
    fn the_run_id_is_the_last_run_line() {
        assert_eq!(run_id("waiting\nrun 8123\n"), Some(8123));
        assert_eq!(run_id("run 1\nrun 2"), Some(2));
        assert_eq!(run_id("running tests\nrun abc"), None);
        assert_eq!(run_id(""), None);
    }

    #[test]
    fn a_dispatch_that_printed_a_run_is_started_and_names_it() {
        let s = interpret(ran(0, "guards ok\nrun 8123"), "feat/x", "origin/train/t");
        assert_eq!(s.exit, Exit::Ok);
        assert!(s.text.join("\n").contains("run 8123"), "{:?}", s.text);
        assert_eq!(s.data["run"], 8123);
    }

    /// Positive control (ER-33): no run line, a failing dispatch and a dispatch that could not start are not a started trial.
    #[test]
    fn a_trial_that_was_not_started_is_not_a_pass() {
        for a in [
            ran(0, "guards ok"),
            ran(5, "::error::conflict in a.txt"),
            ran(1, "run 9"),
            Err("no tool".into()),
        ] {
            let s = interpret(a, "feat/x", "origin/train/t");
            assert_eq!(s.exit, Exit::NotMeasured);
            assert!(s.text.join("\n").contains("NOT DISPATCHED"), "{:?}", s.text);
        }
    }

    #[test]
    fn a_tip_that_cannot_be_pushed_dispatches_nothing() {
        assert_eq!(
            sync_ref("feat/x", "0123456789abcdef0123456789abcdef01234567"),
            "sync/feat/x/012345678"
        );
        let s = not_pushed("sync/feat/x/012345678", "permission denied");
        assert_eq!(s.exit, Exit::NotMeasured);
        assert!(s.text.join("\n").contains("NOT DISPATCHED") && s.text.join("\n").contains("permission denied"));
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = crate::git_env::git().arg("-C").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// The push runs through the shared runner: the token reaches that one child through the scoped askpass program (so a remote that
    /// asks for credentials gets them from the environment), `GITEA_TOKEN` itself is never in the child, and a new ref is a plain push.
    #[test]
    fn the_push_child_keeps_the_callers_credentials_and_a_new_ref_needs_no_force() {
        let tmp = tempfile::tempdir().unwrap();
        let (work, bare, scratch) = (tmp.path().join("work"), tmp.path().join("origin.git"), tmp.path().join("trial"));
        std::fs::create_dir(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        git(&work, &["init", "-q", "--bare", bare.to_str().unwrap()]);
        git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
        std::fs::write(work.join("f"), "1\n").unwrap();
        git(&work, &["add", "f"]);
        git(
            &work,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                "a",
            ],
        );
        let url = bare.to_str().unwrap();
        push_with(&work, url, &scratch, "sync/m/aaaaaaaaa", "1m", Some("tok".into())).unwrap();
        assert_eq!(
            git(&bare, &["rev-parse", "refs/heads/sync/m/aaaaaaaaa"]),
            git(&work, &["rev-parse", "HEAD"])
        );
        assert!(
            scratch.join("askpass.sh").exists(),
            "the credentials come through the shared askpass program"
        );
        // The child's environment: the token only through the askpass pair, never as GITEA_TOKEN.
        let g = GitTrain::for_checkout(&work, url, &scratch, Some("tok".into()));
        let c = g.command(None, Child::Authed);
        let env: std::collections::HashMap<_, _> = c
            .get_envs()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned())))
            .collect();
        assert_eq!(env.get("PADDOCK_QUEUE_PUSH_TOKEN"), Some(&Some("tok".into())));
        assert!(
            env.get("GIT_ASKPASS")
                .is_some_and(|v| v.as_deref().is_some_and(|p| p.ends_with("askpass.sh")))
        );
        assert_eq!(env.get("GITEA_TOKEN"), Some(&None), "GITEA_TOKEN is removed from the child");
        // A different commit under a name that is taken is refused, not forced.
        let commit = |msg: &str| {
            std::fs::write(work.join("f"), msg).unwrap();
            git(
                &work,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-q",
                    "-am",
                    msg,
                ],
            );
        };
        commit("b");
        push_with(&work, url, &scratch, "sync/m/bbbbbbbbb", "1m", None).unwrap();
        let taken = git(&bare, &["rev-parse", "refs/heads/sync/m/bbbbbbbbb"]);
        git(&work, &["reset", "-q", "--hard", "HEAD~1"]);
        commit("c");
        assert!(push_with(&work, url, &scratch, "sync/m/bbbbbbbbb", "1m", None).is_err());
        assert_eq!(git(&bare, &["rev-parse", "refs/heads/sync/m/bbbbbbbbb"]), taken);
    }

    #[test]
    fn a_member_key_drops_the_remote_and_the_sync_prefix_and_sha() {
        for (name, key) in [
            ("feat/x", "feat/x"),
            ("origin/feat/x", "feat/x"),
            ("sync/feat/x", "feat/x"),
            ("sync/feat/x/012345678", "feat/x"),
            ("origin/sync/feat/x", "feat/x"),
        ] {
            assert_eq!(member_key(name), key, "{name}");
        }
    }

    /// An old flat ref (or flat trial ref) on origin is found, and its absence is an empty answer, not a pass of a blind check.
    #[test]
    fn the_old_flat_refs_of_a_member_are_found_on_origin_and_only_those() {
        let tmp = tempfile::tempdir().unwrap();
        let (work, bare, scratch) = (tmp.path().join("work"), tmp.path().join("origin.git"), tmp.path().join("trial"));
        std::fs::create_dir(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        git(&work, &["init", "-q", "--bare", bare.to_str().unwrap()]);
        git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
        std::fs::write(work.join("f"), "1\n").unwrap();
        git(&work, &["add", "f"]);
        git(
            &work,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                "a",
            ],
        );
        let url = bare.to_str().unwrap();
        assert!(flat_refs_with(&work, url, &scratch, "feat/x", None).unwrap().is_empty());
        for r in ["sync/feat/x", "trial/sync/feat/x", "sync/feat/y"] {
            git(&work, &["push", "-q", "origin", &format!("HEAD:refs/heads/{r}")]);
        }
        let mut found = flat_refs_with(&work, url, &scratch, "feat/x", None).unwrap();
        found.sort_unstable();
        assert_eq!(found, ["sync/feat/x", "trial/sync/feat/x"]);
    }

    #[test]
    fn the_escape_is_logged_with_its_reason() {
        let s = skipped("docs-only");
        assert_eq!(s.exit, Exit::Ok);
        assert!(s.text.join("\n").contains("--no-trial: docs-only"));
    }
}
