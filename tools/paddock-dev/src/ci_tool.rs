//! The wrapper contract for third-party programs that a CI step runs (cargo, npm, curl, tar, sccache, apt-get, ...): the
//! programs we cannot rewrite get a thin wrapper instead (docs/plans/ci-in-rust.md, "The wrapper contract"; ER-27).
//!
//! A [`ToolCall`] is the exact command line, built by the caller from typed values (never a shell string), with:
//!
//! - the environment set for this program only ([`ToolCall::env`]): a secret reaches the one program that needs it, never the
//!   whole step;
//! - the values to mask ([`ToolCall::secret`]): every line this module prints (the command, the stderr tail) has them replaced
//!   by `***`;
//! - a timeout: no program runs unbounded; at the limit its whole tree is ended (`paddock_proc`, ADR 0033);
//! - one of three ways to treat its output ([`Output`]): captured (short programs; the tail is printed when it fails), streamed
//!   to the job log as it comes (long programs such as apt-get: the log holds every line), or written to a file (a program that
//!   leaves a daemon behind, such as `sccache --start-server`, whose server would hold a pipe open for its whole life).
//!
//! [`ToolCall::run`] logs one `run:` line before (the masked command line and the names, never the values, of the variables it
//! sets) and one `ran:` line after (how it ended: the exit code, with the NTSTATUS in hex on Windows, the signal on Unix, or the
//! timeout; and the duration). The caller decides what a failure means and prints exactly one `::error::` line for it with
//! [`error_line`]: the cause, then `Next step:`.

use paddock_proc::{KEPT_ENV, Limits, Outcome, RunError, Sink, command, run_bounded, run_waited};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

/// Variables every program keeps besides `paddock_proc::KEPT_ENV`: what Windows programs need to start and find their own
/// files (PowerShell its modules). None of them holds a secret.
const WINDOWS_BASE: &[&str] = &[
    "SystemDrive",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "CommonProgramFiles",
    "PATHEXT",
    "COMSPEC",
    "APPDATA",
    "LOCALAPPDATA",
    "PSModulePath",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
];

/// The proxy and certificate variables a program that reaches the network may need.
pub const NETWORK_ENV: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "all_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
];

/// How many lines of a failed program's error output are printed.
pub const TAIL_LINES: usize = 40;

/// What happens to the program's standard output and standard error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// Kept in memory (bounded); [`Ran::stdout`] and [`Ran::stderr`] hold them.
    Capture,
    /// Inherited: the job log shows them as they come.
    Stream,
    /// Both written to this file, which the caller reads afterwards ([`Ran::stderr`] holds its text).
    File(PathBuf),
}

/// One run of a third-party program.
#[derive(Debug, Clone)]
pub struct ToolCall {
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    pass: Vec<String>,
    secrets: Vec<String>,
    timeout: Duration,
    output: Output,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// It exited with status 0.
    pub success: bool,
    /// How it ended, for a person: `exit 0`, `exit 3221225477 (0xC0000005)`, `signal 9`, `timed out after 60 s`.
    pub how: String,
    /// Standard output (captured runs only).
    pub stdout: String,
    /// Standard error (captured runs), or the file's text (file runs).
    pub stderr: String,
}

impl ToolCall {
    /// A call of `program` with the default limits: captured output, a two-minute timeout.
    pub fn new(program: impl Into<String>) -> ToolCall {
        ToolCall {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            pass: Vec::new(),
            secrets: Vec::new(),
            timeout: Duration::from_secs(120),
            output: Output::Capture,
        }
    }

