//! The trial push profile of `./dev check --pre-push` (ER-9 overlap; docs/adr/0114-the-trial-push-profile-starts-ci-before-the-slow-local-rows-finish.md).
//!
//! A push whose every ref is under `wip/` or `trial/` runs only the fast static rows before the push ([`Phase::Fast`]) and hands the
//! slow rows to one background process ([`Phase::Slow`]) that outlives the hook, so CI for the ref can start at once. A push of any
//! other ref (a `train/*` branch, `main`, a tag, a feature branch) or a mixed push keeps the full gate: [`phase_for`] says
//! [`Phase::Full`] for it and nothing here touches it.
//!
//! The split is by row id and by an allow-list of the FAST rows: a row that is not named fast is slow, so a row added later runs in
//! the background, never silently nowhere. Measured row times (the gate's own `step-history.jsonl`, 2415 steps, 2026-11-22, passed
//! warm steps, median / 90th percentile in seconds): fmt 3.1 / 9.7, lockfile 1.5 / 7.1, commit-lint 1.5, conformance 46.8 / 131,
//! gates-register 7.3 / 17, ipc-endpoints 3.0 / 9.8, pitwall-deny 1.7 / 3.1, pitwall-scripts 1.5 / 4.0, clippy (host) 6.8 / 42
//! warm (143 / 449 cold) are FAST; conformance-tests 92.7 / 237, doctest 38.6 / 202, clippy-linux 18.8 / 66 (up to 710),
//! clippy-windows 14.8 / 99 warm (346 / 859 cold), dyno-wire-fixtures 82, member-audits 36 / 60 are SLOW. The fast set is about
//! 70 s of the median push (conformance dominates), the slow set about 250 s plus the compile queue behind other agents' heavy
//! steps, which is what stalled pushes for hours.

use super::exec::{Exec, Kind, Row};
use super::gates::Planned;
use crate::trial_guard::{self, Slow, State};
use clap::ValueEnum;
use std::path::Path;
use std::time::Duration;

/// Which rows a run takes. Named by the hook's process and its background child; a person never picks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Phase {
    /// The fast rows of a trial push, before the push.
    Fast,
    /// The slow rows of a trial push, in the background after it.
    Slow,
}

/// Which phase a run is in, as the full gate is the case that is not named: `given` is the hidden `--split-phase` of the background
/// child; otherwise the hook's push decides. Either way the push must be [`covers`]ed: a phase named for a `train/*` or `main` push
/// is not honoured (the full gate runs), so no flag reaches a landing. `None` is the full gate.
pub fn phase_for(given: Option<Phase>, refs_from_stdin: bool, member_gate: bool, only: bool, stdin: &str) -> Option<Phase> {
    if !covers(stdin) {
        return None;
    }
    given.or_else(|| (refs_from_stdin && !member_gate && !only).then_some(Phase::Fast))
}

/// The Gitea server and repository a remote URL names (`https://host/owner/name.git` or `git@host:owner/name.git`): where the slow
/// rows' commit status is posted. `None` for a URL that is not one of those shapes.
pub fn server_repo(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let (server, path) = if let Some(rest) = url.strip_prefix("https://") {
        let (host, path) = rest.split_once('/')?;
        (format!("https://{host}"), path)
    } else {
        let rest = url.strip_prefix("git@")?;
        let (host, path) = rest.split_once(':')?;
        (format!("https://{host}"), path)
    };
    let repo = path.trim_end_matches('/').trim_end_matches(".git");
    let ok = repo.split('/').count() == 2
        && repo
            .split('/')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)));
    (ok && !server.contains(['"', '\n', '\\'])).then(|| (server, repo.to_string()))
}

/// Where the slow rows' verdict is posted: the server and repository of `remote`'s URL, needing a token. `Err`: why the profile
/// cannot be used for this push (the caller runs the full gate and says so).
pub fn status_target(root: &Path, remote: Option<&str>, token: Option<&str>) -> Result<(String, String), String> {
    if token.is_none_or(str::is_empty) {
        return Err("GITEA_TOKEN is not set, so the slow rows' verdict cannot be posted as a commit status".into());
    }
    let remote = remote.filter(|r| !r.is_empty() && !r.starts_with('-')).unwrap_or("origin");
    let url = crate::plan_repo::git_text(root, &["remote", "get-url", "--", remote])
        .ok_or_else(|| format!("the URL of remote {remote} cannot be read"))?;
    server_repo(&url)
        .ok_or_else(|| format!("remote {remote} is not an https or ssh Gitea URL (`owner/name`), so no commit status can be posted"))
}

