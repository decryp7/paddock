//! "Run as administrator" for one session: starts Dyno's own agent with administrator rights after the OS shows its own
//! prompt, and returns once it runs. The agent keeps running until Dyno quits; nothing here registers it to start at login
//! or writes anywhere. Declining the prompt is a normal answer ([`ElevationError::Declined`]).
//!
//! - Windows: `ShellExecuteExW` with the verb `runas` (UAC), not waited for.
//! - macOS: `osascript` runs `do shell script … with administrator privileges`, the program in the background.
//! - Linux: `pkexec` (polkit).
//!
//! The quoting and the exit-code meanings are pure functions with tests; the OS calls are exercised by the manual
//! checklist (nothing in the test suite may prompt).

#[cfg_attr(windows, allow(unused_imports))]
use crate::elevate::ELEVATION_TIMEOUT;
use crate::elevate::{shell_quote, ElevationError};
use std::path::Path;

/// Starts a program with administrator rights and keeps it running.
pub trait SessionLauncher: Send + Sync {
    /// Starts `program` (an absolute path) with `args` elevated after the OS prompt (`prompt` is the reason it may show).
    /// Returns when the program is running, or `ready()` says what it was started for is up; fails with
    /// [`ElevationError::Declined`] when the prompt was cancelled.
    fn start(&self, program: &Path, args: &[String], prompt: &str, ready: &dyn Fn() -> bool) -> Result<(), ElevationError>;
}

/// The AppleScript that starts `program args…` in the background as administrator, with `prompt` in the password dialog.
pub fn applescript_background(program: &str, args: &[String], prompt: &str) -> Result<String, String> {
    let mut command = shell_quote(program)?;
    for a in args {
        command.push(' ');
        command.push_str(&shell_quote(a)?);
    }
    // Output goes nowhere and the shell does not wait: osascript returns as soon as the program is started.
    command.push_str(" >/dev/null 2>&1 &");
    let literal = |s: &str| -> Result<String, String> {
        if s.chars().any(|c| c.is_control()) {
            return Err("A control character cannot be shown in the prompt.".into());
        }
        Ok(format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
    };
    Ok(format!(
        "do shell script {} with prompt {} with administrator privileges",
        literal(&command)?,
        literal(prompt)?
    ))
}

/// What `osascript` returning `code` with `stderr` means. Error -128 is "User canceled".
pub fn osascript_session_result(code: Option<i32>, stderr: &str) -> Result<(), ElevationError> {
    if code == Some(0) {
        return Ok(());
    }
    if stderr.contains("(-128)") || stderr.to_lowercase().contains("user canceled") {
        return Err(ElevationError::Declined);
    }
    let first = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("no message").trim();
    Err(ElevationError::Failed(
        code,
        format!(
            "macOS could not start Dyno as administrator: {}",
            first.chars().take(240).collect::<String>()
        ),
    ))
}

/// What `pkexec` ending with `code` before the agent was up means: 126 is "the user dismissed the dialog", 127 "not
/// authorised" or no authentication agent.
pub fn pkexec_session_result(code: Option<i32>, stderr: &str) -> ElevationError {
    match code {
        Some(126) | Some(127) if !stderr.to_lowercase().contains("no authentication agent") => ElevationError::Declined,
        Some(127) => ElevationError::Unavailable(
            "No polkit authentication agent is running in this session, so the system cannot ask for your permission. Start your \
             desktop's authentication agent and try again."
                .into(),
        ),
        other => {
            let first = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("no message").trim();
            ElevationError::Failed(
                other,
                format!(
                    "Dyno could not start as administrator: {}",
                    first.chars().take(240).collect::<String>()
                ),
            )
        }
    }
}

/// The launcher for the OS Dyno runs on.
pub struct OsSessionLauncher;

#[cfg(target_os = "macos")]
impl SessionLauncher for OsSessionLauncher {
    fn start(&self, program: &Path, args: &[String], prompt: &str, _ready: &dyn Fn() -> bool) -> Result<(), ElevationError> {
        let script = applescript_background(&program.to_string_lossy(), args, prompt).map_err(|m| ElevationError::Failed(None, m))?;
        let mut cmd = crate::process::command("/usr/bin/osascript");
        cmd.args(["-e", &script]);
        let out = crate::process::run_with_timeout(cmd, ELEVATION_TIMEOUT).map_err(|e| ElevationError::Unavailable(e.to_string()))?;
        osascript_session_result(out.status.code(), &String::from_utf8_lossy(&out.stderr))
    }
}

#[cfg(target_os = "linux")]
impl SessionLauncher for OsSessionLauncher {
    fn start(&self, program: &Path, args: &[String], _prompt: &str, ready: &dyn Fn() -> bool) -> Result<(), ElevationError> {
        use std::io::Read;
        use std::process::Stdio;
        use std::time::Instant;
        let pkexec = ["/usr/bin/pkexec", "/bin/pkexec"]
            .into_iter()
            .find(|p| Path::new(p).is_file())
            .ok_or_else(|| {
                ElevationError::Unavailable("pkexec (polkit) is not installed, so Dyno cannot ask for your permission.".into())
            })?;
        // The one place the app starts a program as administrator: Dyno's own agent, with arguments the app built.
        #[allow(clippy::disallowed_methods)]
        let mut cmd = std::process::Command::new(pkexec);
        cmd.arg(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| ElevationError::Unavailable(format!("pkexec could not be started: {e}")))?;
        let end = Instant::now() + ELEVATION_TIMEOUT;
        loop {
            if ready() {
                return Ok(());
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut err = String::new();
                    if let Some(mut s) = child.stderr.take() {
                        let _ = s.by_ref().take(4096).read_to_string(&mut err);
                    }
                    return Err(pkexec_session_result(status.code(), &err));
                }
                Ok(None) if Instant::now() < end => std::thread::sleep(std::time::Duration::from_millis(100)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ElevationError::Failed(
                        None,
                        "The permission prompt was not answered in time.".into(),
                    ));
                }
            }
        }
    }
}

