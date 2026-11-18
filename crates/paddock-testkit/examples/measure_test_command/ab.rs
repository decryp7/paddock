//! `--ab <n>`: which part of a `TestCommand` start costs the time on a machine (ci.yml run 8245). Every variant runs the
//! merge_check fixture's sequence (`git init`, `git add`, `git commit`, `git rev-parse`) in a new repository and changes one
//! thing against the plain start main's fixture used (`Command::new("git")`, the inherited environment plus the hermetic git
//! variables, no creation flags):
//!
//! - `plain`: that start, as main's fixture made it;
//! - `plain-resolved`: the same, started by the full path `TestCommand` resolves (which `git.exe` it is);
//! - `plain-no-window`: the same with `CREATE_NO_WINDOW` (the flag `TestCommand` sets);
//! - `plain-cleared-env`: the same with `TestCommand`'s cleared environment, copied from one command made once (no private
//!   folder per start, no flag);
//! - `cleared-no-ceiling`, `cleared-machine-path`, `cleared-machine-folders`: the cleared environment with one part put back as
//!   the machine has it (no `GIT_CEILING_DIRECTORIES`; the machine's `PATH`; the machine's home, app-data, XDG and temp variables);
//! - `plain-ceiling`: the plain start plus `TestCommand`'s `GIT_CEILING_DIRECTORIES` alone;
//! - `tc-per-command`: `TestCommand::new` per start, one private home per command (the train's fixture);
//! - `tc-per-test`: `TestCommand::new_in` with one home shared by the sequence (one home per test);
//! - `tc-no-flag`: `TestCommand::new` per start with the creation flags put back to none;
//! - `tc-inherited-env`: `TestCommand::new` per start with the inherited environment put back (private folder and flag kept);
//! - `tc-build-only`: four `TestCommand::new` and drops, nothing started (the builder's own cost).
//!
//! Each round runs every variant once, in an order rotated per round, so a busy moment costs them alike.

use paddock_testkit::TestCommand;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

const VARIANTS: [&str; 13] = [
    "plain",
    "plain-resolved",
    "plain-no-window",
    "plain-cleared-env",
    "cleared-no-ceiling",
    "cleared-machine-path",
    "cleared-machine-folders",
    "plain-ceiling",
    "tc-per-command",
    "tc-per-test",
    "tc-no-flag",
    "tc-inherited-env",
    "tc-build-only",
];

/// The folder variables `TestCommand` points into its private folder.
const FOLDERS: [&str; 11] = [
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
    "TMPDIR",
    "TEMP",
    "TMP",
];

/// The four starts of one sequence, with the arguments in order; `add` needs a file, written before it.
const STEPS: [&[&str]; 4] = [
    &["init", "-q", "-b", "main"],
    &["add", "--all"],
    &[
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "-m",
        "initial",
    ],
    &["rev-parse", "HEAD"],
];

/// The git variables main's merge_check fixture set on every start.
fn hermetic(cmd: &mut Command) {
    let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", empty)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid");
}

fn hermetic_tc(cmd: &mut TestCommand) {
    let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", empty)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid");
}

fn check(out: &std::process::Output, what: &str) {
    assert!(
        out.status.success(),
        "{what}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[allow(clippy::disallowed_methods)] // the plain starts are the baseline being measured, each awaited here
fn plain(program: &OsString) -> Command {
    Command::new(program)
}

#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(paddock_env::command::CREATE_NO_WINDOW);
}
#[cfg(not(windows))]
fn no_window(_cmd: &mut Command) {}

#[cfg(windows)]
fn no_flags(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0);
}
#[cfg(not(windows))]
fn no_flags(_cmd: &mut Command) {}

