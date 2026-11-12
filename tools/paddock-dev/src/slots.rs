//! `dev ci slot`: job-isolation slots (ADR 0022 R0 addition). The logic is `paddock-slots`; this is the command line and
//! the process plumbing.
//!
//! # The holder
//!
//! A slot is an exclusive OS file lock, and a lock lives exactly as long as the process that took it. `acquire` therefore
//! starts a small *detached holder* (`dev ci slot hold`, this same binary), which takes the slot, tells `acquire` its
//! name through a pipe and keeps the lock. The holder:
//!
//! - ends when the job's folder (`--owner`) no longer exists, because the runner removes it after the job, however the job
//!   ended, or when `release` asks it to (a marker file; no signals, so the same on every OS);
//! - leads its own process group (Unix) or process group without a console (Windows), so a runner that kills the job's
//!   group at the end of a step does not free the slot in the middle of the job;
//! - carries no handle or descriptor of the step that started it ([`paddock_proc::spawn_detached_clearing_handles`]): the Gitea runner ends every process of a
//!   step's job object when the step's output pipe is still open 10 s after the step ended, and a holder that kept that pipe
//!   died with the step (run 8019). A holder without it outlives the step: the step's job object does not kill on close. It
//!   does not break away from the job object (the runner's job does not allow it); the runner ends it with the job, which is
//!   when it would end anyway;
//! - frees the slot at once if it is killed or crashes, because the kernel drops the lock.
//!
//! No stale lock can exist, so nothing is ever reclaimed and two jobs cannot hold one slot.
//!
//! # Work on an idle slot
//!
//! `idle-run --slot sN -- <command>` runs a command only while no job holds slot `sN`, and holds that slot's lock until the
//! command ends, so no job can take the slot while the command deletes its kept folders (the CI kit's disk sweep). A busy
//! slot is skipped (exit 0, `"ran": false`). The command sees `PADDOCK_CI_IDLE_SLOT=sN`.

use clap::Args;
use paddock_cli::{CliError, Report};
use paddock_slots::{Announcement, FsLocks, SlotError, SlotId, release as release_slot, slot_states, with_idle};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// The most slots a caller may ask for.
pub const MAX_SLOTS: u32 = 1024;
const DEFAULT_POLL_MS: u64 = 500;
/// The longest a CI job runs: the Gitea runner's default `runner.timeout` and Gitea's own job timeout, both 3 h (ADR 0025;
/// the runners set no other value).
pub const RUNNER_JOB_TIMEOUT_SECS: u64 = 3 * 60 * 60;
/// What the holder waits beyond that: the runner's own clean-up after a timed-out job, so the cap never ends a live job.
pub const HOLD_MARGIN_SECS: u64 = 30 * 60;
/// A holder lets go after this long even when its job folder still exists: a stalled runner that kept the folder (a job that
/// never ended and never released) frees its slot by itself.
pub const DEFAULT_MAX_HOLD_SECS: u64 = RUNNER_JOB_TIMEOUT_SECS + HOLD_MARGIN_SECS;
/// How long `acquire` waits for the holder's first line (its slot, or `none`) before it gives up and stops the holder.
const DEFAULT_FIRST_LINE_SECS: u64 = 30;
/// How long the holder keeps trying while every slot looks busy (paddock_slots::ACQUIRE_GRACE, in milliseconds).
const DEFAULT_GRACE_MS: u64 = paddock_slots::ACQUIRE_GRACE.as_millis() as u64;

