//! The login-shell `PATH` resolver (G11, ADR 0010 item 9) and a [`ToolRunner`] that uses it.
//!
//! A service started by launchd or systemd gets a minimal `PATH`, so a user's `restic` or `borg` (Homebrew, a version
//! manager, `~/.local/bin`) is not found. Dyno asks the user's login shell for its `PATH` **once**: the shell is started
//! with `-l -c` (non-interactive, so a prompt framework cannot hang it), bounded by a timeout and an output cap, and the
//! answer is cached. A per-tool override (validated by [`dyno_core::toolpath::validate_dirs`]) is searched first. Programs
//! then run niced, with that `PATH`, under the usual timeout.
//!
//! Limits: a `PATH` that the user only builds in an interactive rc file (`.zshrc`) is not seen by a non-interactive login
//! shell; the well-known folders below and the per-tool override cover that.

use crate::runner::{Tool, ToolOutput, ToolRunner};
use dyno_core::ports::PlatformError;
use dyno_core::toolpath::{compose, join, parse_env, parse_marked, ENV_ALLOWLIST, ENV_MARKER, MARKER};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long the login shell may take (nvm and friends can be slow, but not this slow).
pub const SHELL_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a login-shell answer is trusted.
pub const CACHE_TTL: Duration = Duration::from_secs(10 * 60);

/// Folders a service's minimal `PATH` usually misses.
#[cfg(not(windows))]
fn fallback_dirs() -> Vec<String> {
    let mut v: Vec<String> = [
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/snap/bin",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for sub in [".local/bin", ".cargo/bin", "bin", "go/bin"] {
            v.push(home.join(sub).to_string_lossy().into_owned());
        }
    }
    v
}

#[cfg(windows)]
fn fallback_dirs() -> Vec<String> {
    Vec::new()
}

/// What the login shell told.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShellAnswer {
    /// Its `PATH`.
    pub path: Option<String>,
    /// The allow-listed variables it exports (`RESTIC_REPOSITORY`...), never passwords.
    pub env: Vec<(String, String)>,
}

/// Something that can tell the user's login-shell `PATH`.
pub trait ShellPath: Send + Sync {
    /// The shell's answer, or `None` when it cannot be read (no shell, a timeout, an unreadable answer).
    fn ask(&self) -> Option<ShellAnswer>;
}

/// Asks the user's login shell. On Windows there is no login shell and nothing is asked.
#[derive(Debug, Clone)]
pub struct LoginShell {
    shell: Option<PathBuf>,
}

impl LoginShell {
    /// The shell from `$SHELL`, when it is an absolute path to an existing file.
    pub fn from_env() -> LoginShell {
        let shell = std::env::var_os("SHELL")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute() && p.is_file());
        LoginShell { shell }
    }

    /// A specific shell (tests).
    pub fn with(shell: impl Into<PathBuf>) -> LoginShell {
        LoginShell { shell: Some(shell.into()) }
    }
}

/// The arguments that make `shell` print the marker and its `PATH` (and the allow-listed variables): fish keeps `PATH`
/// as a list and has another syntax, so it only gets the `PATH`.
fn shell_args(shell: &Path) -> Vec<String> {
    let name = shell.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let script = if name == "fish" {
        format!("echo {MARKER}(string join : $PATH)")
    } else {
        let vars = ENV_ALLOWLIST.join(" ");
        format!(
            "printf '%s%s\\n' '{MARKER}' \"$PATH\"; for v in {vars}; do eval \"x=\\${{$v-}}\"; [ -n \"$x\" ] && printf '%s%s=%s\\n' '{ENV_MARKER}' \"$v\" \"$x\"; done; true"
        )
    };
    vec!["-l".into(), "-c".into(), script]
}

impl ShellPath for LoginShell {
    fn ask(&self) -> Option<ShellAnswer> {
        if cfg!(windows) {
            return None;
        }
        let shell = self.shell.as_ref()?;
        let mut cmd = crate::process::command(shell);
        cmd.args(shell_args(shell));
        let out = crate::process::run_with_timeout(cmd, SHELL_TIMEOUT).ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let path = parse_marked(&text)?;
        Some(ShellAnswer {
            path: Some(path),
            env: parse_env(&text),
        })
    }
}

