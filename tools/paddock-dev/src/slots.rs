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
//! - frees the slot at once if it is killed or crashes, because the kernel drops the lock.
//!
//! No stale lock can exist, so nothing is ever reclaimed and two jobs cannot hold one slot.

use clap::Args;
use paddock_cli::{CliError, Report};
use paddock_slots::{FsLocks, SlotError, SlotId, SlotLocks, release as release_slot, slot_states};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// The most slots a caller may ask for.
pub const MAX_SLOTS: u32 = 1024;
const DEFAULT_POLL_MS: u64 = 500;

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
}

/// `release`'s arguments.
#[derive(Args)]
pub struct ReleaseArgs {
    /// The slot to let go (`s1`...).
    #[arg(long)]
    slot: String,
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

/// What the holder said on its pipe.
#[derive(Debug, PartialEq, Eq)]
pub enum Announcement {
    /// It holds this slot.
    Slot(SlotId),
    /// All slots were busy.
    None,
}

/// Reads the holder's one line.
pub fn parse_announcement(line: &str) -> Result<Announcement, CliError> {
    match line.trim() {
        "none" => Ok(Announcement::None),
        other => other
            .parse::<SlotId>()
            .map(Announcement::Slot)
            .map_err(|_| CliError::failure(format!("The slot holder said {other:?}, not a slot name."))),
    }
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
    ]
}

/// Starts the holder detached, with a pipe for its announcement and nothing else attached to this process.
fn spawn_holder(args: &[String]) -> Result<std::process::Child, CliError> {
    let exe = std::env::current_exe().map_err(|e| CliError::failure(format!("Cannot find the dev binary: {e}.")))?;
    let mut cmd = paddock_proc::command(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
        cmd.creation_flags(0x0800_0000 | 0x0000_0200);
    }
    cmd.spawn().map_err(|e| {
        CliError::failure(format!("The slot holder could not be started: {e}.")).with_hint("Check that the dev binary can run.")
    })
}

/// `dev ci slot acquire`.
pub fn acquire(a: &AcquireArgs) -> Result<Report, CliError> {
    check_max(a.max)?;
    let owner = a.owner.canonicalize().map_err(|e| {
        CliError::usage(format!("--owner {} does not exist: {e}.", a.owner.display()))
            .with_hint("Pass the job's own folder; it must exist.")
    })?;
    let mut child = spawn_holder(&holder_args(a, &owner))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CliError::failure("The slot holder has no output."))?;
    let mut line = String::new();
    let read = BufReader::new(stdout).read_line(&mut line);
    match (read, line.as_str()) {
        (Ok(n), l) if n > 0 => match parse_announcement(l)? {
            Announcement::Slot(id) => {
                // The holder lives on by itself; reap nothing, wait for nothing.
                drop(child);
                Ok(Report::new(
                    json!({"slot": id.to_string(), "max": a.max, "holder_pid": holder_pid(&a.root, id)}),
                    id.to_string(),
                ))
            }
            Announcement::None => {
                let _ = child.wait();
                Err(CliError::refused(format!("All {} slots are busy.", a.max)).with_hint("Wait for a job to finish or raise --max."))
            }
        },
        _ => {
            let _ = child.wait();
            Err(CliError::failure("The slot holder ended before it took a slot.").with_hint("Run `./dev ci slot hold` by hand to see why."))
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
    crate::fds::close_inherited();
    let locks = FsLocks::new(&a.root);
    let end = paddock_slots::hold(&locks, a.max, &a.owner, Duration::from_millis(a.poll_ms.max(1)), std::io::stdout()).map_err(failure)?;
    Ok(Report::new(json!({"ended": end.map(|(id, why)| format!("{id} {why:?}"))}), ""))
}

/// `dev ci slot release`.
pub fn release(a: &ReleaseArgs) -> Result<Report, CliError> {
    let id: SlotId = a.slot.parse().map_err(failure)?;
    release_slot(
        &FsLocks::new(&a.root),
        id,
        Duration::from_secs(a.timeout_secs),
        Duration::from_millis(20),
    )
    .map_err(failure)?;
    Ok(Report::new(
        json!({"slot": id.to_string(), "released": true}),
        format!("{id} released"),
    ))
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

/// Whether slot `id` can be taken right now (the lock is released again at once). For tests.
pub fn is_free(root: &Path, id: SlotId) -> Result<bool, SlotError> {
    Ok(FsLocks::new(root).try_lock(id)?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcements_are_a_slot_or_none() {
        assert_eq!(parse_announcement("s3\n").unwrap(), Announcement::Slot(SlotId::new(3).unwrap()));
        assert_eq!(parse_announcement("none\n").unwrap(), Announcement::None);
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
        };
        let args = holder_args(&a, Path::new("/abs/o"));
        assert_eq!(args[..3], ["ci", "slot", "hold"]);
        assert!(args.windows(2).any(|w| w == ["--owner", "/abs/o"]));
        assert!(args.windows(2).any(|w| w == ["--poll-ms", "7"]));
    }

    #[test]
    fn slot_errors_map_to_exit_kinds() {
        assert_eq!(failure(SlotError::Invalid("x".into())).exit, paddock_cli::Exit::Usage);
        assert_eq!(failure(SlotError::ReleaseTimeout("s1".into())).exit, paddock_cli::Exit::Failure);
    }
}