/// `acquire`'s arguments.
#[derive(Args)]
pub struct AcquireArgs {
    /// How many slots there are (s1..sN).
    #[arg(long)]
    max: u32,
    /// The job's own folder: the slot is freed when it is gone.
    #[arg(long)]
    owner: PathBuf,
    /// The CI home that holds `slots/`.
    #[arg(long)]
    root: PathBuf,
    /// How often the holder looks for the end of the job (milliseconds).
    #[arg(long, default_value_t = DEFAULT_POLL_MS, hide = true)]
    poll_ms: u64,
    /// The longest the holder keeps the slot, in seconds, whether or not the job folder still exists (0: no limit).
    #[arg(long, default_value_t = DEFAULT_MAX_HOLD_SECS)]
    max_hold_secs: u64,
    /// How long to wait for the holder to take a slot, in seconds.
    #[arg(long, default_value_t = DEFAULT_FIRST_LINE_SECS, hide = true)]
    holder_timeout_secs: u64,
    /// How long the holder keeps trying while every slot looks busy before it says so (milliseconds; tests shorten it).
    #[arg(long, default_value_t = DEFAULT_GRACE_MS, hide = true)]
    acquire_grace_ms: u64,
}

/// `release`'s arguments.
#[derive(Args)]
pub struct ReleaseArgs {
    /// The slot to let go (`s1`...).
    #[arg(long)]
    slot: String,
    /// The token `acquire` printed (second word of its output, `token` with --json): only the holder it belongs to is released.
    #[arg(long)]
    token: String,
    /// The CI home that holds `slots/`.
    #[arg(long)]
    root: PathBuf,
    /// How long to wait for the holder to let go.
    #[arg(long, default_value_t = 30)]
    timeout_secs: u64,
}

/// `status`'s arguments.
#[derive(Args)]
pub struct StatusArgs {
    /// How many slots there are.
    #[arg(long)]
    max: u32,
    /// The CI home that holds `slots/`.
    #[arg(long)]
    root: PathBuf,
}

/// `idle-run`'s arguments.
#[derive(Args)]
pub struct IdleRunArgs {
    /// The slot that must be idle (`s1`...).
    #[arg(long)]
    slot: String,
    /// The CI home that holds `slots/`.
    #[arg(long)]
    root: PathBuf,
    /// The command and its arguments, after `--`.
    #[arg(last = true, required = true)]
    command: Vec<String>,
}

/// `hold`'s arguments (the holder process).
#[derive(Args)]
pub struct HoldArgs {
    /// How many slots there are.
    #[arg(long)]
    max: u32,
    /// The job's folder.
    #[arg(long)]
    owner: PathBuf,
    /// The CI home.
    #[arg(long)]
    root: PathBuf,
    /// Look interval in milliseconds.
    #[arg(long, default_value_t = DEFAULT_POLL_MS)]
    poll_ms: u64,
    /// The longest the holder keeps the slot, in seconds (0: no limit).
    #[arg(long, default_value_t = DEFAULT_MAX_HOLD_SECS)]
    max_hold_secs: u64,
    /// How long to keep trying while every slot looks busy (milliseconds).
    #[arg(long, default_value_t = DEFAULT_GRACE_MS, hide = true)]
    acquire_grace_ms: u64,
}

fn check_max(max: u32) -> Result<(), CliError> {
    if (1..=MAX_SLOTS).contains(&max) {
        Ok(())
    } else {
        Err(CliError::usage(format!("--max must be from 1 to {MAX_SLOTS}, not {max}.")).with_hint("Give the number of job slots."))
    }
}

fn failure(e: SlotError) -> CliError {
    match e {
        SlotError::Invalid(_) => CliError::usage(e.to_string()),
        other => CliError::failure(other.to_string()),
    }
}

/// Reads the holder's one line.
pub fn parse_announcement(line: &str) -> Result<Announcement, CliError> {
    Announcement::parse(line).map_err(|_| CliError::failure(format!("The slot holder said {:?}, not a slot and a token.", line.trim())))
}

/// The holder's command line.
pub fn holder_args(a: &AcquireArgs, owner: &Path) -> Vec<String> {
    vec![
        "ci".into(),
        "slot".into(),
        "hold".into(),
        "--max".into(),
        a.max.to_string(),
        "--owner".into(),
        owner.display().to_string(),
        "--root".into(),
        a.root.display().to_string(),
        "--poll-ms".into(),
        a.poll_ms.to_string(),
        "--max-hold-secs".into(),
        a.max_hold_secs.to_string(),
        "--acquire-grace-ms".into(),
        a.acquire_grace_ms.to_string(),
    ]
}