#[cfg(windows)]
impl SessionLauncher for OsSessionLauncher {
    fn start(&self, program: &Path, args: &[String], _prompt: &str, _ready: &dyn Fn() -> bool) -> Result<(), ElevationError> {
        let line = crate::elevate::windows_command_line(args).map_err(|m| ElevationError::Failed(None, m))?;
        crate::windows::elevate::start_elevated(program, &line)
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
impl SessionLauncher for OsSessionLauncher {
    fn start(&self, _: &Path, _: &[String], _: &str, _: &dyn Fn() -> bool) -> Result<(), ElevationError> {
        Err(ElevationError::Unavailable(
            "Run as administrator is not available on this system.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_applescript_starts_the_program_in_the_background_with_quoted_arguments() {
        let s = applescript_background(
            "/Applications/Dyno.app/dyno-agent",
            &["run".into(), "--elevated".into(), "/a b/it's".into()],
            "Dyno wants \"more\"",
        )
        .unwrap();
        assert!(s.starts_with("do shell script \""), "{s}");
        assert!(s.contains("with administrator privileges"));
        assert!(s.contains(">/dev/null 2>&1 &"), "the shell must not wait for the agent: {s}");
        assert!(
            s.contains("'/a b/it'\\\\''s'"),
            "the argument is quoted for the shell, then escaped for AppleScript: {s}"
        );
        assert!(s.contains("with prompt \"Dyno wants \\\"more\\\"\""), "{s}");
        assert!(applescript_background("/x", &["a\nb".into()], "p").is_err());
        assert!(applescript_background("/x", &[], "p\nq").is_err());
    }

    #[test]
    fn a_cancelled_prompt_is_declined_and_other_failures_say_why() {
        assert_eq!(osascript_session_result(Some(0), ""), Ok(()));
        assert_eq!(
            osascript_session_result(Some(1), "execution error: User canceled. (-128)"),
            Err(ElevationError::Declined)
        );
        assert!(matches!(osascript_session_result(Some(1), "\n boom \n"), Err(ElevationError::Failed(Some(1), m)) if m.contains("boom")));
        assert!(matches!(osascript_session_result(None, ""), Err(ElevationError::Failed(None, m)) if m.contains("no message")));
        assert_eq!(pkexec_session_result(Some(126), ""), ElevationError::Declined);
        assert_eq!(pkexec_session_result(Some(127), "Error: not authorized"), ElevationError::Declined);
        assert!(matches!(
            pkexec_session_result(Some(127), "No authentication agent found."),
            ElevationError::Unavailable(_)
        ));
        assert!(matches!(pkexec_session_result(Some(2), "x\n"), ElevationError::Failed(Some(2), _)));
    }
}
