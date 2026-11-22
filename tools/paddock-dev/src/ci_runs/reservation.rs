//! The train's CI reservation (2026-11-16, user and coordinator: "the train sequence always goes first").
//!
//! A train's CI is a sequence of workflows. `ci dispatch`'s runner guard sees only a busy runner, so a member run (a trial, the
//! member gate) could take the gap between two workflows of the sequence: run 8166 did, and train 12's Pitwall gate then waited
//! behind it on the macOS runner (ER-1: the release comes first). So the integrator's sequence **holds a reservation for its whole
//! duration** (`scripts/train-ci-hold.sh <label> -- <the sequence>`: one kernel file lock under `<lock root>/train-ci`, freed when
//! the holder ends or at its `--max-hold-secs`), and every dispatch that is not itself a train's waits on it, bounded by the
//! caller's `--max`, then reports "still held" (the member gate: `not measured`). A train dispatch never waits here.
//!
//! The lock is the machine's: all agents and the integrator run on one Mac, and the shared runners are one machine each. A lock
//! that does not exist (nobody ever took it) is free.

use crate::wait::{Clock, SystemClock, fmt_duration};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// Who holds the reservation, or that nobody does. The real one is [`LockSlots`]; tests give a table.
pub trait Held {
    /// `Some(owner)` while the reservation is held; `Err` when it cannot be read (never taken for "free").
    fn holder(&mut self) -> Result<Option<String>, String>;
}

/// How the wait ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Free {
    /// Free (after waiting `waited`, zero when it was free at once).
    Yes {
        /// How long the reservation was held.
        waited: Duration,
    },
    /// Still held when the bound ended: nothing may be dispatched.
    StillHeld {
        /// The holder at the end.
        holder: String,
    },
}

/// Waits while the train reservation is held, polling every `interval`, at most `max` (ER-32: the wait always ends). A holder that
/// cannot be read is an error, not "free".
pub fn wait_free(
    held: &mut dyn Held,
    clock: &mut dyn Clock,
    max: Duration,
    interval: Duration,
    log: &mut dyn Write,
) -> Result<Free, String> {
    let mut said = false;
    loop {
        match held.holder()? {
            None => {
                let waited = if said { clock.elapsed() } else { Duration::ZERO };
                if said {
                    let _ = writeln!(
                        log,
                        "reservation: the train's CI sequence is done; going on after {}",
                        fmt_duration(waited)
                    );
                }
                return Ok(Free::Yes { waited });
            }
            Some(holder) => {
                if clock.elapsed() >= max {
                    return Ok(Free::StillHeld { holder });
                }
                if !said {
                    let _ = writeln!(
                        log,
                        "reservation: a train's CI sequence holds the runners ({holder}); this run goes after it (waiting at most {})",
                        fmt_duration(max)
                    );
                    said = true;
                }
                clock.sleep(interval.min(max.saturating_sub(clock.elapsed()).max(Duration::from_millis(1))));
            }
        }
    }
}

/// The folder of the reservation lock: `PADDOCK_TRAIN_CI_LOCK_ROOT`, else `train-ci` beside the test machines' lock folder
/// (`./dev places --get lock-root`'s parent). `None` when neither can be derived.
pub fn lock_root() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PADDOCK_TRAIN_CI_LOCK_ROOT") {
        return Some(PathBuf::from(p));
    }
    let from = std::env::current_dir().ok()?;
    let layout = crate::places::Layout::derive(&from, &|k| std::env::var(k).ok(), false);
    layout.lock_root.ok()?.parent().map(|p| p.join("train-ci"))
}

/// The real reader: `ci slot status` of the one-slot lock folder.
pub struct LockSlots {
    root: PathBuf,
}

impl LockSlots {
    /// The reader for the folder `lock_root()` names.
    pub fn new(root: PathBuf) -> LockSlots {
        LockSlots { root }
    }
}

/// The holder named by `ci slot status --json`: the owner folder of a busy slot.
pub fn holder_of(status: &serde_json::Value) -> Option<String> {
    status["slots"]
        .as_array()?
        .iter()
        .find(|s| s["busy"].as_bool() == Some(true))
        .map(|s| s["owner"].as_str().map(str::to_string).unwrap_or_else(|| "a holder".into()))
}