/// One sequence of `variant` in a new repository under `root`; its wall time in milliseconds.
fn sequence(
    variant: &str,
    root: &Path,
    resolved: &OsString,
    cleared: &[(OsString, OsString)],
) -> f64 {
    let repo = tempfile::tempdir_in(root).unwrap();
    let shared = tempfile::tempdir_in(root).unwrap();
    let t = Instant::now();
    for (i, args) in STEPS.iter().enumerate() {
        if i == 1 {
            std::fs::write(repo.path().join("a.txt"), "x\n").unwrap();
        }
        let what = format!("{variant} git {args:?}");
        match variant {
            "plain"
            | "plain-resolved"
            | "plain-no-window"
            | "plain-cleared-env"
            | "cleared-no-ceiling"
            | "cleared-machine-path"
            | "cleared-machine-folders"
            | "plain-ceiling" => {
                let mut c = plain(&if variant == "plain" {
                    OsString::from("git")
                } else {
                    resolved.clone()
                });
                if variant == "plain-no-window" {
                    no_window(&mut c);
                }
                if variant == "plain-ceiling"
                    && let Some((k, v)) =
                        cleared.iter().find(|(k, _)| k == "GIT_CEILING_DIRECTORIES")
                {
                    c.env(k, v);
                }
                if variant == "plain-cleared-env" || variant.starts_with("cleared-") {
                    #[allow(clippy::disallowed_methods)]
                    // copies TestCommand's own cleared environment, the thing measured
                    c.env_clear();
                    c.envs(cleared.iter().map(|(k, v)| (k, v)));
                    match variant {
                        "cleared-no-ceiling" => {
                            c.env_remove("GIT_CEILING_DIRECTORIES");
                        }
                        "cleared-machine-path" => {
                            if let Some(p) = std::env::var_os("PATH") {
                                c.env("PATH", p);
                            }
                        }
                        "cleared-machine-folders" => {
                            for k in FOLDERS {
                                match std::env::var_os(k) {
                                    Some(v) => c.env(k, v),
                                    None => c.env_remove(k),
                                };
                            }
                        }
                        _ => {}
                    }
                }
                hermetic(&mut c);
                check(
                    &c.current_dir(repo.path()).args(*args).output().unwrap(),
                    &what,
                );
            }
            "tc-per-command" => {
                let mut c = TestCommand::new("git");
                hermetic_tc(&mut c);
                check(
                    &c.current_dir(repo.path()).args(*args).output().unwrap(),
                    &what,
                );
            }
            "tc-per-test" => {
                let mut c = TestCommand::new_in("git", shared.path());
                hermetic_tc(&mut c);
                check(
                    &c.current_dir(repo.path()).args(*args).output().unwrap(),
                    &what,
                );
            }
            "tc-no-flag" | "tc-inherited-env" => {
                let mut tc = TestCommand::new("git");
                hermetic_tc(&mut tc);
                tc.current_dir(repo.path()).args(*args);
                let (mut c, _guard) = tc.into_parts();
                if variant == "tc-no-flag" {
                    no_flags(&mut c);
                } else {
                    #[allow(clippy::disallowed_methods)]
                    // puts the inherited environment back, the thing measured
                    c.env_clear();
                    c.envs(std::env::vars_os());
                    hermetic(&mut c);
                }
                check(&c.output().unwrap(), &what);
            }
            "tc-build-only" => {
                drop(TestCommand::new("git"));
            }
            other => unreachable!("{other}"),
        }
    }
    t.elapsed().as_secs_f64() * 1000.0
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

/// Runs `rounds` rounds and prints the facts, then one table row per variant.
pub fn run(rounds: usize) {
    let root = tempfile::tempdir().unwrap();
    let probe = TestCommand::new("git");
    let resolved = probe.get_program().to_os_string();
    let cleared = probe.environment();
    let report = probe.path_report();
    println!(
        "ab: git resolved by TestCommand: {}",
        resolved.to_string_lossy()
    );
    println!("ab: temp folder: {}", std::env::temp_dir().display());
    println!(
        "ab: private folder of a command: {}",
        probe.private_dir().display()
    );
    println!("ab: child PATH: {:?}", report.child);
    let mut names: Vec<_> = cleared
        .iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    names.sort();
    println!("ab: cleared environment names: {}", names.join(" "));
    drop(probe);
    // One sequence of each first, not counted: the first starts of a process pay for disk caches and the kit's own first call.
    for v in VARIANTS {
        sequence(v, root.path(), &resolved, &cleared);
    }
    let mut times: Vec<Vec<f64>> = vec![Vec::new(); VARIANTS.len()];
    for r in 0..rounds {
        for k in 0..VARIANTS.len() {
            let i = (k + r) % VARIANTS.len();
            times[i].push(sequence(VARIANTS[i], root.path(), &resolved, &cleared));
        }
    }
    let base = {
        let mut s = times[0].clone();
        s.sort_by(f64::total_cmp);
        pct(&s, 0.5)
    };
    println!(
        "ab: {rounds} rounds, ms per sequence of 4 git starts (tc-build-only: 4 builds and drops, no start)"
    );
    println!("| variant | median | p90 | max | median / plain |");
    println!("|---|---:|---:|---:|---:|");
    for (v, t) in VARIANTS.iter().zip(&times) {
        let mut s = t.clone();
        s.sort_by(f64::total_cmp);
        let m = pct(&s, 0.5);
        println!(
            "| {v} | {m:.1} | {:.1} | {:.1} | {:.2} |",
            pct(&s, 0.9),
            s[s.len() - 1],
            m / base
        );
    }
}