/// Resolves programs against the override folders, the login-shell `PATH` and the process's own.
pub struct PathResolver {
    shell: Box<dyn ShellPath>,
    ttl: Duration,
    cache: Mutex<Option<(Instant, Option<ShellAnswer>)>>,
}

impl PathResolver {
    /// A resolver over `shell`, trusting its answer for `ttl`.
    pub fn new(shell: Box<dyn ShellPath>, ttl: Duration) -> PathResolver {
        PathResolver {
            shell,
            ttl,
            cache: Mutex::new(None),
        }
    }

    /// The real one: the user's login shell, cached for [`CACHE_TTL`].
    pub fn system() -> PathResolver {
        PathResolver::new(Box::new(LoginShell::from_env()), CACHE_TTL)
    }

    /// The login-shell `PATH`, from the cache when fresh. A failed ask is cached as well (for the same time), so a broken
    /// shell is not started again on every call.
    pub fn login_path(&self) -> Option<String> {
        self.answer().and_then(|a| a.path)
    }

    /// The variables the login shell exports among the allow-listed ones (empty when it could not be asked).
    pub fn login_env(&self) -> Vec<(String, String)> {
        self.answer().map(|a| a.env).unwrap_or_default()
    }

    fn answer(&self) -> Option<ShellAnswer> {
        let mut c = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((at, a)) = c.as_ref() {
            if at.elapsed() < self.ttl {
                return a.clone();
            }
        }
        let a = self.shell.ask();
        *c = Some((Instant::now(), a.clone()));
        a
    }

    /// Whether the last answer came from the login shell (false: not asked yet, or it failed).
    pub fn login_path_used(&self) -> bool {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|(_, a)| a.is_some())
    }

    /// The folders searched, in order.
    pub fn search_dirs(&self, overrides: &[String]) -> Vec<String> {
        let sep = if cfg!(windows) { ';' } else { ':' };
        let process = std::env::var("PATH").unwrap_or_default();
        let fallbacks = fallback_dirs();
        let fb: Vec<&str> = fallbacks.iter().map(String::as_str).collect();
        compose(overrides, self.login_path().as_deref(), &process, sep, &fb)
    }

    /// The full path of `program` (a bare name) in the search folders.
    pub fn find(&self, program: &str, overrides: &[String]) -> Option<PathBuf> {
        if program.is_empty() || program.contains(['/', '\\', '\0']) {
            return None;
        }
        self.search_dirs(overrides)
            .iter()
            .find_map(|d| executable_in(Path::new(d), program))
    }
}

#[cfg(not(windows))]
fn executable_in(dir: &Path, program: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(program);
    let meta = std::fs::metadata(&p).ok()?;
    (meta.is_file() && meta.permissions().mode() & 0o111 != 0).then_some(p)
}

#[cfg(windows)]
fn executable_in(dir: &Path, program: &str) -> Option<PathBuf> {
    ["", ".exe", ".cmd", ".bat"]
        .iter()
        .map(|e| dir.join(format!("{program}{e}")))
        .find(|p| p.is_file())
}

/// Runs programs found through a [`PathResolver`], niced, with the resolved `PATH` in their environment.
pub struct PathRunner<'a> {
    resolver: &'a PathResolver,
    overrides: Vec<String>,
}