/// A remote ref the profile covers: `refs/heads/wip/<name>` or `refs/heads/trial/<name>`.
pub fn is_trial_ref(remote_ref: &str) -> bool {
    ["refs/heads/wip/", "refs/heads/trial/"]
        .iter()
        .any(|p| remote_ref.strip_prefix(p).is_some_and(|rest| !rest.is_empty()))
}

/// Whether the profile covers this push: at least one ref sends a commit and every such ref is a trial ref. Read from every line of
/// git's pre-push input, not from the distinct commits (two refs of one commit are two refs), and a deletion line sends nothing.
pub fn covers(stdin: &str) -> bool {
    let mut any = false;
    for l in stdin.lines() {
        if let [_, sha, remote_ref, _] = l.split_whitespace().collect::<Vec<_>>()[..]
            && sha.chars().any(|c| c != '0')
        {
            if !is_trial_ref(remote_ref) {
                return false;
            }
            any = true;
        }
    }
    any
}

/// The rows that run before the push: each is cheap (median under about a minute) and static. Everything else is slow.
pub const FAST: &[&str] = &[
    "fmt",
    "lockfile",
    "commit-lint",
    "conformance",
    "gates-register",
    "ipc-endpoints",
    "pitwall-deny",
    "pitwall-scripts",
    "clippy",
];

/// Whether the row `id` is fast: its first word names a fast row (`clippy crates` is, `clippy-linux crates` is not, `conformance` is,
/// `conformance-tests` is not).
pub fn is_fast_row(id: &str) -> bool {
    let first = id.split_whitespace().next().unwrap_or(id).trim_end_matches(',');
    FAST.contains(&first)
}

fn planned_id(p: &Planned) -> &str {
    match p {
        Planned::Run(g) => &g.id,
        Planned::Skip { id, .. } | Planned::NotMeasured { id, .. } | Planned::Broken { id, .. } => id,
    }
}

/// The rows of `phase`: every planned entry (a run, a skip, a not-measured or a broken check) goes to exactly one phase by its id, so
/// a slow row that cannot be measured is reported by the slow phase and cancels the trial like a failure does.
pub fn rows_of(planned: Vec<Planned>, phase: Phase) -> Vec<Planned> {
    planned
        .into_iter()
        .filter(|p| is_fast_row(planned_id(p)) == (phase == Phase::Fast))
        .collect()
}

/// How long after its start a slow phase may run before its record counts as never finished: the gate's run budget and queue
/// allowance, one row cap and a margin. Derived from the gate's own limits, not a new number.
pub fn deadline_after(budget: Duration, queue: Duration, row_cap: Duration) -> Duration {
    budget + queue + row_cap + Duration::from_secs(300)
}

/// The record a finished slow phase leaves: the first failing row (or the first not-measured) decides, nothing else passes. A row
/// excused by a person's override still counts as not measured here: the override lets a push go, it never makes a trial's evidence.
pub fn fold(sha: &str, rows: &[(String, Row)], base: &Slow) -> Slow {
    let mut rec = Slow {
        state: State::Passed,
        row: String::new(),
        why: String::new(),
        next: String::new(),
        ..base.clone()
    };
    rec.sha = sha.to_string();
    let worst = |k: Kind| match k {
        Kind::Failed | Kind::Broken => 3,
        Kind::NotMeasured | Kind::Accepted => 2,
        Kind::Passed | Kind::Skipped => 0,
    };
    if rows.is_empty() {
        // Nothing ran: an empty slow phase is no evidence either (a selector or a plan that lost every slow row).
        rec.state = State::NotMeasured;
        rec.why = "the slow phase ran no row".into();
        rec.next = "run `./dev check --pre-push` on the commit".into();
        return rec;
    }
    if let Some((id, row)) = rows
        .iter()
        .filter(|(_, r)| worst(r.kind()) > 0)
        .max_by_key(|(_, r)| worst(r.kind()))
    {
        rec.state = if worst(row.kind()) == 3 {
            State::Failed
        } else {
            State::NotMeasured
        };
        rec.row = id.clone();
        rec.why = row.reason();
        rec.next = row.next.clone();
    }
    rec
}