/// The holder must run from somewhere that does not stand in the job folder's way: a process whose working folder or program
/// file is inside `owner` stops Windows from deleting the folder when the job ends (and the slot would stay held). The
/// program is refused when it lies under the job folder (a dirty `tools/paddock-dev/target` or a `PADDOCK_DEV_FRESH` build
/// folder inside the job).
pub fn check_program_outside_owner(exe: &Path, owner: &Path) -> Result<(), CliError> {
    let (exe, owner) = (canonical_even_if_missing(exe), canonical_even_if_missing(owner));
    if exe.starts_with(&owner) {
        return Err(CliError::usage(format!(
            "The dev program {} is inside the job folder {}, so the slot holder would keep that folder from being deleted.",
            exe.display(),
            owner.display()
        ))
        .with_hint("Build or copy the tool outside the job folder (PADDOCK_DEV_TARGET_DIR, PADDOCK_DEV_HOME) and run it from there."));
    }
    Ok(())
}

/// The CI home holds the holder's working folder and the lock files; inside the job folder it would stand in the way of deleting
/// that folder just as the program would, and the slot's own files would vanish with the job.
pub fn check_root_outside_owner(root: &Path, owner: &Path) -> Result<(), CliError> {
    let (root, owner) = (canonical_even_if_missing(root), canonical_even_if_missing(owner));
    if root.starts_with(&owner) {
        return Err(CliError::usage(format!(
            "--root {} is inside the job folder {}, so the slot would disappear with the job and keep the folder from being deleted.",
            root.display(),
            owner.display()
        ))
        .with_hint("Use a CI home outside the job folder."));
    }
    Ok(())
}

/// `path` with symbolic links resolved, also when its last parts do not exist yet (the nearest existing folder is resolved).
fn canonical_even_if_missing(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut at = path;
    loop {
        if let Ok(real) = at.canonicalize() {
            return missing.iter().rev().fold(real, |acc, part| acc.join(part));
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name.to_os_string());
                at = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// The holder's command: a pipe for its announcement and nothing else attached to this process, started in the CI home (never
/// inside the job folder). It is started with [`paddock_proc::spawn_detached_clearing_handles`] (`dev` is single-threaded there).
pub fn holder_command(exe: &Path, args: &[String], root: &Path) -> std::process::Command {
    let mut cmd = paddock_proc::command(exe);
    cmd.args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd
}

/// `dev ci slot acquire`.
pub fn acquire(a: &AcquireArgs) -> Result<Report, CliError> {
    let exe = std::env::current_exe().unwrap_or_default();
    acquire_with(a, &exe)
}

/// What the holder said first: `Some(line)` (empty when it ended without saying anything), or `None` when it said nothing within
/// `limit`. The read happens on its own thread, which ends when the holder is stopped (its pipe closes).
fn first_line(child: &mut std::process::Child, limit: Duration) -> Option<String> {
    let out = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(out).read_line(&mut line);
        let _ = tx.send(line);
    });
    rx.recv_timeout(limit).ok()
}

