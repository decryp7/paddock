//! The train-CI hold of `train release`: the reservation every member run waits on (`scripts/train-ci-hold.sh`, one kernel file
//! lock with a detached holder). The command runs only while it holds it, and proves that the way the lock scripts do: the
//! `PADDOCK_VM_LOCK_HELD` variable (pid, start time, owner folder) is believed only when `scripts/vm-lock-run.sh --check` confirms
//! it against the slot status and the owner's `who` file; a variable without a live holder is refused. The hold has no in-process
//! library (the holder is a detached process the shell scripts own), so a command that does not hold it runs itself again under
//! `train-ci-hold.sh` and ends with the child's exit code.
//!
//! **Inherited-held semantics** (as in `scripts/vm-lock.sh`): `PADDOCK_VM_LOCK_HELD` is an exported variable, so every process the
//! holder starts inherits it, and any of them proves the hold for as long as the holder lives: the check confirms the named pid
//! still has its start time, the slot status shows that owner folder holding, and the owner's `who` file names that pid. A copy of the
//! variable in a shell that outlived the holder, or one made by hand, fails (the pid or the slot no longer matches). A process
//! that is not a descendant but has the variable (a `nohup` from the sequence) is counted as held while the holder lives; that is
//! the scripts' own meaning, and the hold ends when the holder does, so such a process cannot hold anything for longer.
//!
//! A held child that is ended (the hard deadline, Ctrl-C, SIGTERM) is ended as a tree, which skips the script's exit trap; the hold
//! is then released explicitly (`release_dead`): an acquire of the same lock takes a dead holder over and releases it at once, so a
//! member run that waits on the reservation (`reservation::wait_real`) never waits on a holder that is gone.

use paddock_proc::{Limits, Outcome, Sink, command, run_bounded, run_waited_cancellable};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// The exit code of the hold when another train's sequence holds it (`scripts/vm-lock.sh`: EX_TEMPFAIL).
pub const BUSY: i32 = 75;
/// The variable the re-executed child carries: it is only ever used to refuse a second re-execution, never to skip the proof.
pub const REEXEC_VAR: &str = "PADDOCK_TRAIN_RELEASE_REEXEC";
/// What the held child may take beyond `--max`: its own final steps; the sequence bounds itself with `--max`.
pub const GRACE: Duration = Duration::from_secs(10 * 60);

/// The hold, as the command sees it.
pub trait Hold {
    /// `Ok` when this process holds the train-CI reservation (proven); else why not.
    fn proven(&mut self) -> Result<(), String>;
    /// Runs this command again under the hold, to its end or the hard deadline: the child's exit code.
    fn run_under_hold(&mut self) -> i32;
}

/// What the command does next.
#[derive(Debug, PartialEq, Eq)]
pub enum Enter {
    /// Run the sequence here: the hold is proven.
    Run,
    /// The child ran under the hold; end with its exit code.
    Exit(i32),
    /// Stop with this `::error::` line.
    Refuse(String),
}

/// Decides how to enter: run here when the hold is proven, else re-execute once under the hold (and refuse when this already is
/// the re-executed child, so it can never loop).
pub fn enter(hold: &mut dyn Hold, reexecuted: bool, log: &mut dyn Write) -> Enter {
    match hold.proven() {
        Ok(()) => Enter::Run,
        Err(why) if reexecuted => Enter::Refuse(format!(
            "::error::train release: the train-CI hold is not proven in the re-executed command ({why}). Next step: run `scripts/train-ci-hold.sh --status`, and start it again; never set PADDOCK_VM_LOCK_HELD by hand."
        )),
        Err(why) => {
            let _ = writeln!(
                log,
                "train release: not holding the train-CI reservation ({why}); taking it (scripts/train-ci-hold.sh)"
            );
            let code = hold.run_under_hold();
            if code == BUSY {
                let _ = writeln!(
                    log,
                    "::error::train release: another train's CI sequence holds the runners. Next step: `scripts/train-ci-hold.sh --status` names it; wait for it to end, then run this again."
                );
            }
            Enter::Exit(code)
        }
    }
}

/// The variables the proof reads, given explicitly so a test never depends on the ambient ones.
#[derive(Debug, Clone, Default)]
pub struct HoldEnv {
    /// `PADDOCK_VM_LOCK_HELD`.
    pub held: Option<String>,
    /// `PADDOCK_VM_LOCK_ROOT`.
    pub vm_root: Option<String>,
    /// `PADDOCK_TRAIN_CI_LOCK_ROOT`.
    pub train_root: Option<String>,
    /// The reservation folder the family derives (`./dev places --get lock-root`, its sibling `train-ci`): the one the lock must be.
    pub expected_root: Option<PathBuf>,
}

impl HoldEnv {
    /// From this process's environment.
    pub fn ambient() -> HoldEnv {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        HoldEnv {
            held: get("PADDOCK_VM_LOCK_HELD"),
            vm_root: get("PADDOCK_VM_LOCK_ROOT"),
            train_root: get("PADDOCK_TRAIN_CI_LOCK_ROOT"),
            expected_root: crate::ci_runs::reservation::lock_root(),
        }
    }
}