    /// Adds arguments.
    pub fn args<I: IntoIterator<Item = S>, S: Into<String>>(mut self, args: I) -> ToolCall {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Sets a variable for this program only.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> ToolCall {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Variables of the caller's environment this program also gets, besides the base list: exact names, or a prefix ending
    /// in `*` (`SCCACHE_*`). Everything else (tokens above all) never reaches it.
    pub fn pass<I: IntoIterator<Item = S>, S: Into<String>>(mut self, names: I) -> ToolCall {
        self.pass.extend(names.into_iter().map(Into::into));
        self
    }

    /// The child's environment: the base list and the passed names from `parent`, then the call's own variables.
    pub fn child_env(&self, parent: &HashMap<String, String>) -> Vec<(String, String)> {
        let keep = |k: &str| {
            KEPT_ENV.contains(&k)
                || WINDOWS_BASE.iter().any(|b| b.eq_ignore_ascii_case(k))
                || self.pass.iter().any(|p| match p.strip_suffix('*') {
                    Some(prefix) => k.starts_with(prefix),
                    None => p == k,
                })
        };
        let mut env: Vec<(String, String)> = parent
            .iter()
            .filter(|(k, _)| keep(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        env.sort();
        env.extend(self.env.iter().cloned());
        env
    }

    /// A value that is never printed (empty values are ignored).
    pub fn secret(mut self, value: impl Into<String>) -> ToolCall {
        let v = value.into();
        if !v.is_empty() {
            self.secrets.push(v);
        }
        self
    }

    /// The timeout.
    pub fn timeout(mut self, timeout: Duration) -> ToolCall {
        self.timeout = timeout;
        self
    }

    /// The treatment of its output.
    pub fn output(mut self, output: Output) -> ToolCall {
        self.output = output;
        self
    }

    /// The command line as it is logged: each argument quoted when it needs it, the secrets masked.
    pub fn display(&self) -> String {
        let mut line = quote(&self.program);
        for a in &self.args {
            line.push(' ');
            line.push_str(&quote(a));
        }
        mask(&line, &self.secrets)
    }

    /// Masks this call's secrets in `text`.
    pub fn masked(&self, text: &str) -> String {
        mask(text, &self.secrets)
    }

    /// Runs it with an environment built from `parent` ([`ToolCall::child_env`]), logging the `run:` and `ran:` lines to `log`.
    /// `Err` only when it could not be started (the message names the program and what to do); every way it ends after a start
    /// is an `Ok`.
    pub fn run(&self, parent: &HashMap<String, String>, log: &mut dyn Write) -> Result<Ran, String> {
        let names: Vec<&str> = self.env.iter().map(|(k, _)| k.as_str()).collect();
        let set = if names.is_empty() {
            String::new()
        } else {
            format!(" (sets {})", names.join(" "))
        };
        let _ = writeln!(log, "run: {}{set}", self.display());
        let started = Instant::now();
        let cmd = self.build(parent);
        let ran = match &self.output {
            Output::Capture => self.run_captured(cmd),
            Output::Stream => self.run_to(cmd, Sink::Inherit, None),
            Output::File(p) => self.run_to(cmd, Sink::File(p.clone()), Some(p)),
        }
        .map_err(|e| self.masked(&e))?;
        let _ = writeln!(log, "ran: {} ({}, {:.1} s)", self.program, ran.how, started.elapsed().as_secs_f64());
        Ok(Ran {
            stdout: self.masked(&ran.stdout),
            stderr: self.masked(&ran.stderr),
            ..ran
        })
    }

    fn build(&self, parent: &HashMap<String, String>) -> std::process::Command {
        let mut cmd = command(&self.program);
        cmd.args(&self.args).env_clear();
        for (k, v) in self.child_env(parent) {
            cmd.env(k, v);
        }
        cmd
    }

    fn not_started(&self, e: RunError) -> String {
        match e {
            RunError::NotFound(_) => format!("{} is not installed on this runner (or not on PATH).", self.program),
            other => other.to_string(),
        }
    }

    fn timed_out(&self) -> String {
        format!("timed out after {} s", self.timeout.as_secs())
    }

    fn run_captured(&self, cmd: std::process::Command) -> Result<Ran, String> {
        let out = run_bounded(cmd, Limits::new(self.timeout)).map_err(|e| self.not_started(e))?;
        let (success, how) = match out.outcome {
            Outcome::Exited(s) => (s.success(), describe_status(s)),
            Outcome::TimedOut => (false, self.timed_out()),
            Outcome::OutputCap => (false, "stopped at the output cap".to_string()),
        };
        Ok(Ran {
            success,
            how,
            stdout: out.stdout_text(),
            stderr: out.stderr_text(),
        })
    }

    /// Streamed or file output, through `paddock_proc::run_waited` (bounded, the whole tree ended at the timeout).
    fn run_to(&self, cmd: std::process::Command, sink: Sink, file: Option<&PathBuf>) -> Result<Ran, String> {
        let outcome = run_waited(cmd, self.timeout, sink).map_err(|e| self.not_started(e))?;
        let text = file.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let (success, how) = match outcome {
            Outcome::Exited(s) => (s.success(), describe_status(s)),
            Outcome::TimedOut => (false, self.timed_out()),
            Outcome::OutputCap => (false, "stopped at the output cap".to_string()),
        };
        Ok(Ran {
            success,
            how,
            stdout: String::new(),
            stderr: text,
        })
    }
}

/// `exit <n>` (with the NTSTATUS in hex for a Windows crash code) or `signal <n>`.
pub fn describe_status(s: ExitStatus) -> String {
    if let Some(code) = s.code() {
        return describe_code(code);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = s.signal() {
            return format!("signal {sig}");
        }
    }
    "ended without an exit code".to_string()
}

/// An exit code for a person: Windows reports a crash as an NTSTATUS (0xC0000005 is an access violation, 0xC0000409 a fast
/// fail), which reads as a huge or negative number in decimal.
pub fn describe_code(code: i32) -> String {
    let unsigned = code as u32;
    if unsigned >= 0xC000_0000 {
        format!("exit {unsigned} (0x{unsigned:08X}, an NTSTATUS crash code)")
    } else {
        format!("exit {code}")
    }
}

/// The last `n` lines of `text`, each indented, or nothing.
pub fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(n);
    let mut out = String::new();
    for l in &lines[from..] {
        let _ = writeln!(out, "  {l}");
    }
    out
}

/// `text` with every secret replaced by `***` (longest first, so a secret that contains another is masked whole).
pub fn mask(text: &str, secrets: &[String]) -> String {
    let mut sorted: Vec<&String> = secrets.iter().filter(|s| !s.is_empty()).collect();
    sorted.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut out = text.to_string();
    for s in sorted {
        out = out.replace(s.as_str(), "***");
    }
    out
}

/// The one `::error::` line of a failure: the cause, then the next step.
pub fn error_line(cause: &str, next: &str) -> String {
    format!("::error::{cause} Next step: {next}")
}

fn quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=,+@%".contains(c) || (c == '\\' && cfg!(windows)));
    if plain {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logged_command_line_quotes_what_needs_it_and_masks_secrets() {
        let call = ToolCall::new("curl")
            .args(["-fsSL", "-o", "/tmp/a b", "https://u:pw@h/x"])
            .secret("pw")
            .secret("");
        assert_eq!(call.display(), "curl -fsSL -o '/tmp/a b' https://u:***@h/x");
        assert_eq!(mask("ab abc", &["abc".into(), "ab".into()]), "*** ***");
    }

    #[test]
    fn windows_crash_codes_are_named_in_hex() {
        assert_eq!(describe_code(1), "exit 1");
        assert_eq!(describe_code(127), "exit 127");
        assert_eq!(
            describe_code(-1_073_741_819),
            "exit 3221225477 (0xC0000005, an NTSTATUS crash code)"
        );
    }

    #[test]
    fn the_tail_is_the_last_lines_indented() {
        assert_eq!(tail("a\nb\nc\n", 2), "  b\n  c\n");
        assert_eq!(tail("", 5), "");
    }

    #[test]
    fn an_error_line_has_the_cause_and_the_next_step() {
        assert_eq!(error_line("x failed.", "do y."), "::error::x failed. Next step: do y.");
    }

    fn parent(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn a_child_gets_the_base_list_and_what_the_call_passes_never_an_unlisted_token() {
        let env = parent(&[
            ("PATH", "/bin"),
            ("HOME", "/h"),
            ("PADDOCK_DEV_TOKEN", "t1"),
            ("GITHUB_TOKEN", "t2"),
            ("SCCACHE_DIR", "/s"),
            ("CARGO_HOME", "/c"),
            ("HTTPS_PROXY", "p"),
            ("ProgramData", "C:/pd"),
        ]);
        let call = ToolCall::new("x").pass(["SCCACHE_*", "CARGO_*"]).env("SCCACHE_SERVER_PORT", "4227");
        let names: Vec<String> = call.child_env(&env).into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            names,
            ["CARGO_HOME", "HOME", "PATH", "ProgramData", "SCCACHE_DIR", "SCCACHE_SERVER_PORT"]
        );
        let curl: Vec<String> = ToolCall::new("curl")
            .pass(NETWORK_ENV.iter().copied())
            .child_env(&env)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert!(
            curl.contains(&"HTTPS_PROXY".to_string()) && !curl.iter().any(|k| k.ends_with("_TOKEN")),
            "{curl:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unlisted_token_does_not_reach_the_program() {
        let env = parent(&[("PATH", "/usr/bin:/bin"), ("PADDOCK_DEV_TOKEN", "hunter2"), ("SCCACHE_DIR", "/s")]);
        let ran = ToolCall::new("sh")
            .args(["-c", "env"])
            .pass(["SCCACHE_*"])
            .run(&env, &mut Vec::new())
            .unwrap();
        assert!(ran.success);
        assert!(ran.stdout.contains("SCCACHE_DIR=/s"), "{}", ran.stdout);
        assert!(
            !ran.stdout.contains("PADDOCK_DEV_TOKEN") && !ran.stdout.contains("hunter2"),
            "{}",
            ran.stdout
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_run_reports_how_it_ended_and_what_it_wrote() {
        let mut log = Vec::new();
        let ran = ToolCall::new("sh")
            .args(["-c", "echo out; echo err-$SECRET >&2; exit 3"])
            .env("SECRET", "hunter2")
            .secret("hunter2")
            .run(&parent(&[("PATH", "/usr/bin:/bin")]), &mut log)
            .unwrap();
        assert!(!ran.success);
        assert_eq!(ran.how, "exit 3");
        assert_eq!(ran.stdout, "out\n");
        assert_eq!(ran.stderr, "err-***\n", "a secret in the output is masked too");
        let log = String::from_utf8(log).unwrap();
        assert!(log.contains("(sets SECRET)") && !log.contains("hunter2"), "{log}");
        assert!(log.contains("ran: sh (exit 3, "), "{log}");
    }

    #[cfg(unix)]
    #[test]
    fn a_waited_run_is_bounded_ends_its_tree_and_a_file_run_keeps_its_output() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("out.txt");
        let pid_file = dir.path().join("grandchild.pid");
        let env = parent(&[("PATH", "/usr/bin:/bin")]);
        let ran = ToolCall::new("sh")
            .args([
                "-c".to_string(),
                format!("echo started; sleep 60 & echo $! > '{}'; wait", pid_file.display()),
            ])
            .timeout(Duration::from_millis(500))
            .output(Output::File(file))
            .run(&env, &mut Vec::new())
            .unwrap();
        assert!(!ran.success);
        assert!(ran.how.starts_with("timed out"), "{}", ran.how);
        assert_eq!(ran.stderr, "started\n");
        let pid: u32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
        paddock_testkit::wait_until_within("the grandchild to be gone", Duration::from_secs(10), || {
            !paddock_proc::is_alive(pid)
        });
        let missing = ToolCall::new("paddock-no-such-program")
            .output(Output::Stream)
            .run(&env, &mut Vec::new());
        assert!(missing.unwrap_err().contains("not installed"));
    }
}