/// `acquire` with the program that will hold the slot named (the running binary in production).
pub fn acquire_with(a: &AcquireArgs, exe: &Path) -> Result<Report, CliError> {
    check_max(a.max)?;
    let owner = a.owner.canonicalize().map_err(|e| {
        CliError::usage(format!("--owner {} does not exist: {e}.", a.owner.display()))
            .with_hint("Pass the job's own folder; it must exist.")
    })?;
    check_program_outside_owner(exe, &owner)?;
    check_root_outside_owner(&a.root, &owner)?;
    std::fs::create_dir_all(&a.root).map_err(|e| {
        CliError::failure(format!("{} cannot be created: {e}.", a.root.display())).with_hint("Pass a writable CI home with --root.")
    })?;
    // The holder must not inherit the runner's pipes (run 8019): a holder that keeps the step's pipe is ended with the step.
    // `spawn_detached_clearing_handles` fails closed: no holder at all rather than one that frees the slot in the middle of the job.
    let mut child =
        paddock_proc::spawn_detached_clearing_handles(&mut holder_command(exe, &holder_args(a, &owner), &a.root)).map_err(|e| match e {
            paddock_proc::DetachError::Handles(_) => CliError::failure(format!("The slot holder was not started: {e}."))
                .with_hint("Report it with the job's log; the slot was not taken, so no other job is affected."),
            paddock_proc::DetachError::Spawn { .. } => {
                CliError::failure(format!("The slot holder could not be started: {e}.")).with_hint("Check that the dev binary can run.")
            }
        })?;
    let Some(line) = first_line(&mut child, Duration::from_secs(a.holder_timeout_secs.max(1))) else {
        // It neither took a slot nor said "none" in time: stop it, so it cannot take one later and keep it.
        let _ = child.kill();
        let _ = child.wait();
        return Err(CliError::failure(format!(
            "The slot holder did not answer within {} seconds.",
            a.holder_timeout_secs.max(1)
        ))
        .with_hint("The machine may be overloaded; try again, or run `./dev ci slot hold` by hand to see why."));
    };
    if line.is_empty() {
        let _ = child.wait();
        return Err(
            CliError::failure("The slot holder ended before it took a slot.").with_hint("Run `./dev ci slot hold` by hand to see why.")
        );
    }
    match parse_announcement(&line)? {
        Announcement::Slot(id, token) => {
            // The holder lives on by itself; reap nothing, wait for nothing.
            drop(child);
            Ok(Report::new(
                json!({"slot": id.to_string(), "token": token, "max": a.max, "holder_pid": holder_pid(&a.root, id)}),
                format!("{id} {token}"),
            ))
        }
        Announcement::None => {
            let _ = child.wait();
            Err(CliError::refused(format!(
                "All {} slots are busy (the holder kept trying for {:.1} s).",
                a.max,
                a.acquire_grace_ms as f64 / 1000.0
            ))
            .with_hint("Wait for a job to finish or raise --max."))
        }
    }
}

fn holder_pid(root: &Path, id: SlotId) -> Value {
    FsLocks::new(root).read_owner(id).map_or(Value::Null, |o| json!(o.pid))
}

/// `dev ci slot hold` (the holder).
pub fn hold(a: &HoldArgs) -> Result<Report, CliError> {
    check_max(a.max)?;
    // Before anything else: descriptors inherited from the job (a pipe the runner waits on) must not outlive the step.
    paddock_proc::close_inherited();
    let locks = FsLocks::new(&a.root);
    let end = paddock_slots::hold_with(
        &locks,
        a.max,
        &a.owner,
        Duration::from_millis(a.poll_ms.max(1)),
        (a.max_hold_secs > 0).then(|| Duration::from_secs(a.max_hold_secs)),
        Duration::from_millis(a.acquire_grace_ms),
        &mut std::io::stdout(),
    )
    .map_err(failure)?;
    Ok(Report::new(json!({"ended": end.map(|(id, why)| format!("{id} {why:?}"))}), ""))
}

/// `dev ci slot release`.
pub fn release(a: &ReleaseArgs) -> Result<Report, CliError> {
    let id: SlotId = a.slot.parse().map_err(failure)?;
    release_slot(
        &FsLocks::new(&a.root),
        id,
        &a.token,
        Duration::from_secs(a.timeout_secs),
        Duration::from_millis(20),
    )
    .map_err(failure)?;
    Ok(Report::new(
        json!({"slot": id.to_string(), "released": true}),
        format!("{id} released"),
    ))
}

/// The variable that tells an `idle-run` command which slot it holds.
pub const IDLE_SLOT_VAR: &str = "PADDOCK_CI_IDLE_SLOT";