impl<'a> PathRunner<'a> {
    /// A runner that also searches `overrides` first.
    pub fn new(resolver: &'a PathResolver, overrides: &[String]) -> PathRunner<'a> {
        PathRunner {
            resolver,
            overrides: overrides.to_vec(),
        }
    }
}

impl ToolRunner for PathRunner<'_> {
    fn run_tool(&self, t: &Tool, timeout: Duration) -> Result<ToolOutput, PlatformError> {
        let (program, args) = (t.program.as_str(), &t.args);
        let found = if program.contains(['/', '\\']) {
            PathBuf::from(program)
        } else {
            self.resolver
                .find(program, &self.overrides)
                .ok_or_else(|| PlatformError::Unsupported(format!("{program} was not found in your PATH or the folders Dyno looks in.")))?
        };
        let path = join(&self.resolver.search_dirs(&self.overrides), if cfg!(windows) { ';' } else { ':' });
        let mut cmd = niced(&found);
        cmd.args(args).env("PATH", path);
        // The repository settings the user exports in their shell, unless this process already has them.
        for (name, value) in self.resolver.login_env() {
            if std::env::var_os(&name).is_none() {
                cmd.env(name, value);
            }
        }
        let out = crate::process::run_with_timeout(cmd, timeout)?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        Ok(ToolOutput {
            ok: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            error: stderr
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim()
                .chars()
                .take(200)
                .collect(),
        })
    }
}

/// A command for `exe`, run at low priority where `nice` exists (backup checks must never compete with the user's work).
#[cfg(unix)]
fn niced(exe: &Path) -> std::process::Command {
    let nice = Path::new("/usr/bin/nice");
    if nice.is_file() {
        let mut c = crate::process::command(nice);
        c.args(["-n", "10"]).arg(exe);
        c
    } else {
        crate::process::command(exe)
    }
}

