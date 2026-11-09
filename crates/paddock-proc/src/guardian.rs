//! The orphan guard (Unix): a child's process group is ended, and its folder removed, when the process that owns the
//! child dies without ending it (a SIGKILL, a crash, a test runner's kill). Without it the child's tree runs on forever:
//! the owner's own end-of-life code (the `Drop` of [`crate::Supervised`], a time-limit watchdog thread) dies with the
//! owner. Windows needs none: the child's job is kill-on-close, and the job handle closes when the owner dies.
//!
//! Shape: two small `/bin/sh` processes per child, started right after it.
//! - The **keeper** joins the child's process group and blocks reading a pipe whose write end only the guardian holds. While
//!   it exists (alive, or a zombie not yet reaped) the group's id cannot be handed to another group, so the guardian's
//!   signal to the group can only reach the child's tree (rule 53, ADR 0033). It is the owner's child (reaped when the
//!   guard is disarmed).
//! - The **guardian** runs in a group of its own, detached from the owner (its starting shell exits at once), and blocks
//!   reading a pipe whose write end only the owner holds (close-on-exec, so no other child inherits it). The owner writes
//!   `done` before it ends the child itself ([`Guard`]'s `Drop`): the guardian then exits and does nothing. End of file
//!   without `done` means the owner died: if the keeper is still in the group, the guardian sends SIGKILL to the group,
//!   waits until it is empty (bounded, 10 s), removes the folder it was given and exits.
//!
//! Not done: `PR_SET_PDEATHSIG` (Linux). It needs a `pre_exec` hook, which the family bans (lint/clippy.toml: it forces
//! fork+exec, whose forks carry inheritable descriptors), and it fires when the *thread* that started the child ends
//! (prctl(2): "the parent in this case is considered to be the thread that created this process"), so a browser started
//! from a short-lived worker thread would be killed while its owner runs.
//!
//! Limits: a child that leaves its process group (`setsid`) is outside the guard, as it is outside the owner's tree kill.
//! An owner killed in the moment between the child's start and the guard's is not covered; the caller's start-up sweep is
//! the backstop for its folder. Without `/bin/sh` or `ps` the guard is not started or does nothing, and the start says so
//! in the log.

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// The keeper: reads its standard input (a pipe the guardian holds) until the guardian has gone.
const KEEPER: &str = "read -r _; :";

/// The guardian's start: moves the pipes off the standard descriptors, starts the guardian in the background and exits.
/// `$0` is `paddock-guardian-<group>` (it names the guardian in a process list), `$1` the child's process group, `$2` the
/// keeper's pid; the folder comes in [`FOLDER_ENV`], so no process list shows it (a caller that ends what mentions its
/// folder never meets the guardian). The group signal is the guard's one kill outside `sys.rs` (C53 cannot see a shell
/// string): it is sent only while the keeper, by its group, still holds the group's id, so it reaches the child's tree only.
/// `kill -s <signal> -- -<group>` is the spelling both dash (Debian's `/bin/sh`) and bash accept: dash rejects
/// `kill -KILL -- -<group>` ("Illegal number"), which left the group running on Linux.
const GUARDIAN: &str = r#"exec 3<&0 4>&1 </dev/null >/dev/null 2>&1
(
  if read -r word <&3 && [ "$word" = done ]; then exit 0; fi
  held=$(ps -o pgid= -p "$2" 2>/dev/null | tr -d ' ')
  [ "$held" = "$1" ] || exit 0
  kill -s KILL -- "-$1"
  n=0
  while kill -s 0 -- "-$1" 2>/dev/null && [ "$n" -lt 200 ]; do sleep 0.05; n=$((n + 1)); done
  if [ -n "$PADDOCK_GUARDED_FOLDER" ]; then rm -rf -- "$PADDOCK_GUARDED_FOLDER"; fi
) &
exit 0"#;

/// The folder the guardian removes after the group.
const FOLDER_ENV: &str = "PADDOCK_GUARDED_FOLDER";

/// The owner's side of a child's orphan guard. Dropping it disarms the guard: the guardian exits without acting and the
/// keeper is ended and reaped. Drop it before ending the child's tree.
pub(crate) struct Guard {
    owner: Option<std::io::PipeWriter>,
    keeper: Child,
}

impl Guard {
    /// Guards the process group `pgid` of a child just started (and not reaped: the group is still its own), removing
    /// `folder` after the group when the owner dies.
    pub(crate) fn start(pgid: u32, folder: Option<&Path>) -> std::io::Result<Guard> {
        let group = i32::try_from(pgid).map_err(std::io::Error::other)?;
        let (keeper_read, keeper_write) = std::io::pipe()?;
        let (owner_read, owner_write) = std::io::pipe()?;
        let mut keeper = Command::new("/bin/sh")
            .args(["-c", KEEPER, "paddock-keeper"])
            .stdin(keeper_read)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(group)
            .spawn()?;
        let folder = folder.map(|f| f.as_os_str().to_owned()).unwrap_or_default();
        let started = Command::new("/bin/sh")
            .args(["-c", GUARDIAN])
            .arg(format!("paddock-guardian-{pgid}"))
            .arg(pgid.to_string())
            .arg(keeper.id().to_string())
            .env(FOLDER_ENV, folder)
            .stdin(owner_read)
            .stdout(keeper_write)
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            // The starting shell exits as soon as the guardian runs in the background.
            .and_then(|mut s| s.wait());
        match started {
            Ok(status) if status.success() => Ok(Guard {
                owner: Some(owner_write),
                keeper,
            }),
            other => {
                crate::cleanup::logged(
                    "ending the keeper of a guard that did not start",
                    keeper.kill(),
                );
                crate::cleanup::logged(
                    "reaping the keeper of a guard that did not start",
                    keeper.wait(),
                );
                Err(match other {
                    Err(e) => e,
                    Ok(status) => {
                        std::io::Error::other(format!("the guardian's shell ended with {status}"))
                    }
                })
            }
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(mut w) = self.owner.take() {
            crate::cleanup::logged("disarming the orphan guard", w.write_all(b"done\n"));
        }
        // Ended through its handle: it is this process's own unreaped child.
        crate::cleanup::logged("ending the guard's keeper", self.keeper.kill());
        crate::cleanup::logged("reaping the guard's keeper", self.keeper.wait());
    }
}

#[cfg(test)]
mod tests;
