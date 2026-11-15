//! `dev self hash`: the content address of the tool's binary (ADR 0022 §3.3).
//!
//! The hash is `git hash-object --stdin` over `git ls-tree HEAD -- <inputs>` followed by the `commit-hash:` and `release:`
//! lines of `<rustc> -vV` (in rustc's order) and [`flag_lines`] (the hashed environment variables and a digest of each cargo config
//! file), where `<inputs>` are the paths in `tools/paddock-dev/inputs.txt` and `<rustc>`
//! is the compiler cargo will use. The computation is `dev_launch::hash`, the one copy; the `./dev` launcher prints the same value
//! (`./dev --wrapper-hash`) and a test runs both on one repository. When an input has uncommitted changes (`git status --porcelain --untracked-files=all`), the
//! hash cannot describe the files and the wrapper builds.

use dev_launch::hash::HashError;
pub use dev_launch::hash::{
    InputHash, build_rustc, compiler_lines, config_files, flag_lines, is_hashed_var, only_names_build_folder, resolve_rustc,
};
use paddock_cli::{CliError, Report};
use paddock_proc::{Limits, command as proc_command, run_bounded};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

/// The hash the binary was built from, when the launcher passed it (`PADDOCK_DEV_HASH`).
pub const EMBEDDED: Option<&str> = option_env!("PADDOCK_DEV_EMBEDDED_HASH");

const TIMEOUT: Duration = Duration::from_secs(60);

fn to_cli(e: HashError) -> CliError {
    CliError::failure(e.message).with_hint(e.hint)
}

/// The input paths of `inputs.txt`: one per line, `#` comments and blank lines skipped, no spaces, no absolute or `..` paths.
pub fn parse_inputs(text: &str) -> Result<Vec<String>, CliError> {
    dev_launch::hash::parse_inputs(text).map_err(to_cli)
}

/// Runs programs for the hash: a port, so the computation is testable without a repository.
pub trait Exec {
    /// Runs `program args...` in `cwd` with `stdin` and returns standard output, or a message when it failed.
    fn run(&self, program: &str, args: &[&str], cwd: &Path, stdin: Option<&[u8]>) -> Result<String, CliError>;
}

/// The most one output pipe is read into: a reply past it ends the child's tree and is an error.
const MAX_REPLY: usize = 64 << 20;

/// Starts `cmd` with all three standard streams piped, leading its own process group (Unix), so that a timeout can end the
/// whole tree through `paddock_proc::kill_tree`, grandchildren included.
fn spawn_piped(cmd: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(cmd, 0);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
}

/// Reads `pipe` into `buf` until it closes; past [`MAX_REPLY`] it stops, flags `over` and drops the pipe.
fn drain(mut pipe: impl std::io::Read, buf: &std::sync::Mutex<Vec<u8>>, over: &std::sync::atomic::AtomicBool) {
    let mut chunk = [0u8; 8192];
    while let Ok(n) = pipe.read(&mut chunk) {
        let mut g = buf.lock().unwrap_or_else(|e| e.into_inner());
        if n == 0 {
            return;
        }
        if g.len() + n > MAX_REPLY {
            over.store(true, std::sync::atomic::Ordering::SeqCst);
            return;
        }
        g.extend_from_slice(&chunk[..n]);
    }
}