/// `dev ci slot idle-run`.
pub fn idle_run(a: &IdleRunArgs) -> Result<Report, CliError> {
    let id: SlotId = a.slot.parse().map_err(failure)?;
    let Some((program, args)) = a.command.split_first() else {
        return Err(CliError::usage("Give the command to run after --.")
            .with_hint("For example: idle-run --slot s3 --root <home> -- bash sweep.sh"));
    };
    let locks = FsLocks::new(&a.root);
    let ran = with_idle(&locks, id, || {
        paddock_proc::command(program)
            .args(args)
            .env(IDLE_SLOT_VAR, id.to_string())
            .stdin(Stdio::null())
            .status()
    })
    .map_err(failure)?;
    match ran {
        None => Ok(Report::new(
            json!({"slot": id.to_string(), "ran": false}),
            format!("{id} busy: skipped"),
        )),
        Some(Ok(status)) if status.success() => Ok(Report::new(json!({"slot": id.to_string(), "ran": true}), format!("{id} idle: ran"))),
        Some(Ok(status)) => {
            Err(CliError::failure(format!("The command run on idle slot {id} failed ({status})."))
                .with_hint("Its own output above says why."))
        }
        Some(Err(e)) => Err(CliError::failure(format!("{program} could not be started: {e}.")).with_hint("Check the command after --.")),
    }
}