#[cfg(not(unix))]
fn niced(exe: &Path) -> std::process::Command {
    crate::process::command(exe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Counting(Arc<AtomicUsize>, Option<String>);

    impl ShellPath for Counting {
        fn ask(&self) -> Option<ShellAnswer> {
            self.0.fetch_add(1, Ordering::SeqCst);
            self.1.clone().map(|p| ShellAnswer {
                path: Some(p),
                env: vec![("BORG_REPO".into(), "/mnt/b".into())],
            })
        }
    }

    #[test]
    fn the_shell_is_asked_once_per_ttl_even_when_it_fails() {
        let n = Arc::new(AtomicUsize::new(0));
        let r = PathResolver::new(Box::new(Counting(n.clone(), Some("/opt/x/bin".into()))), Duration::from_secs(60));
        assert!(!r.login_path_used());
        assert_eq!(r.login_path().as_deref(), Some("/opt/x/bin"));
        assert_eq!(r.login_path().as_deref(), Some("/opt/x/bin"));
        assert_eq!(n.load(Ordering::SeqCst), 1);
        assert!(r.login_path_used());
        assert!(r
            .search_dirs(&["/mine".into()])
            .starts_with(&["/mine".to_string(), "/opt/x/bin".to_string()]));

        let n = Arc::new(AtomicUsize::new(0));
        let broken = PathResolver::new(Box::new(Counting(n.clone(), None)), Duration::from_secs(60));
        assert_eq!(broken.login_path(), None);
        assert_eq!(broken.login_path(), None);
        assert_eq!(n.load(Ordering::SeqCst), 1, "a failure is cached too");
        assert!(!broken.login_path_used());

        let expired = PathResolver::new(Box::new(Counting(n.clone(), None)), Duration::ZERO);
        expired.login_path();
        expired.login_path();
        assert_eq!(n.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn program_names_cannot_escape_the_search_folders() {
        let r = PathResolver::new(Box::new(Counting(Arc::default(), None)), Duration::from_secs(1));
        assert_eq!(r.find("../sh", &[]), None);
        assert_eq!(r.find("a/b", &[]), None);
        assert_eq!(r.find("", &[]), None);
        assert_eq!(r.find("dyno-no-such-tool", &[]), None);
    }

    #[test]
    fn arguments_differ_for_fish() {
        assert!(shell_args(Path::new("/usr/bin/fish"))[2].contains("string join"));
        assert!(shell_args(Path::new("/bin/zsh"))[2].contains("printf"));
        assert_eq!(shell_args(Path::new("/bin/zsh"))[..2], ["-l", "-c"]);
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
            let p = dir.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        }

        #[test]
        fn a_login_shell_answer_survives_noise_around_it() {
            let t = tempfile::tempdir().unwrap();
            let sh = script(
                t.path(),
                "fakeshell",
                "echo 'rc file chatter'\nprintf '%s%s\\n' __DYNO_PATH__ \"/opt/fake/bin:/usr/bin\"\necho trailing",
            );
            let a = LoginShell::with(sh).ask().unwrap();
            assert_eq!(a.path.as_deref(), Some("/opt/fake/bin:/usr/bin"));
            assert!(a.env.is_empty());
        }

        #[test]
        fn the_real_shell_arguments_export_only_listed_variables() {
            // Run the exact script a real shell gets, with /bin/sh standing in for the user's shell.
            let args = shell_args(Path::new("/bin/sh"));
            let mut cmd = crate::process::command("/bin/sh");
            cmd.args(&args)
                .env("BORG_REPO", "/mnt/borg")
                .env("RESTIC_PASSWORD", "hunter2")
                .env("BORG_PASSCOMMAND", "");
            let out = crate::process::output_text(cmd, Duration::from_secs(20)).unwrap();
            assert!(out.contains("__DYNO_PATH__"), "{out}");
            let env = parse_env(&out);
            assert_eq!(env, [("BORG_REPO".to_string(), "/mnt/borg".to_string())]);
            assert!(!out.contains("hunter2"));
        }

        #[test]
        fn a_shell_that_fails_or_hangs_gives_nothing_and_is_stopped() {
            let t = tempfile::tempdir().unwrap();
            assert_eq!(LoginShell::with(script(t.path(), "bad", "exit 3")).ask(), None);
            assert_eq!(LoginShell::with(script(t.path(), "mute", "echo hi")).ask(), None);
            assert_eq!(LoginShell::with(t.path().join("missing")).ask(), None);
        }

        #[test]
        fn an_override_folder_is_searched_first_and_programs_run_niced_with_the_path() {
            let t = tempfile::tempdir().unwrap();
            script(t.path(), "faketool", "echo \"ran $1 with $PATH repo=$BORG_REPO\"");
            let r = PathResolver::new(
                Box::new(Counting(Arc::default(), Some("/opt/login/bin".into()))),
                Duration::from_secs(60),
            );
            let dirs = vec![t.path().to_string_lossy().into_owned()];
            assert_eq!(r.find("faketool", &dirs), Some(t.path().join("faketool")));
            assert_eq!(r.find("faketool", &[]), None, "not found without the override");
            let out = PathRunner::new(&r, &dirs)
                .run("faketool", &["x".into()], Duration::from_secs(20))
                .unwrap();
            assert!(out.starts_with("ran x with "), "{out}");
            assert!(out.contains("/opt/login/bin"), "{out}");
            assert!(out.contains("repo=/mnt/b"), "{out}");
            let err = PathRunner::new(&r, &[]).run("faketool", &[], Duration::from_secs(5)).unwrap_err();
            assert!(matches!(err, PlatformError::Unsupported(m) if m.contains("not found")));
        }

        #[test]
        fn a_tool_that_hangs_is_killed_at_the_timeout() {
            let t = tempfile::tempdir().unwrap();
            script(t.path(), "slowtool", "sleep 30");
            let r = PathResolver::new(Box::new(Counting(Arc::default(), None)), Duration::from_secs(60));
            let dirs = vec![t.path().to_string_lossy().into_owned()];
            let started = Instant::now();
            let err = PathRunner::new(&r, &dirs)
                .run("slowtool", &[], Duration::from_millis(400))
                .unwrap_err();
            assert!(matches!(err, PlatformError::Timeout(_)), "{err:?}");
            assert!(started.elapsed() < Duration::from_secs(15));
        }

        #[test]
        fn a_file_without_the_execute_bit_is_not_a_tool() {
            let t = tempfile::tempdir().unwrap();
            std::fs::write(t.path().join("notexec"), "x").unwrap();
            let r = PathResolver::new(Box::new(Counting(Arc::default(), None)), Duration::from_secs(60));
            assert_eq!(r.find("notexec", &[t.path().to_string_lossy().into_owned()]), None);
        }
    }
}