/// Feeds `bytes` to the child's standard input (on its own thread, so a large input cannot stall the wait), reads both outputs
/// while it runs, and waits for the child for at most `limit`. A child still running then (or one that writes more than
/// [`MAX_REPLY`]) has its whole tree ended and is reported as an error; the readers end because the pipes close, and are joined.
/// The child must come from [`spawn_piped`].
fn pipe_in(mut child: std::process::Child, bytes: &[u8], limit: Duration) -> std::io::Result<std::process::Output> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let deadline = std::time::Instant::now() + limit;
    // Taken before the child can be reaped: the tree is ended by identity (ADR 0033).
    let root = paddock_proc::ProcessId::of(child.id());
    let (out, err, over) = (Mutex::new(Vec::new()), Mutex::new(Vec::new()), AtomicBool::new(false));
    let tail = |b: &Mutex<Vec<u8>>| {
        let g = b.lock().unwrap_or_else(|e| e.into_inner());
        let text = String::from_utf8_lossy(&g).trim().to_string();
        let keep: Vec<&str> = text.lines().rev().take(5).collect();
        keep.into_iter().rev().collect::<Vec<_>>().join(" | ")
    };
    std::thread::scope(|s| {
        s.spawn(move || {
            if let Some(mut pipe) = stdin {
                // The child may end before reading everything; its exit status says what went wrong.
                let _ = pipe.write_all(bytes);
            }
        });
        if let Some(p) = stdout {
            s.spawn(|| drain(p, &out, &over));
        }
        if let Some(p) = stderr {
            s.spawn(|| drain(p, &err, &over));
        }
        let end_tree = |child: &mut std::process::Child| {
            if let Some(id) = root {
                paddock_proc::kill_tree(id);
            }
            let _ = child.kill();
            let _ = child.wait();
        };
        loop {
            if over.load(Ordering::SeqCst) {
                end_tree(&mut child);
                return Err(std::io::Error::other(format!("it wrote more than {} MiB", MAX_REPLY >> 20)));
            }
            if let Some(status) = child.try_wait()? {
                // The child is gone; whatever it started that still holds a pipe is ended too, or the readers never finish.
                if let Some(id) = root {
                    paddock_proc::kill_tree(id);
                }
                return Ok(status);
            }
            if std::time::Instant::now() >= deadline {
                end_tree(&mut child);
                let said = tail(&err);
                let said = if said.is_empty() {
                    String::new()
                } else {
                    format!("; its stderr so far: {said}")
                };
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("it did not finish within {} seconds{said}", limit.as_secs()),
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })
    .map(|status| std::process::Output {
        status,
        stdout: std::mem::take(&mut *out.lock().unwrap_or_else(|e| e.into_inner())),
        stderr: std::mem::take(&mut *err.lock().unwrap_or_else(|e| e.into_inner())),
    })
}

/// The real thing, through `paddock-proc` (stdin only for `git hash-object`, which reads it and ends).
pub struct Real;

impl Exec for Real {
    fn run(&self, program: &str, args: &[&str], cwd: &Path, stdin: Option<&[u8]>) -> Result<String, CliError> {
        let fail = |why: String| {
            CliError::failure(format!("{program} {}: {why}", args.first().unwrap_or(&"")))
                .with_hint("Check that git and rustc work in this repository.")
        };
        let mut cmd = proc_command(program);
        cmd.args(args).current_dir(cwd);
        let out = match stdin {
            None => {
                let out = run_bounded(cmd, Limits::new(TIMEOUT)).map_err(|e| fail(e.to_string()))?;
                if !out.success() {
                    return Err(fail(out.stderr_text().trim().to_string()));
                }
                out.stdout_text()
            }
            Some(bytes) => {
                let out = spawn_piped(&mut cmd)
                    .and_then(|child| pipe_in(child, bytes, TIMEOUT))
                    .map_err(|e| fail(e.to_string()))?;
                if !out.status.success() {
                    return Err(fail(String::from_utf8_lossy(&out.stderr).trim().to_string()));
                }
                String::from_utf8_lossy(&out.stdout).into_owned()
            }
        };
        Ok(out)
    }
}

/// Computes the hash for the repository at `root`.
pub fn compute(
    exec: &dyn Exec,
    env: &dyn Fn(&str) -> Option<String>,
    vars: &[(String, String)],
    root: &Path,
) -> Result<InputHash, CliError> {
    compute_for(exec, env, vars, root, "tools/paddock-dev")
}

/// [`compute`] for another tool of the repository (`tools/heavy`): the same hash over the `inputs.txt` in its folder `tool_dir`
/// (relative to `root`), so a tool built on demand is cached under its own content address. The computation is
/// `dev_launch::hash::compute_for`, the one copy the `./dev` launcher also runs.
pub fn compute_for(
    exec: &dyn Exec,
    env: &dyn Fn(&str) -> Option<String>,
    vars: &[(String, String)],
    root: &Path,
    tool_dir: &str,
) -> Result<InputHash, CliError> {
    let run = |program: &str, args: &[&str], cwd: &Path, stdin: Option<&[u8]>| {
        exec.run(program, args, cwd, stdin)
            .map_err(|e| HashError::new(e.message).with_hint(e.hint))
    };
    dev_launch::hash::compute_for(&run, env, vars, root, tool_dir).map_err(to_cli)
}

/// `dev self hash`.
pub fn command() -> Result<Report, CliError> {
    // An unreadable current folder is an empty path: git then runs in the folder it inherited.
    let cwd = std::env::current_dir().unwrap_or_default();
    let env = |k: &str| std::env::var(k).ok();
    let root = Real
        .run("git", &["rev-parse", "--show-toplevel"], &cwd, None)
        .map_err(|e| e.with_hint("Run ./dev inside the Paddock repository."))?;
    let vars: Vec<(String, String)> = std::env::vars().collect();
    let h = compute(&Real, &env, &vars, Path::new(root.trim()))?;
    let wrapper = env("PADDOCK_DEV_WRAPPER_HASH");
    let text = format!(
        "hash     {}\nwrapper  {}\nembedded {}\ndirty    {}",
        h.hash,
        wrapper.as_deref().unwrap_or("-"),
        EMBEDDED.unwrap_or("-"),
        h.dirty
    );
    Ok(Report::new(
        json!({"hash": h.hash, "wrapper": wrapper, "embedded": EMBEDDED, "dirty": h.dirty, "compiler": h.compiler.lines().collect::<Vec<_>>(), "inputs": h.inputs}),
        text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// The child echoes more than a pipe holds: its output must be read while it runs, or it blocks until the limit.
    #[cfg(unix)]
    #[test]
    fn pipe_in_reads_a_large_reply_while_the_child_runs() {
        let bytes = vec![b'x'; 1 << 20];
        let mut cmd = proc_command("cat");
        let out = pipe_in(spawn_piped(&mut cmd).unwrap(), &bytes, Duration::from_secs(20)).expect("finishes");
        assert_eq!(out.stdout.len(), bytes.len());
    }

    /// Scripted programs: answers by program and first argument, and records the stdin of `hash-object`.
    struct Fake {
        rustc: String,
        tree: String,
        status: String,
        stdin_seen: RefCell<Vec<String>>,
    }

    impl Exec for Fake {
        fn run(&self, program: &str, args: &[&str], _cwd: &Path, stdin: Option<&[u8]>) -> Result<String, CliError> {
            match (program, args.first().copied()) {
                ("git", Some("ls-tree")) => Ok(self.tree.clone()),
                ("git", Some("status")) => Ok(self.status.clone()),
                ("git", Some("hash-object")) => {
                    let text = String::from_utf8_lossy(stdin.unwrap_or_default()).into_owned();
                    self.stdin_seen.borrow_mut().push(text.clone());
                    Ok(format!("hash-of-{}\n", text.len()))
                }
                (p, Some("-vV")) if p == self.rustc => Ok("commit-hash: c1\nhost: h\nrelease: 1.0\n".to_string()),
                _ => Err(CliError::failure("unexpected program")),
            }
        }
    }

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("tools/paddock-dev")).unwrap();
        std::fs::write(d.path().join("tools/paddock-dev/inputs.txt"), "# x\ntools/paddock-dev\ncrates/a\n").unwrap();
        d
    }

    #[test]
    fn compute_hashes_the_tree_and_the_compiler_lines_and_reports_dirt() {
        let d = repo();
        let fake = Fake {
            rustc: "rustc".into(),
            tree: "T\n".into(),
            status: " M tools/paddock-dev/src/x.rs\n".into(),
            stdin_seen: RefCell::default(),
        };
        let h = compute(&fake, &|_: &str| None, &[], d.path()).unwrap();
        assert_eq!(fake.stdin_seen.borrow()[0], "T\ncommit-hash: c1\nrelease: 1.0\n\n");
        assert!(h.dirty && h.inputs == ["tools/paddock-dev", "crates/a"] && h.hash.starts_with("hash-of-"));
        let clean = Fake {
            status: "\n".into(),
            ..fake
        };
        assert!(!compute(&clean, &|_: &str| None, &[], d.path()).unwrap().dirty);
    }

    #[test]
    fn compute_asks_the_compiler_named_by_the_environment() {
        let d = repo();
        let fake = Fake {
            rustc: "my-rustc".into(),
            tree: String::new(),
            status: String::new(),
            stdin_seen: RefCell::default(),
        };
        let env = |k: &str| (k == "RUSTC").then(|| "my-rustc".to_string());
        assert!(compute(&fake, &env, &[], d.path()).is_ok());
        assert!(compute(&fake, &|_: &str| None, &[], d.path()).is_err());
    }

    #[test]
    fn the_real_runner_reports_missing_programs_and_failures() {
        // A folder of its own: the checkout may be a worktree whose .git file names a path this machine (a container) lacks.
        let dir = tempfile::tempdir().unwrap();
        let here = dir.path();
        assert!(
            Real.run("paddock-no-such-program", &["x"], here, None)
                .unwrap_err()
                .text()
                .contains("not available")
        );
        assert!(Real.run("paddock-no-such-program", &["x"], here, Some(b"in")).is_err());
        assert!(Real.run("git", &["definitely-not-a-git-command"], here, None).is_err());
        assert!(Real.run("git", &["hash-object", "--no-such-flag"], here, Some(b"in")).is_err());
        let blob = Real.run("git", &["hash-object", "--stdin"], here, Some(b"hello\n")).unwrap();
        assert_eq!(
            blob.trim(),
            "ce013625030ba8dba906f756967f9e9ca394464a",
            "git's blob id of \"hello\\n\""
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_reads_its_input_but_never_ends_is_killed_at_the_limit_with_its_tree() {
        let dir = tempfile::tempdir().unwrap();
        let report = dir.path().join("grandchild.identity");
        let script = format!(
            "echo stuck-here >&2; {} & cat >/dev/null; wait",
            crate::test_sleeper::shell_command()
        );
        let mut cmd = proc_command("sh");
        cmd.args(["-c", &script]).env(crate::test_sleeper::FILE_VAR, &report);
        let child = spawn_piped(&mut cmd).unwrap();
        let started = std::time::Instant::now();
        let err = pipe_in(child, b"x", Duration::from_millis(500)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(err.to_string().contains("stuck-here"), "the stderr tail is in the error: {err}");
        // The grandchild holds the pipes open for its whole life: the limit must still end the call, and end the grandchild too.
        assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());
        // It reported its own identity (pid and start time) when it ran: once it is gone a bare pid reads the same as a failed
        // look, and the wait below would pass on nothing (rule 53).
        let grandchild = crate::test_sleeper::reported(&report).expect("the grandchild reported its identity before the limit");
        let gone = paddock_testkit::poll_until_within(Duration::from_secs(10), || grandchild.has_ended());
        assert!(gone, "the grandchild (pid {}) is still running after the limit", grandchild.pid());
    }

    #[test]
    fn every_step_that_fails_stops_the_hash_with_that_error() {
        let d = repo();
        let ok = Fake {
            rustc: "rustc".into(),
            tree: "T\n".into(),
            status: String::new(),
            stdin_seen: RefCell::default(),
        };
        assert!(compute(&ok, &|_: &str| None, &[], d.path()).is_ok());
        // Each program fails in turn.
        struct FailOn(&'static str);
        impl Exec for FailOn {
            fn run(&self, program: &str, args: &[&str], _: &Path, _: Option<&[u8]>) -> Result<String, CliError> {
                let step = if args.first() == Some(&"-vV") {
                    "rustc"
                } else {
                    args.first().copied().unwrap_or(program)
                };
                if step == self.0 {
                    Err(CliError::failure(format!("{step} failed")))
                } else {
                    Ok("x\n".to_string())
                }
            }
        }
        for step in ["rustc", "ls-tree", "hash-object", "status"] {
            let err = compute(&FailOn(step), &|_: &str| None, &[], d.path()).unwrap_err();
            assert_eq!(err.message, format!("{step} failed"));
        }
        std::fs::write(d.path().join("tools/paddock-dev/inputs.txt"), "has space\n").unwrap();
        assert!(
            compute(&ok, &|_: &str| None, &[], d.path())
                .unwrap_err()
                .text()
                .contains("not a plain repository-relative path")
        );
    }

    #[test]
    fn a_missing_input_list_names_the_next_step() {
        let d = tempfile::tempdir().unwrap();
        let fake = Fake {
            rustc: "rustc".into(),
            tree: String::new(),
            status: String::new(),
            stdin_seen: RefCell::default(),
        };
        let err = compute(&fake, &|_: &str| None, &[], d.path()).unwrap_err();
        assert!(err.text().contains("inputs.txt") && err.text().contains("Paddock repository"));
    }
}