impl Held for LockSlots {
    fn holder(&mut self) -> Result<Option<String>, String> {
        if !self.root.exists() {
            return Ok(None); // never taken
        }
        let me = crate::tool_cache::self_exe().map_err(|e| format!("this program cannot be found: {e}"))?;
        let mut cmd = paddock_proc::command(me);
        cmd.args(["ci", "slot", "status", "--max", "1", "--json", "--root"]).arg(&self.root);
        let out = paddock_proc::run_bounded(cmd, paddock_proc::Limits::new(Duration::from_secs(60)))
            .map_err(|e| format!("`ci slot status` could not run: {e}"))?;
        if !out.success() {
            return Err(format!("`ci slot status` failed: {}", out.stderr_text().trim()));
        }
        let text = out.stdout_text();
        let json: serde_json::Value = text
            .lines()
            .find(|l| l.starts_with('{'))
            .and_then(|l| serde_json::from_str(l).ok())
            .ok_or("`ci slot status` printed no JSON")?;
        Ok(holder_of(&json))
    }
}

/// Waits on the real reservation with the real clock. `Ok(Free::Yes)` when it cannot be derived is NOT assumed: an underivable
/// root is an error (the caller reports not measured).
pub fn wait_real(max: Duration, log: &mut dyn Write) -> Result<Free, String> {
    let root = lock_root().ok_or("the train reservation folder cannot be derived (set PADDOCK_TRAIN_CI_LOCK_ROOT)")?;
    let mut clock = SystemClock::new();
    wait_free(&mut LockSlots::new(root), &mut clock, max, Duration::from_secs(15), log)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Held for the first `polls_held` reads, then free.
    struct Table {
        polls_held: usize,
        reads: usize,
        broken: bool,
    }

    impl Held for Table {
        fn holder(&mut self) -> Result<Option<String>, String> {
            if self.broken {
                return Err("unreadable".into());
            }
            self.reads += 1;
            Ok((self.reads <= self.polls_held).then(|| "train 12's sequence".to_string()))
        }
    }

    struct Fake(Duration);
    impl Clock for Fake {
        fn elapsed(&self) -> Duration {
            self.0
        }
        fn sleep(&mut self, d: Duration) {
            self.0 += d;
        }
    }

    const MAX: Duration = Duration::from_secs(600);
    const EVERY: Duration = Duration::from_secs(15);

    /// The train always goes first: a member dispatch waits while the reservation is held, and goes on once it is released.
    #[test]
    fn a_member_dispatch_waits_while_a_train_reservation_is_held_and_goes_on_when_it_is_released() {
        let mut held = Table {
            polls_held: 4,
            reads: 0,
            broken: false,
        };
        let mut log = Vec::new();
        let got = wait_free(&mut held, &mut Fake(Duration::ZERO), MAX, EVERY, &mut log).unwrap();
        assert_eq!(held.reads, 5, "it polled until the release");
        assert_eq!(
            got,
            Free::Yes {
                waited: Duration::from_secs(60)
            }
        );
        let said = String::from_utf8(log).unwrap();
        assert!(said.contains("a train's CI sequence holds the runners"), "{said}");
    }

    #[test]
    fn a_free_reservation_costs_no_wait_and_says_nothing() {
        let mut held = Table {
            polls_held: 0,
            reads: 0,
            broken: false,
        };
        let mut log = Vec::new();
        assert_eq!(
            wait_free(&mut held, &mut Fake(Duration::ZERO), MAX, EVERY, &mut log).unwrap(),
            Free::Yes { waited: Duration::ZERO }
        );
        assert!(log.is_empty());
    }

    /// Bounded (ER-32): a reservation held for longer than `max` is reported as still held, never waited for forever and never
    /// ignored.
    #[test]
    fn a_reservation_held_past_the_bound_is_still_held() {
        let mut held = Table {
            polls_held: usize::MAX,
            reads: 0,
            broken: false,
        };
        let got = wait_free(&mut held, &mut Fake(Duration::ZERO), MAX, EVERY, &mut Vec::new()).unwrap();
        assert_eq!(
            got,
            Free::StillHeld {
                holder: "train 12's sequence".into()
            }
        );
        assert!(held.reads <= 42, "{}", held.reads);
    }

    #[test]
    fn a_reservation_that_cannot_be_read_is_an_error_not_free() {
        let mut held = Table {
            polls_held: 0,
            reads: 0,
            broken: true,
        };
        assert!(wait_free(&mut held, &mut Fake(Duration::ZERO), MAX, EVERY, &mut Vec::new()).is_err());
    }

    #[test]
    fn the_holder_is_read_from_the_slot_status() {
        assert_eq!(
            holder_of(&json!({"slots":[{"slot":"s1","busy":false,"pid":null,"owner":null}]})),
            None
        );
        assert_eq!(
            holder_of(&json!({"slots":[{"slot":"s1","busy":true,"pid":7,"owner":"/x/owner-1"}]})),
            Some("/x/owner-1".into())
        );
        assert_eq!(holder_of(&json!({})), None);
    }
}