/// The background child's command line: the same program, the slow phase, the repository root (it runs from there, never from the
/// logs folder, which is inside `.git`) and the logs folder of the hook's run. Its standard input is git's pre-push lines in a file,
/// and it always measures a throw-away worktree of the pushed commit, never the live tree.
pub fn child_args(base: &str, remote: Option<&str>, root: &Path, logs: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["check", "--pre-push", "--refs-from-stdin", "--split-phase", "slow", "--base", base]
        .iter()
        .map(|s| s.to_string())
        .collect();
    if let Some(r) = remote {
        a.extend(["--remote".into(), r.into()]);
    }
    a.extend(["--root".into(), root.to_string_lossy().into_owned()]);
    a.extend(["--split-logs".into(), logs.to_string_lossy().into_owned()]);
    a
}

/// Starts the slow phase as one process in a group of its own (it outlives the hook, and no Ctrl+C of the push reaches it), with
/// its output in `<logs>/slow-phase.log` and its input from `<logs>/slow-phase.stdin`. Returns the child's identity: the handle the
/// starter keeps (ER-51), written into the record. Neither of git's pipes goes to the child: it would hold the push open.
pub fn start_slow(
    program: &str,
    args: &[String],
    stdin: &str,
    root: &Path,
    logs: &Path,
    env: &std::collections::HashMap<String, String>,
) -> Result<(u32, u64), String> {
    use std::process::Stdio;
    std::fs::create_dir_all(logs).map_err(|e| format!("{} cannot be created: {e}", logs.display()))?;
    let in_file = logs.join("slow-phase.stdin");
    std::fs::write(&in_file, stdin).map_err(|e| format!("{} cannot be written: {e}", in_file.display()))?;
    let out = std::fs::File::create(logs.join("slow-phase.log")).map_err(|e| format!("slow-phase.log cannot be created: {e}"))?;
    let err = out.try_clone().map_err(|e| format!("slow-phase.log cannot be shared: {e}"))?;
    let input = std::fs::File::open(&in_file).map_err(|e| format!("{} cannot be read: {e}", in_file.display()))?;
    let mut cmd = paddock_proc::command(program);
    cmd.args(args)
        .current_dir(root)
        .stdin(Stdio::from(input))
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    // The caller's environment, as the hook had it (ER-47), minus nothing: the gates inside the child read what the hook's did.
    cmd.envs(env);
    let child = paddock_proc::spawn_detached(&mut cmd).map_err(|e| e.to_string())?;
    let pid = child.id();
    // The child is not waited for: it ends on its own and the record says how. Dropping the handle does not end it.
    let start = paddock_proc::ProcessId::of(pid).map_or(0, |p| p.started().as_micros());
    drop(child);
    if start == 0 {
        // A holder whose start time cannot be read cannot be told from a reused pid later: the rows are not measured.
        return Err(format!(
            "the start time of process {pid} cannot be read, so its identity cannot be kept (ER-51)"
        ));
    }
    Ok((pid, start))
}

/// What a finished slow phase prints, and does, about the CI run: writes the final record, then settles with the dispatch record.
/// Never fails the rows' own verdict; an error here is returned as text for the log.
pub fn finish(ex: &dyn Exec, state: &Path, rec: &Slow) -> String {
    if let Err(e) = trial_guard::write_slow(state, rec) {
        return format!(
            "::error::trial: the slow rows' record could not be written ({e}); a guarded dispatch will treat the trial as not measured. Next: run `./dev check --pre-push` on the commit"
        );
    }
    ex.settle_trial(state, &rec.sha)
}

/// The settle on the real machine: the token from `GITEA_TOKEN`, curl, the clock and the process table. The final status is posted
/// first, retried for a minute (the push may not have landed on the server yet), then the run is settled.
pub fn settle_real(state: &Path, sha: &str) -> String {
    let token = std::env::var("GITEA_TOKEN").unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut status = String::new();
    if let Ok(Some(rec)) = trial_guard::read_slow(state, sha) {
        for attempt in 0..6 {
            match trial_guard::post_slow_status(&mut crate::wait::CurlHttp, &token, &rec) {
                Ok(_) => {
                    status.clear();
                    break;
                }
                Err(e) => {
                    status = format!(
                        "::warning::trial {}: the status {} could not be posted ({e}); `./dev ci dispatch --guard` and the next gate start post it again. ",
                        &sha[..sha.len().min(12)],
                        trial_guard::CONTEXT
                    );
                    if attempt < 5 {
                        std::thread::sleep(Duration::from_secs(10));
                    }
                }
            }
        }
    }
    format!(
        "{status}{}",
        settle_with(
            state,
            sha,
            &mut crate::wait::CurlHttp,
            &token,
            now,
            &trial_guard::holder_alive,
            false
        )
    )
}

