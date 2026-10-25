//! Asking the operating system for administrator rights, once, for one command (ADR 0016). This is the only way Dyno
//! ever runs something elevated: the program is Dyno's own helper (`dyno-helper install …`), the arguments are values
//! the agent built (a validated account id, an absolute path), and the OS shows its own prompt. Declining is a normal
//! answer ([`ElevationError::Declined`]), not a failure of Dyno.
//!
//! - macOS: `osascript` runs `do shell script … with administrator privileges` (the system password dialog).
//! - Linux: `pkexec` (polkit).
//! - Windows: `ShellExecuteExW` with the verb `runas` (UAC), see `windows::elevate`.
//!
//! The quoting and the exit-code meanings are pure functions with tests; the three calls themselves are exercised by
//! the manual checklist (nothing in the test suite may prompt or install).

use std::path::Path;

/// Why a command did not run elevated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevationError {
    /// The user closed or cancelled the system prompt.
    Declined,
    /// This system cannot ask (no polkit agent, no `pkexec`, no `osascript`), with what to do.
    Unavailable(String),
    /// The command ran elevated and failed (its exit code, when known, and a message).
    Failed(Option<i32>, String),
}

impl std::fmt::Display for ElevationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElevationError::Declined => {
                f.write_str("The permission prompt was closed, so nothing was installed. Dyno keeps working without the helper.")
            }
            ElevationError::Unavailable(m) | ElevationError::Failed(_, m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ElevationError {}

/// Runs one program elevated and waits for it. Implemented for the OS and, in tests, by a script.
pub trait Elevator: Send + Sync {
    /// Runs `program` (an absolute path) with `args` elevated; `prompt` is the reason the OS dialog may show. Returns
    /// when the program has exited with code 0.
    fn run(&self, program: &Path, args: &[String], prompt: &str) -> Result<(), ElevationError>;
}

/// How long the user may take to answer the system prompt, and the command to run.
pub const ELEVATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// An argument inside single quotes for `/bin/sh`: a quote inside becomes `'\''`. Control characters are refused
/// (there is no use for them in the paths and ids Dyno passes, and AppleScript would reinterpret some).
pub fn shell_quote(arg: &str) -> Result<String, String> {
    if arg.chars().any(|c| c.is_control()) {
        return Err("A control character cannot be passed to an elevated command.".into());
    }
    Ok(format!("'{}'", arg.replace('\'', "'\\''")))
}

/// The AppleScript that runs `program args…` as administrator, with the system password dialog showing `prompt`.
pub fn applescript(program: &str, args: &[String], prompt: &str) -> Result<String, String> {
    let mut command = shell_quote(program)?;
    for a in args {
        command.push(' ');
        command.push_str(&shell_quote(a)?);
    }
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
pub fn osascript_result(code: Option<i32>, stderr: &str) -> Result<(), ElevationError> {
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
            "The helper installer did not finish: {}",
            first.chars().take(240).collect::<String>()
        ),
    ))
}

/// What `pkexec` returning `code` with `stderr` means: 126 is "the user dismissed the dialog", 127 "not authorised"
/// or no authentication agent.
pub fn pkexec_result(code: Option<i32>, stderr: &str) -> Result<(), ElevationError> {
    match code {
        Some(0) => Ok(()),
        Some(126) => Err(ElevationError::Declined),
        Some(127) if stderr.to_lowercase().contains("no authentication agent") => Err(ElevationError::Unavailable(
            "No polkit authentication agent is running in this session, so the system cannot ask for your permission. Start your desktop's \
             authentication agent, or run `sudo` with the helper's install command yourself (see docs/linux.md)."
                .into(),
        )),
        Some(127) => Err(ElevationError::Declined),
        other => {
            let first = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("no message").trim();
            Err(ElevationError::Failed(
                other,
                format!("The helper installer did not finish: {}", first.chars().take(240).collect::<String>()),
            ))
        }
    }
}

