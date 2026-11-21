//! The processes of a host job end with the job (ER-14: no process left behind), and one that did not is reported.
//!
//! The 2026-11-20 incident: `vm-lock.sh` (`vm_lock_acquire`, pid 58558), started by a macOS runner job, was still running four days
//! after the job ended. A host runner starts every step of a job as a process of its own and gives the job no process group or job
//! object that its end would close, so what a step starts and leaves behind (a background script, a process that ignores SIGTERM,
//! one that detached into a session of its own) lives on. The runner cannot be asked to group a job, and a process group would not
//! hold a detached child anyway, so a job's processes are told by what they carry (a [`Job`]), through one read of the process table
//! (`paddock_proc::own::survey`):
//!
//! - [`TOKEN`] in their environment (`PADDOCK_JOB_TEMP`, the job's own temp folder, unique per job, exported to every later step
//!   through `GITHUB_ENV`; a child inherits it, so a grandchild and a detached process carry it). **macOS shows no environment of a
//!   program Apple ships** (`/bin/bash`, `/bin/sh`, `/bin/sleep`; measured 2026-11-21), so this finds Rust, Homebrew and Linux programs;
//! - a current folder inside the job's folder (`<work>/<16 hex>`, the parent of `RUNNER_TEMP`; the runner removes the whole folder
//!   after the job), which a step's script has unless it moves away;
//! - the job's folder in the command line (`bash <job>/tmp/<id>.sh`, an absolute path to a script or a socket).
//!
//! - [`stop_job`] (the end of `release`, and `dev ci isolation processes stop`): ends every process of this job and every process
//!   below one, except the caller and its ancestors (the step that runs the release is itself a member) and the job's slot holder
//!   (`ci slot hold`, which `release` itself asks to let go). Each is asked to stop and ended after [`GRACE`], all at once, through
//!   `paddock_proc::terminate` by identity (pid plus start time, rule 53): a pid handed on meanwhile is left alone.
//! - [`sweep`] (`acquire`, and `dev ci isolation processes sweep [--end]`): lists the processes of this user that carry a job folder
//!   under the same work folder that no longer exists, as `::warning::` lines with pid, age, job and command (ER-27). It ends nothing
//!   unless `--end` is given: an acquire never ends what an earlier job left, it says so.
//!
//! What it cannot see: a process with none of the three marks (an Apple program that moved to another folder, was started with a
//! relative script path and has a cleared environment), another user's processes, and on Windows every process (another process's
//! environment and folder are not read there: the answer is "not measured", never "none"). A Windows job object that the end of the
//! job closes needs the runner's cooperation, which it does not give (the runner's own step job object does not kill on close,
//! `slots.rs`): not done here, recorded in docs/lessons.md.

use super::{Ctx, R, Sinks, Stop};
use paddock_proc::own::{Row, now, survey};
use paddock_proc::{ProcessId, Terminated, terminate};
use regex::Regex;
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

/// The environment variable the job-isolation step exports to every later step of a job: the job's own temp folder.
pub const TOKEN: &str = "PADDOCK_JOB_TEMP";
/// How long a member asked to stop has before it is ended.
pub const GRACE: Duration = Duration::from_secs(5);
/// The most processes ended at the same time (one thread each).
const AT_ONCE: usize = 32;
/// What the command line of the job's slot holder holds (`dev ci slot hold --owner <job temp> ...`).
const HOLDER: &str = " slot hold ";

/// One process of a job, or a leftover of an earlier one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Its identity (pid plus start time).
    pub id: ProcessId,
    /// The job it belongs to: the value of [`TOKEN`] it carries.
    pub job: String,
    /// Its command line, cut.
    pub command: String,
}

/// A job, by what its processes carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// The value of [`TOKEN`]: the job's own temp folder (`RUNNER_TEMP`).
    pub token: String,
    /// The folder the runner removes after the job: the parent of the temp folder when its name is the runner's 16 hex digits, else the
    /// temp folder itself.
    pub scope: String,
    /// `scope` as the OS spells it (symlinks resolved: macOS reports a process's current folder as `/private/var/...` for `/var/...`),
    /// when the folder exists.
    pub real: Option<String>,
}

impl Job {
    /// The job whose temp folder is `token`.
    pub fn of(token: &str) -> Job {
        let parent = token.rsplit_once('/').map(|(p, _)| p).filter(|p| is_job_folder(p));
        let scope = parent.unwrap_or(token).to_string();
        let real = paddock_env::winpath::real(std::path::Path::new(&scope))
            .ok()
            .and_then(|p| p.to_str().map(str::to_string))
            .filter(|r| *r != scope);
        Job {
            token: token.to_string(),
            scope,
            real,
        }
    }