/// The proof: the variable names a live holder (`scripts/vm-lock-run.sh --check`), and the lock is the train-CI one (the VM lock
/// root is the train-CI root, which is the family's).
pub fn proof(repo: &Path, env: &HoldEnv) -> Result<(), String> {
    let held = env.held.as_deref().ok_or("PADDOCK_VM_LOCK_HELD is not set")?;
    let (vm, train) = match (&env.vm_root, &env.train_root) {
        (Some(v), Some(t)) => (v, t),
        _ => return Err("PADDOCK_VM_LOCK_ROOT and PADDOCK_TRAIN_CI_LOCK_ROOT are not both set".into()),
    };
    if vm != train {
        return Err(format!("the lock held is {vm}, not the train-CI reservation {train}"));
    }
    // The lock is the family's reservation folder, as `./dev places` derives it (an explicit PADDOCK_TRAIN_CI_LOCK_ROOT is the
    // same value the hold script exported, so it is compared with the derivation all the same; an underivable one is refused).
    match &env.expected_root {
        Some(expected) if expected == Path::new(train) => {}
        Some(expected) => {
            return Err(format!(
                "the lock root is {train}, not the reservation folder {} that ./dev places derives",
                expected.display()
            ));
        }
        None => return Err("the reservation folder cannot be derived (./dev places --get lock-root)".into()),
    }
    let mut cmd = command("bash");
    cmd.arg(repo.join("scripts/vm-lock-run.sh")).arg("--check");
    cmd.env("PADDOCK_VM_LOCK_HELD", held).env("PADDOCK_VM_LOCK_ROOT", vm);
    let out = run_bounded(cmd, Limits::new(Duration::from_secs(90))).map_err(|e| format!("the lock check could not run: {e}"))?;
    match out.outcome {
        Outcome::Exited(st) if st.success() => Ok(()),
        Outcome::Exited(_) => Err("PADDOCK_VM_LOCK_HELD names no live holder (pid, start time or owner do not match the slot)".into()),
        _ => Err("the lock check did not finish in time".into()),
    }
}

/// The real hold: the proof of [`proof`], and the re-execution under `scripts/train-ci-hold.sh` with a hard deadline.
pub struct ShellHold {
    /// The repository (the worktree) holding `scripts/`.
    pub repo: PathBuf,
    /// This program's arguments after its name, as they were given.
    pub args: Vec<String>,
    /// The longest the sequence takes (`--max`); the child is ended at this plus [`GRACE`].
    pub max: Duration,
}

impl Hold for ShellHold {
    fn proven(&mut self) -> Result<(), String> {
        proof(&self.repo, &HoldEnv::ambient())
    }

    fn run_under_hold(&mut self) -> i32 {
        let exe = match crate::tool_cache::self_exe() {
            Ok(e) => e,
            Err(e) => {
                eprintln!(
                    "::error::train release: this program cannot be found ({e}). Next step: run it again from the repository's ./dev."
                );
                return 1;
            }
        };
        let mut cmd = command("bash");
        cmd.arg(self.repo.join("scripts/train-ci-hold.sh"))
            .arg("train-release")
            .arg("--")
            .arg(exe)
            .args(&self.args);
        cmd.env(REEXEC_VAR, "1");
        // Ctrl-C and SIGTERM end the child (as a tree) and the hold is released, instead of leaving a sequence running unattended.
        let stop = Arc::new(AtomicBool::new(false));
        crate::pre_push::stop_on_signals(&stop);
        let result = run_waited_cancellable(cmd, self.max + GRACE, Sink::Inherit, &stop);
        let code = match result {
            Ok((Outcome::Exited(st), _)) => st.code().unwrap_or(1),
            Ok((_, cancelled)) => {
                let why = if cancelled {
                    "it was interrupted (Ctrl-C or SIGTERM)".to_string()
                } else {
                    format!("its hard deadline passed ({} s, --max plus a grace)", (self.max + GRACE).as_secs())
                };
                eprintln!(
                    "::error::train release: the held command was ended: {why}. Next step: read its log above, then run it again with the runs that are green given as --since-run passed:<run> or --reuse."
                );
                if cancelled { 130 } else { 124 }
            }
            Err(e) => {
                eprintln!(
                    "::error::train release: the hold could not be started: {e}. Next step: check scripts/train-ci-hold.sh and the lock folder (./dev places)."
                );
                return 1;
            }
        };
        if code == 124 || code == 130 {
            self.release_dead();
        }
        code
    }
}

