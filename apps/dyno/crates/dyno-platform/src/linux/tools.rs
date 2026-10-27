//! The external programs the Linux adapters run (`systemctl`, `loginctl`, package managers, `nvidia-smi`…), behind a
//! small trait so tests substitute scripted answers. The real implementation always bounds the run with a timeout.

use dyno_core::ports::PlatformError;
use std::path::PathBuf;
use std::time::Duration;

/// What a finished tool printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// Exit status 0.
    pub ok: bool,
    /// Standard output as text.
    pub stdout: String,
    /// Standard error as text.
    pub stderr: String,
}

impl ToolOutput {
    /// The first non-empty line of the error output, for messages.
    pub fn first_error_line(&self) -> String {
        let line = self.stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("no message");
        crate::process::truncate(line.trim(), 200)
    }
}

/// Runs external programs.
pub trait Tools: Send + Sync {
    /// Runs `program` with `args`, killing it after `timeout`. A missing program is
    /// [`PlatformError::Unsupported`]; a timeout is [`PlatformError::Timeout`]; a non-zero exit is `Ok` with
    /// `ok == false`.
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> Result<ToolOutput, PlatformError>;
    /// Whether `program` can be found.
    fn exists(&self, program: &str) -> bool;
}

/// The real programs of this machine.
pub struct SystemTools;

/// Folders searched besides `$PATH` (services and desktop sessions often run with a short `PATH`).
const EXTRA_DIRS: [&str; 5] = ["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/usr/lib/wsl/lib"];

fn find(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        return None;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(EXTRA_DIRS.iter().map(PathBuf::from))
        .map(|d| d.join(program))
        .find(|p| p.is_file())
}

impl Tools for SystemTools {
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> Result<ToolOutput, PlatformError> {
        let mut cmd = crate::process::command(find(program).unwrap_or_else(|| PathBuf::from(program)));
        cmd.args(args);
        let out = crate::process::run_with_timeout(cmd, timeout)?;
        Ok(ToolOutput {
            ok: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    fn exists(&self, program: &str) -> bool {
        find(program).is_some()
    }
}

/// Scripted tools for tests: answers by program name, records calls.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Answers `program` with a fixed result; unknown programs do not exist.
    #[derive(Default)]
    pub struct FakeTools {
        pub answers: Mutex<HashMap<String, Result<ToolOutput, PlatformError>>>,
        pub calls: Mutex<Vec<String>>,
    }

    impl FakeTools {
        pub fn with(program: &str, stdout: &str) -> Self {
            let f = FakeTools::default();
            f.answer(
                program,
                Ok(ToolOutput {
                    ok: true,
                    stdout: stdout.into(),
                    stderr: String::new(),
                }),
            );
            f
        }

        pub fn answer(&self, program: &str, r: Result<ToolOutput, PlatformError>) {
            self.answers.lock().unwrap().insert(program.into(), r);
        }

        pub fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Tools for FakeTools {
        fn run(&self, program: &str, args: &[&str], _timeout: Duration) -> Result<ToolOutput, PlatformError> {
            self.calls.lock().unwrap().push(format!("{program} {}", args.join(" ")));
            self.answers
                .lock()
                .unwrap()
                .get(program)
                .cloned()
                .unwrap_or_else(|| Err(PlatformError::Unsupported(format!("{program} is not available on this computer."))))
        }

        fn exists(&self, program: &str) -> bool {
            self.answers.lock().unwrap().contains_key(program)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_real_programs_with_a_timeout() {
        let t = SystemTools;
        assert!(t.exists("sh"));
        assert!(!t.exists("dyno-no-such-tool"));
        assert!(!t.exists("/bin/sh"));
        let out = t
            .run("sh", &["-c", "echo hi; echo oops >&2; exit 2"], Duration::from_secs(20))
            .unwrap();
        assert!(!out.ok && out.stdout.trim() == "hi" && out.first_error_line() == "oops");
        assert!(matches!(
            t.run("dyno-no-such-tool", &[], Duration::from_secs(1)),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(matches!(
            t.run("sleep", &["30"], Duration::from_millis(200)),
            Err(PlatformError::Timeout(_))
        ));
        assert_eq!(
            ToolOutput {
                ok: false,
                stdout: String::new(),
                stderr: "\n".into()
            }
            .first_error_line(),
            "no message"
        );
    }
}