/// Settles and words the result as the log line of the slow phase (an `::error::` when the run is cancelled or cannot be). With
/// `post_status`, the record's verdict is first posted as the commit status (once, no retry).
pub fn settle_with(
    state: &Path,
    sha: &str,
    http: &mut dyn crate::wait::Http,
    token: &str,
    now: u64,
    alive: &dyn Fn(u32, u64) -> bool,
    post_status: bool,
) -> String {
    use trial_guard::Settled;
    let short = &sha[..sha.len().min(12)];
    let mut status = String::new();
    if post_status
        && let Ok(Some(rec)) = trial_guard::read_slow(state, sha)
        && let Err(e) = trial_guard::post_slow_status(http, token, &rec)
    {
        status = format!(
            "::warning::trial {short}: the status {} could not be posted ({e}). ",
            trial_guard::CONTEXT
        );
    }
    let line = match trial_guard::settle(state, sha, http, token, now, alive) {
        Ok(Settled::Cancelled { run, why, already_over }) => format!(
            "::error::{why}. CI run {run} {}",
            if already_over {
                "had already finished"
            } else {
                "was cancelled (ER-9: a failed local check cancels the run)"
            }
        ),
        Ok(Settled::NoRun(trial_guard::Verdict::Cancel(why))) => {
            format!("::error::{why}. No CI run is registered for {short} yet: `./dev ci dispatch --guard` will refuse to dispatch it")
        }
        Ok(Settled::NoRun(_)) => {
            format!("trial {short}: the slow rows ended; no CI run is registered, `./dev ci dispatch --guard` reads this record")
        }
        Ok(Settled::Waiting) => format!("trial {short}: the slow rows are still running"),
        Ok(Settled::Fine) => format!("trial {short}: every slow row passed; the CI run stays (it is still the evidence a train needs)"),
        Ok(Settled::Unguarded) => format!("trial {short}: no record to settle"),
        Err(e) => format!(
            "::error::trial {short}: the CI run could not be settled against the slow rows ({e}). Next: cancel the run by hand if the record says failed (`{}`)",
            trial_guard::dir(state).join(format!("{sha}.slow.json")).display()
        ),
    };
    format!("{status}{line}")
}

/// The sweep on the real machine ([`trial_guard::sweep`]): records whose process is dead or past its deadline are settled. Without
/// a token nothing is posted or cancelled (the records still refuse locally).
pub fn sweep_real(state: &Path) -> Vec<String> {
    let token = std::env::var("GITEA_TOKEN").unwrap_or_default();
    if token.is_empty() {
        return Vec::new();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    trial_guard::sweep(
        state,
        &mut crate::wait::CurlHttp,
        &token,
        &trial_guard::Time {
            now,
            alive: &trial_guard::holder_alive,
        },
    )
}

/// Posts `pending` for the record of `sha`, retried every 5 s for at most a minute while `done` is not set (the push may not have
/// landed yet, and the status needs the commit on the server). Returns what happened, for the log.
pub fn post_pending_real(state: &Path, sha: &str, done: &std::sync::atomic::AtomicBool) -> String {
    use std::sync::atomic::Ordering;
    let short = &sha[..sha.len().min(12)];
    let token = std::env::var("GITEA_TOKEN").unwrap_or_default();
    let Ok(Some(rec)) = trial_guard::read_slow(state, sha) else {
        return format!("trial {short}: no record to post a status for");
    };
    let mut last = String::new();
    for _ in 0..12 {
        match trial_guard::post_slow_status(&mut crate::wait::CurlHttp, &token, &rec) {
            Ok(_) => return format!("trial {short}: status {} is pending", trial_guard::CONTEXT),
            Err(e) => last = e,
        }
        for _ in 0..50 {
            if done.load(Ordering::SeqCst) {
                return format!("trial {short}: the pending status was not posted ({last}); the rows ended first");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    format!("trial {short}: the pending status could not be posted within a minute ({last})")
}

/// Ends the process that held a superseded trial's slow rows, by identity: only a process still running with that pid and start
/// time (`paddock_proc::kill_tree`, ER-51, rule 53).
pub fn end_holder_real(pid: u32, start_micros: u64) {
    let id = paddock_proc::ProcessId::new(pid, paddock_proc::StartTime::from_micros(start_micros));
    if id.may_be_alive() {
        paddock_proc::kill_tree(id);
    }
}

#[cfg(test)]
mod tests;