/// `dev ci slot status`.
pub fn status(a: &StatusArgs) -> Result<Report, CliError> {
    check_max(a.max)?;
    let locks = FsLocks::new(&a.root);
    let states = slot_states(&locks, a.max).map_err(failure)?;
    let rows: Vec<Value> = states
        .iter()
        .map(|s| {
            let owner = if s.busy { locks.read_owner(s.id) } else { None };
            json!({
                "slot": s.id.to_string(),
                "busy": s.busy,
                "pid": owner.as_ref().map(|o| o.pid),
                "owner": owner.as_ref().map(|o| o.owner.display().to_string()),
            })
        })
        .collect();
    let text = states
        .iter()
        .map(|s| format!("{} {}", s.id, if s.busy { "busy" } else { "free" }))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Report::new(json!({"slots": rows}), text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcements_are_a_slot_or_none() {
        assert_eq!(
            parse_announcement("s3 tok-1\n").unwrap(),
            Announcement::Slot(SlotId::new(3).unwrap(), "tok-1".into())
        );
        assert_eq!(parse_announcement("none\n").unwrap(), Announcement::None);
        assert!(parse_announcement("s3").is_err(), "a slot without its token is refused");
        assert!(parse_announcement("hello").is_err());
    }

    #[test]
    fn max_is_bounded() {
        assert!(check_max(1).is_ok() && check_max(MAX_SLOTS).is_ok());
        assert!(check_max(0).is_err() && check_max(MAX_SLOTS + 1).is_err());
    }

    #[test]
    fn holder_arguments_carry_everything_the_holder_needs() {
        let a = AcquireArgs {
            max: 4,
            owner: "o".into(),
            root: "r".into(),
            poll_ms: 7,
            max_hold_secs: 9,
            holder_timeout_secs: 30,
            acquire_grace_ms: 1234,
        };
        let args = holder_args(&a, Path::new("/abs/o"));
        assert_eq!(args[..3], ["ci", "slot", "hold"]);
        assert!(args.windows(2).any(|w| w == ["--owner", "/abs/o"]));
        assert!(args.windows(2).any(|w| w == ["--poll-ms", "7"]));
        assert!(args.windows(2).any(|w| w == ["--max-hold-secs", "9"]));
        assert!(args.windows(2).any(|w| w == ["--acquire-grace-ms", "1234"]));
    }

    #[test]
    fn the_holder_starts_in_the_ci_home_with_its_own_pipe_and_nothing_else() {
        let cmd = holder_command(Path::new("dev"), &["ci".to_string()], Path::new("ci-home"));
        assert_eq!(cmd.get_current_dir(), Some(Path::new("ci-home")));
        assert_eq!(cmd.get_args().collect::<Vec<_>>(), ["ci"]);
    }

    #[test]
    fn a_program_inside_the_job_folder_is_refused_and_one_outside_is_fine() {
        let d = tempfile::tempdir().unwrap();
        let (job, tools) = (d.path().join("job"), d.path().join("tools"));
        std::fs::create_dir_all(job.join("target/release")).unwrap();
        std::fs::create_dir_all(&tools).unwrap();
        let inside = job.join("target/release/paddock-dev");
        let outside = tools.join("paddock-dev");
        std::fs::write(&inside, "x").unwrap();
        std::fs::write(&outside, "x").unwrap();
        let err = check_program_outside_owner(&inside, &job).unwrap_err();
        assert_eq!(err.exit, paddock_cli::Exit::Usage);
        assert!(err.text().contains("inside the job folder") && err.text().contains("PADDOCK_DEV_TARGET_DIR"));
        assert!(check_program_outside_owner(&outside, &job).is_ok());
        // A path that does not exist (yet) is still judged by where it would be.
        assert!(check_program_outside_owner(&job.join("not-built-yet"), &job).is_err());
        assert!(check_program_outside_owner(&tools.join("not-built-yet"), &job).is_ok());
    }

    #[test]
    fn a_ci_home_inside_the_job_folder_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let job = d.path().join("job");
        std::fs::create_dir(&job).unwrap();
        let err = check_root_outside_owner(&job.join("ci"), &job).unwrap_err();
        assert_eq!(err.exit, paddock_cli::Exit::Usage);
        assert!(err.text().contains("inside the job folder") && err.text().contains("CI home outside"));
        assert!(check_root_outside_owner(&d.path().join("ci"), &job).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_holder_that_never_answers_is_stopped_and_reported() {
        let d = tempfile::tempdir().unwrap();
        let job = d.path().join("job");
        std::fs::create_dir(&job).unwrap();
        let silent = d.path().join("silent-dev");
        std::fs::write(&silent, "#!/bin/sh\nexec sleep 30\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&silent, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut a = args(&d.path().join("ci"), &job);
        a.holder_timeout_secs = 1;
        let err = acquire_with(&a, &silent).unwrap_err();
        assert_eq!(err.exit, paddock_cli::Exit::Failure);
        assert!(err.text().contains("did not answer within 1 seconds"), "{}", err.text());
    }

    #[test]
    fn a_path_with_nothing_existing_in_it_is_taken_as_it_is() {
        assert_eq!(
            canonical_even_if_missing(Path::new("no-such-relative-path")),
            Path::new("no-such-relative-path")
        );
    }

    fn args(root: &Path, owner: &Path) -> AcquireArgs {
        AcquireArgs {
            max: 1,
            owner: owner.to_path_buf(),
            root: root.to_path_buf(),
            poll_ms: 20,
            max_hold_secs: 0,
            holder_timeout_secs: 30,
            acquire_grace_ms: 50,
        }
    }

    #[test]
    fn a_program_that_cannot_start_is_a_failure_that_names_the_next_step() {
        let d = tempfile::tempdir().unwrap();
        let job = d.path().join("job");
        std::fs::create_dir(&job).unwrap();
        let err = acquire_with(&args(&d.path().join("ci"), &job), &d.path().join("no-such-dev")).unwrap_err();
        assert_eq!(err.exit, paddock_cli::Exit::Failure);
        assert!(err.text().contains("could not be started") && err.text().contains("dev binary"));
    }

    #[test]
    fn a_ci_home_that_cannot_be_created_is_reported_before_anything_starts() {
        let d = tempfile::tempdir().unwrap();
        let job = d.path().join("job");
        std::fs::create_dir(&job).unwrap();
        let file = d.path().join("a-file");
        std::fs::write(&file, "x").unwrap();
        let err = acquire_with(&args(&file.join("ci"), &job), &d.path().join("dev")).unwrap_err();
        assert!(err.text().contains("cannot be created") && err.text().contains("--root"));
    }

    #[test]
    fn slot_errors_map_to_exit_kinds() {
        assert_eq!(failure(SlotError::Invalid("x".into())).exit, paddock_cli::Exit::Usage);
        assert_eq!(failure(SlotError::ReleaseTimeout("s1".into())).exit, paddock_cli::Exit::Failure);
    }
}