/// The command line `CreateProcess` parses back into `args` (the rules `CommandLineToArgvW` documents): quote every
/// argument, double the backslashes before a quote and at the end, escape quotes.
pub fn windows_command_line(args: &[String]) -> Result<String, String> {
    let mut out = String::new();
    for (i, a) in args.iter().enumerate() {
        if a.contains('\0') || a.chars().any(|c| c == '\r' || c == '\n') {
            return Err("A control character cannot be passed to an elevated command.".into());
        }
        if i > 0 {
            out.push(' ');
        }
        out.push('"');
        let mut backslashes = 0usize;
        for c in a.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    out.push_str(&"\\".repeat(backslashes * 2 + 1));
                    out.push('"');
                    backslashes = 0;
                }
                c => {
                    out.push_str(&"\\".repeat(backslashes));
                    backslashes = 0;
                    out.push(c);
                }
            }
        }
        out.push_str(&"\\".repeat(backslashes * 2));
        out.push('"');
    }
    Ok(out)
}

/// The folders Windows names, from the environment (the usual defaults when a variable is missing).
pub fn windows_roots() -> dyno_core::helper_paths::WindowsRoots {
    let var = |name: &str, default: &str| {
        std::env::var_os(name)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(default))
    };
    dyno_core::helper_paths::WindowsRoots {
        program_files: var("ProgramFiles", r"C:\Program Files"),
        program_data: var("ProgramData", r"C:\ProgramData"),
        system_root: var("SystemRoot", r"C:\Windows"),
    }
}

/// The system Dyno runs on, as the tuning code names it.
pub fn this_host() -> dyno_core::helper::TuningHost {
    if cfg!(windows) {
        dyno_core::helper::TuningHost::Windows
    } else if cfg!(target_os = "macos") {
        dyno_core::helper::TuningHost::Macos
    } else {
        dyno_core::helper::TuningHost::Linux
    }
}

/// Where the helper lives on this computer.
pub fn helper_paths() -> dyno_core::helper_paths::HelperPaths {
    dyno_core::helper_paths::HelperPaths::for_host(this_host(), &windows_roots())
}

/// The elevator for the OS Dyno runs on.
pub struct OsElevator;

#[cfg(target_os = "macos")]
impl Elevator for OsElevator {
    fn run(&self, program: &Path, args: &[String], prompt: &str) -> Result<(), ElevationError> {
        let script = applescript(&program.to_string_lossy(), args, prompt).map_err(|m| ElevationError::Failed(None, m))?;
        let mut cmd = crate::process::command("/usr/bin/osascript");
        cmd.args(["-e", &script]);
        let out = crate::process::run_with_timeout(cmd, ELEVATION_TIMEOUT).map_err(|e| ElevationError::Unavailable(e.to_string()))?;
        osascript_result(out.status.code(), &String::from_utf8_lossy(&out.stderr))
    }
}

#[cfg(target_os = "linux")]
impl Elevator for OsElevator {
    fn run(&self, program: &Path, args: &[String], _prompt: &str) -> Result<(), ElevationError> {
        let pkexec = ["/usr/bin/pkexec", "/bin/pkexec"]
            .into_iter()
            .find(|p| Path::new(p).is_file())
            .ok_or_else(|| {
                ElevationError::Unavailable(
                    "pkexec (polkit) is not installed, so Dyno cannot ask for your permission. Install polkit, or see docs/linux.md."
                        .into(),
                )
            })?;
        let mut cmd = crate::process::command(pkexec);
        cmd.arg(program).args(args);
        let out = crate::process::run_with_timeout(cmd, ELEVATION_TIMEOUT).map_err(|e| ElevationError::Unavailable(e.to_string()))?;
        pkexec_result(out.status.code(), &String::from_utf8_lossy(&out.stderr))
    }
}