    /// The folder the runner makes the job folders in (`<work>`), when the job folder is one of its.
    pub fn work(&self) -> Option<&str> {
        self.scope.rsplit_once('/').map(|(w, _)| w).filter(|_| is_job_folder(&self.scope))
    }

    /// The folder as given and as the OS spells it.
    fn spellings(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.scope.as_str()).chain(self.real.as_deref())
    }

    /// Whether `row` carries this job's token, has its folder as (or inside) its current folder, or names it in its command line.
    pub fn owns(&self, row: &Row) -> bool {
        row.value.as_deref() == Some(self.token.as_str())
            || row.cwd.as_deref().is_some_and(|c| self.spellings().any(|s| under(c, s)))
            || self.spellings().any(|s| mentions(&row.args, s))
    }
}

/// Whether `path` is a job folder of the runner: a last component of 16 hexadecimal digits.
fn is_job_folder(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|n| n.len() == 16 && n.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Whether `path` is `scope` or inside it.
fn under(path: &str, scope: &str) -> bool {
    path.strip_prefix(scope)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Whether `text` names `scope` as a whole path: the next character is not part of a longer name.
fn mentions(text: &str, scope: &str) -> bool {
    text.match_indices(scope).any(|(at, _)| {
        text[at + scope.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
    })
}

/// `me` and its ancestors that `table` knows: the step that runs the release is a member of its own job and never ends itself, nor the
/// shell and scripts above it.
pub fn self_and_ancestors(table: &[Row], me: u32) -> HashSet<u32> {
    let parents: BTreeMap<u32, Option<u32>> = table.iter().map(|c| (c.id.pid(), c.parent)).collect();
    let mut out = HashSet::from([me]);
    let mut at = me;
    // A parent chain is short; the bound only stops a corrupt table that loops.
    for _ in 0..256 {
        match parents.get(&at).copied().flatten() {
            Some(p) if out.insert(p) => at = p,
            _ => break,
        }
    }
    out
}

/// Whether `command` is the slot holder of a job: the holder frees the slot when it ends, and `release` asks it to do that itself.
fn is_holder(command: &str) -> bool {
    command.contains(HOLDER) && command.contains(" ci ")
}

/// The command line of `row`, cut for a message.
fn shown(row: &Row) -> String {
    row.args.chars().take(300).collect()
}

/// The processes of `job` in `table`: every one that [`Job::owns`], and every process below one (a descendant that cleared its
/// environment and moved away), except `protect` (the caller and its ancestors), the job's slot holder and processes that have already
/// ended.
pub fn members_of(table: &[Row], job: &Job, protect: &HashSet<u32>) -> Vec<Member> {
    let mut out: Vec<Member> = table
        .iter()
        .filter(|c| job.owns(c) && !c.zombie && !protect.contains(&c.id.pid()) && !is_holder(&c.args))
        .map(|c| Member {
            id: c.id,
            job: job.token.clone(),
            command: shown(c),
        })
        .collect();
    let have: HashSet<ProcessId> = out.iter().map(|m| m.id).collect();
    let mut below: Vec<Member> = Vec::new();
    for m in &out {
        for d in paddock_proc::descendants_of(m.id) {
            if !have.contains(&d) && !protect.contains(&d.pid()) && !below.iter().any(|b| b.id == d) {
                below.push(Member {
                    id: d,
                    job: job.token.clone(),
                    command: "(below a member of the job)".to_string(),
                });
            }
        }
    }
    out.extend(below);
    out
}

/// The processes in `table` that carry the folder of a job that no longer exists (`exists` says whether a folder is there): a token
/// whose folder is gone, or a job folder of `work` (`<work>/<16 hex>`) in the current folder or the command line that is gone. Except
/// `protect`.
pub fn leftovers(table: &[Row], work: Option<&str>, exists: &dyn Fn(&str) -> bool, protect: &HashSet<u32>) -> Vec<Member> {
    let folder = work.and_then(|w| Regex::new(&format!(r"{}/[0-9a-f]{{16}}(?:[/\s'\x22]|$)", regex::escape(w))).ok());
    let mut seen: BTreeMap<String, bool> = BTreeMap::new();
    let mut gone = |path: &str| *seen.entry(path.to_string()).or_insert_with(|| !exists(path));
    let mut out = Vec::new();
    for c in table
        .iter()
        .filter(|c| !c.zombie && !protect.contains(&c.id.pid()) && !is_holder(&c.args))
    {
        let mut job: Option<String> = c.value.clone().filter(|v| gone(v));
        if job.is_none()
            && let Some(re) = &folder
        {
            let text = format!("{} {}", c.cwd.as_deref().unwrap_or(""), c.args);
            job = re
                .find_iter(&text)
                .map(|m| m.as_str().trim_end_matches(|ch: char| !(ch.is_ascii_alphanumeric())).to_string())
                .find(|f| gone(f));
        }
        if let Some(job) = job {
            out.push(Member {
                id: c.id,
                job,
                command: shown(c),
            });
        }
    }
    out
}

/// Ends every member at once: asked to stop, ended after `grace` (a process that ignores the request is killed), each by identity.
pub fn stop(members: &[Member], grace: Duration) -> Vec<(Member, Terminated)> {
    let mut out = Vec::with_capacity(members.len());
    for batch in members.chunks(AT_ONCE) {
        let done: Vec<Terminated> = std::thread::scope(|s| {
            let running: Vec<_> = batch
                .iter()
                .map(|m| s.spawn(move || gone_after(m.id, terminate(m.id, grace))))
                .collect();
            running.into_iter().map(|h| h.join().unwrap_or(Terminated::Unknown)).collect()
        });
        out.extend(batch.iter().cloned().zip(done));
    }
    out
}

/// How long a process that was sent SIGKILL may take to leave the table (the kernel ends it asynchronously; under load it is not at once).
const GONE_WITHIN: Duration = Duration::from_secs(5);

/// `how`, once `id` has really ended: a kill that returned before the process left the table is waited for (a release that returned
/// early let the runner remove the job's folder under a process that was still dying).
fn gone_after(id: ProcessId, how: Terminated) -> Terminated {
    if how == Terminated::Killed {
        let end = std::time::Instant::now() + GONE_WITHIN;
        while !paddock_proc::own::has_ended(id) && std::time::Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    how
}

/// `3 d 4 h`, `5 min`, `12 s`: how long ago `id` started.
fn age(id: ProcessId) -> String {
    let secs = now().as_micros().saturating_sub(id.started().as_micros()) / 1_000_000;
    match secs {
        s if s >= 86_400 => format!("{} d {} h", s / 86_400, s % 86_400 / 3_600),
        s if s >= 3_600 => format!("{} h {} min", s / 3_600, s % 3_600 / 60),
        s if s >= 60 => format!("{} min", s / 60),
        s => format!("{s} s"),
    }
}

fn how(t: Terminated) -> &'static str {
    match t {
        Terminated::Gracefully => "stopped when asked",
        Terminated::Killed => "did not stop when asked: ended",
        Terminated::Gone => "already gone",
        Terminated::Unknown => "could not be told apart from a reused pid: left alone",
    }
}

/// Windows reads no other process's environment: say so rather than answer "none".
fn unmeasured(ctx: &Ctx, io: &mut Sinks<'_>, what: &str) -> bool {
    if ctx.windows || cfg!(windows) {
        io.say(format!(
            "::warning::job-isolation: {what} not measured: another process's environment is not read on Windows, so the job's processes cannot be told from other processes. Next step: end a leftover of an earlier job by hand (Task Manager) and see docs/lessons.md (2026-11-21, job processes)."
        ));
        return true;
    }
    false
}

/// The end of a job: ends every process of this job (see the module). Never fails the job: a process that cannot be ended is a warning.
pub fn stop_job(ctx: &Ctx, io: &mut Sinks<'_>) -> R<()> {
    let Some(job) = ctx.get(TOKEN) else {
        io.say("job-isolation: this job has no PADDOCK_JOB_TEMP, so it has no processes to end");
        return Ok(());
    };
    if unmeasured(ctx, io, "the end of the job's processes is") {
        return Ok(());
    }
    // The token must be this job's own folder (job-tmpdir sets PADDOCK_JOB_TEMP to the runner's RUNNER_TEMP). A token inherited from
    // another job's environment (a test or a tool run inside a job, an operator's shell) names processes that are not ours to end.
    if super::home::job_temp(ctx).ok().as_deref() != Some(job) {
        io.say("::warning::job-isolation: PADDOCK_JOB_TEMP is not this job's RUNNER_TEMP, so it names another job's processes: nothing is ended. Next step: run the release step of the job itself, or `./dev ci isolation processes sweep --end` for the leftovers of finished jobs.");
        return Ok(());
    }
    let table = survey(TOKEN);
    let protect = self_and_ancestors(&table, std::process::id());
    let found = members_of(&table, &Job::of(job), &protect);
    if found.is_empty() {
        io.say("job-isolation: no process of this job is left running");
        return Ok(());
    }
    let started = std::time::Instant::now();
    let results = stop(&found, GRACE);
    for (m, t) in &results {
        io.say(format!(
            "::warning::job-isolation: pid {} (started {} ago) was still running at the end of the job, {}: {}",
            m.id.pid(),
            age(m.id),
            how(*t),
            m.command
        ));
    }
    io.say(format!(
        "job-isolation: ended {} process(es) the job left running, in {:.1} s",
        results.len(),
        started.elapsed().as_secs_f64()
    ));
    Ok(())
}

/// Lists (and, with `end`, ends) the processes of finished jobs. Always exits 0 once the arguments are valid.
pub fn sweep(ctx: &Ctx, io: &mut Sinks<'_>, end: bool) -> R<()> {
    if unmeasured(ctx, io, "the sweep for processes of finished jobs is") {
        return Ok(());
    }
    let table = survey(TOKEN);
    let protect = self_and_ancestors(&table, std::process::id());
    let own = super::home::job_temp(ctx).ok().map(|t| Job::of(&t));
    let left = leftovers(
        &table,
        own.as_ref().and_then(Job::work),
        &|j| std::path::Path::new(j).exists(),
        &protect,
    );
    if left.is_empty() {
        io.say("job-isolation: no process of a finished job is running");
        return Ok(());
    }
    for m in &left {
        io.say(format!(
            "::warning::job-isolation: pid {} (started {} ago) runs on after its job ended ({} is gone): {}. Next step: `./dev ci isolation processes sweep --end` ends it by identity.",
            m.id.pid(),
            age(m.id),
            m.job,
            m.command
        ));
    }
    if end {
        for (m, t) in stop(&left, GRACE) {
            io.say(format!("job-isolation: pid {} {}", m.id.pid(), how(t)));
        }
    }
    Ok(())
}

/// `dev ci isolation processes stop | sweep [--end]`.
pub fn command(ctx: &Ctx, rest: &[String], io: &mut Sinks<'_>) -> R<()> {
    match rest.first().map(String::as_str) {
        Some("stop") if rest.len() == 1 => stop_job(ctx, io),
        Some("sweep") if rest.len() == 1 => sweep(ctx, io, false),
        Some("sweep") if rest.len() == 2 && rest[1] == "--end" => sweep(ctx, io, true),
        _ => Err(Stop::line(2, "usage: dev ci isolation processes stop | sweep [--end]")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slot_holder_is_recognised_by_its_command_line() {
        assert!(is_holder(
            "/h/tools/paddock-dev/bin/0/paddock-dev ci slot hold --max 4 --owner /w/tmp --root /h"
        ));
        assert!(!is_holder("sleep 300"));
        assert!(!is_holder("bash vm-lock.sh vm_lock_acquire"));
    }

    #[test]
    fn a_token_that_is_not_this_jobs_own_temp_ends_nothing() {
        // A test, a tool or a shell inside another job inherits that job's PADDOCK_JOB_TEMP: it must never end that job's processes.
        let ctx = super::super::testkit::ctx(&[("PADDOCK_JOB_TEMP", "/w/other/tmp"), ("RUNNER_TEMP", "/w/mine/tmp")]);
        let (code, _, err) = super::super::testkit::go(&ctx, &["processes", "stop"]);
        assert_eq!(code, 0);
        assert!(err.contains("nothing is ended"), "{err}");
        let ctx = super::super::testkit::ctx(&[("PADDOCK_JOB_TEMP", "/w/other/tmp")]);
        let (code, _, err) = super::super::testkit::go(&ctx, &["processes", "stop"]);
        assert_eq!(code, 0);
        assert!(err.contains("nothing is ended"), "{err}");
    }

    #[test]
    fn a_usage_error_is_exit_2() {
        let ctx = super::super::testkit::ctx(&[]);
        let (code, _, err) = super::super::testkit::go(&ctx, &["processes", "bogus"]);
        assert_eq!(code, 2);
        assert!(err.contains("usage: dev ci isolation processes"), "{err}");
    }
}