impl ShellHold {
    /// Releases the reservation of a held child that was ended as a tree (its exit trap did not run): acquiring the same lock
    /// takes a dead holder over, and the command it runs (`true`) ends at once and releases it. A lock that a live sequence holds
    /// is not ours: the acquire is refused (exit 75) and nothing is changed.
    fn release_dead(&self) {
        let mut cmd = command("bash");
        cmd.arg(self.repo.join("scripts/train-ci-hold.sh"))
            .arg("train-release-cleanup")
            .arg("--")
            .arg("true");
        match run_bounded(cmd, Limits::new(Duration::from_secs(300))) {
            Ok(out) if out.success() => eprintln!("train release: the hold of the ended command was released"),
            Ok(out) => eprintln!(
                "::warning::train release: the hold of the ended command could not be released ({}); `scripts/train-ci-hold.sh --status` names the holder",
                out.stderr_text().lines().last().unwrap_or("no message").trim()
            ),
            Err(e) => eprintln!("::warning::train release: the hold of the ended command could not be released: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        proven: Result<(), String>,
        code: i32,
        ran: usize,
    }
    impl Hold for Fake {
        fn proven(&mut self) -> Result<(), String> {
            self.proven.clone()
        }
        fn run_under_hold(&mut self) -> i32 {
            self.ran += 1;
            self.code
        }
    }
    fn fake(proven: Result<(), String>, code: i32) -> Fake {
        Fake { proven, code, ran: 0 }
    }

    #[test]
    fn a_proven_hold_runs_here_and_never_re_executes() {
        let mut f = fake(Ok(()), 0);
        assert_eq!(enter(&mut f, false, &mut Vec::new()), Enter::Run);
        assert_eq!(enter(&mut f, true, &mut Vec::new()), Enter::Run);
        assert_eq!(f.ran, 0);
    }

    #[test]
    fn no_hold_re_executes_once_and_ends_with_the_childs_exit_code() {
        let mut f = fake(Err("not set".into()), 3);
        let mut log = Vec::new();
        assert_eq!(enter(&mut f, false, &mut log), Enter::Exit(3));
        assert_eq!(f.ran, 1);
        assert!(String::from_utf8(log).unwrap().contains("taking it (scripts/train-ci-hold.sh)"));
        assert_eq!(enter(&mut fake(Err("x".into()), 0), false, &mut Vec::new()), Enter::Exit(0));
    }

    /// Exit 75 is another train's sequence holding the runners: one `::error::` with the next step, the code passes through.
    #[test]
    fn a_busy_hold_ends_with_75_and_one_error_line() {
        let mut log = Vec::new();
        assert_eq!(enter(&mut fake(Err("x".into()), BUSY), false, &mut log), Enter::Exit(75));
        let text = String::from_utf8(log).unwrap();
        assert_eq!(text.matches("::error::").count(), 1, "{text}");
        assert!(text.contains("another train's CI sequence holds the runners") && text.contains("--status"));
    }

    #[test]
    fn the_re_executed_child_without_a_proven_hold_refuses_instead_of_looping() {
        let mut f = fake(Err("PADDOCK_VM_LOCK_HELD names no live holder".into()), 0);
        let Enter::Refuse(line) = enter(&mut f, true, &mut Vec::new()) else {
            panic!("must refuse")
        };
        assert!(
            line.starts_with("::error::train release: the train-CI hold is not proven") && line.contains("Next step"),
            "{line}"
        );
        assert_eq!(f.ran, 0);
    }

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// The positive control of the refusal: a variable with no live holder behind it (a pid that is not this one's, no owner folder)
    /// is refused by the real `vm-lock-run.sh --check`, and so are the other shapes that are not a proof.
    #[test]
    fn the_variable_without_a_live_holder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().display().to_string();
        let env = |held: Option<&str>, vm: Option<&str>, train: Option<&str>| HoldEnv {
            held: held.map(String::from),
            vm_root: vm.map(String::from),
            train_root: train.map(String::from),
            expected_root: Some(PathBuf::from(&root)),
        };
        let bogus = "999999\tMon Jan 1 00:00:00 2001\t/nonexistent/owner";
        let why = proof(&repo(), &env(Some(bogus), Some(&root), Some(&root))).unwrap_err();
        assert!(why.contains("names no live holder"), "{why}");
        assert!(
            proof(&repo(), &env(None, Some(&root), Some(&root)))
                .unwrap_err()
                .contains("not set")
        );
        assert!(
            proof(&repo(), &env(Some(bogus), None, Some(&root)))
                .unwrap_err()
                .contains("not both set")
        );
        assert!(
            proof(&repo(), &env(Some(bogus), Some("/other"), Some(&root)))
                .unwrap_err()
                .contains("not the train-CI reservation")
        );
    }

    /// The lock must be the family's reservation folder, and one that cannot be derived is refused.
    #[test]
    fn a_lock_elsewhere_than_the_reservation_folder_is_refused() {
        let bogus = "999999\tMon Jan 1 00:00:00 2001\t/nonexistent/owner";
        let env = |expected: Option<&str>| HoldEnv {
            held: Some(bogus.into()),
            vm_root: Some("/locks/a".into()),
            train_root: Some("/locks/a".into()),
            expected_root: expected.map(PathBuf::from),
        };
        assert!(
            proof(&repo(), &env(Some("/locks/train-ci")))
                .unwrap_err()
                .contains("not the reservation folder /locks/train-ci")
        );
        assert!(proof(&repo(), &env(None)).unwrap_err().contains("cannot be derived"));
    }
}