#[cfg(windows)]
impl Elevator for OsElevator {
    fn run(&self, program: &Path, args: &[String], _prompt: &str) -> Result<(), ElevationError> {
        let line = windows_command_line(args).map_err(|m| ElevationError::Failed(None, m))?;
        crate::windows::elevate::run_elevated(program, &line, ELEVATION_TIMEOUT)
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
impl Elevator for OsElevator {
    fn run(&self, _: &Path, _: &[String], _: &str) -> Result<(), ElevationError> {
        Err(ElevationError::Unavailable("The helper is not available on this system.".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_quotes_survive_the_shell() {
        assert_eq!(shell_quote("plain").unwrap(), "'plain'");
        assert_eq!(shell_quote("it's").unwrap(), "'it'\\''s'");
        assert_eq!(
            shell_quote("a b; rm -rf / $(x) `y` \"z\" \\").unwrap(),
            "'a b; rm -rf / $(x) `y` \"z\" \\'"
        );
        assert!(shell_quote("a\nb").is_err());
        assert!(shell_quote("a\0b").is_err());
    }

    #[test]
    fn the_shell_really_reads_back_what_was_quoted() {
        // Run the quoting through /bin/sh printf: every nasty argument comes back byte for byte.
        #[cfg(unix)]
        for nasty in [
            "a b",
            "it's",
            "$(touch /tmp/dyno-should-not-exist)",
            "`x`",
            "\"q\"",
            "back\\slash",
            "é ü",
            "-n",
            "*",
            "~",
        ] {
            let line = format!("printf %s {}", shell_quote(nasty).unwrap());
            let mut cmd = crate::process::command("/bin/sh");
            cmd.args(["-c", &line]);
            let out = crate::process::run_with_timeout(cmd, std::time::Duration::from_secs(5)).unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout), nasty);
        }
        assert!(!Path::new("/tmp/dyno-should-not-exist").exists());
    }

    #[test]
    fn the_applescript_quotes_twice_and_shows_the_prompt() {
        let s = applescript(
            "/Applications/Dyno.app/Contents/MacOS/dyno-helper",
            &[
                "install".into(),
                "--user".into(),
                "uid:501".into(),
                "--agent-exe".into(),
                "/A b/it's \"x\"".into(),
            ],
            "Dyno wants to install \"its helper\".",
        )
        .unwrap();
        assert_eq!(
            s,
            "do shell script \"'/Applications/Dyno.app/Contents/MacOS/dyno-helper' 'install' '--user' 'uid:501' '--agent-exe' \
             '/A b/it'\\\\''s \\\"x\\\"'\" with prompt \"Dyno wants to install \\\"its helper\\\".\" with administrator privileges"
        );
        assert!(applescript("/x", &["a\nb".into()], "p").is_err());
        assert!(applescript("/x", &[], "p\nq").is_err());
    }

    #[test]
    fn exit_codes_mean_declined_unavailable_or_failed() {
        assert_eq!(osascript_result(Some(0), ""), Ok(()));
        assert_eq!(
            osascript_result(Some(1), "execution error: User canceled. (-128)"),
            Err(ElevationError::Declined)
        );
        assert!(matches!(osascript_result(Some(1), "\nboom\n"), Err(ElevationError::Failed(Some(1), m)) if m.contains("boom")));
        assert_eq!(pkexec_result(Some(0), ""), Ok(()));
        assert_eq!(pkexec_result(Some(126), ""), Err(ElevationError::Declined));
        assert_eq!(
            pkexec_result(Some(127), "Error executing command as another user: Not authorized"),
            Err(ElevationError::Declined)
        );
        assert!(
            matches!(pkexec_result(Some(127), "Error: No authentication agent found."), Err(ElevationError::Unavailable(m)) if m.contains("authentication agent"))
        );
        assert!(matches!(pkexec_result(None, ""), Err(ElevationError::Failed(None, _))));
        assert!(matches!(pkexec_result(Some(2), "bad"), Err(ElevationError::Failed(Some(2), _))));
        assert!(ElevationError::Declined.to_string().contains("without the helper"));
    }

    #[test]
    fn windows_command_lines_follow_the_documented_rules() {
        let s = |a: &[&str]| windows_command_line(&a.iter().map(|x| x.to_string()).collect::<Vec<_>>()).unwrap();
        assert_eq!(
            s(&["install", "--user", "sid:S-1-5-21-1-2-3-1001"]),
            "\"install\" \"--user\" \"sid:S-1-5-21-1-2-3-1001\""
        );
        assert_eq!(
            s(&["C:\\Program Files\\Dyno\\dyno-agent.exe"]),
            "\"C:\\Program Files\\Dyno\\dyno-agent.exe\""
        );
        assert_eq!(s(&["a\"b"]), "\"a\\\"b\"");
        assert_eq!(s(&["dir\\"]), "\"dir\\\\\"");
        assert_eq!(s(&["x\\\"y"]), "\"x\\\\\\\"y\"");
        assert!(windows_command_line(&["a\nb".to_string()]).is_err());
    }
}
