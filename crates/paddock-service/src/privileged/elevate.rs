//! The one OS prompt of an install (ADR 0027 §1, R1): macOS asks for the account password through `osascript` "with
//! administrator privileges" (the dialog names `osascript`, not the app, so the app says so first), Windows through UAC
//! (`ShellExecuteExW` verb `runas`, the [`Elevator`] port), Linux through `pkexec`. The command lines are pure and
//! golden-tested; the exit codes are mapped to what the user did.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::port::{Elevator, Output, RegisterError, RunError, System};
use paddock_env::Os;
use std::path::Path;
use std::time::Duration;

/// How long an elevation prompt and the elevated install may take in all (the user types a password).
pub const ELEVATION_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// `osascript`.
pub const OSASCRIPT: &str = "/usr/bin/osascript";
/// `pkexec`.
pub const PKEXEC: &str = "pkexec";
/// The command prefix that gives the elevated program an empty environment and the system's PATH.
const ELEVATED_ENV: &str = "/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin";
/// `osascript`'s error number for "User canceled".
const MAC_CANCELLED: &str = "(-128)";

/// A program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElevatedCommand {
    /// The program that shows the prompt (`osascript`, `pkexec`).
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
}

/// Why the elevated run did not happen or did not work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevationError {
    /// The user closed or cancelled the prompt. Nothing was changed.
    Declined,
    /// The user is not allowed to authorise (no polkit rule, not an administrator, wrong password).
    NotAuthorized(String),
    /// The prompt's program is missing or cannot run here.
    Unavailable(String),
    /// The prompt ran and the elevated program had not ended at the ceiling, or the wait on it broke: it was not stopped and
    /// may still finish. The text says what was seen.
    StillRunning(String),
    /// A part of the command cannot be passed safely (a control character).
    Invalid(String),
    /// The elevated program ran and failed; the text is its own words.
    Failed {
        /// Its exit code.
        code: Option<i32>,
        /// What it said, first line.
        reason: String,
    },
}

impl std::fmt::Display for ElevationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElevationError::Declined => {
                f.write_str("The permission prompt was declined, so nothing was changed.")
            }
            ElevationError::NotAuthorized(why) => write!(
                f,
                "You are not allowed to do this ({why}); ask an administrator."
            ),
            ElevationError::Unavailable(why) => {
                write!(f, "The system's permission prompt cannot run here: {why}")
            }
            ElevationError::StillRunning(why) => write!(
                f,
                "The elevated step is still running and was not stopped; it may still finish. ({why})"
            ),
            ElevationError::Invalid(why) => {
                write!(f, "The install command is not safe to pass on: {why}")
            }
            ElevationError::Failed { code, reason } => match code {
                Some(c) => write!(
                    f,
                    "The install failed (exit code {c}): {reason}. The helper's log has the details."
                ),
                None => write!(
                    f,
                    "The install was stopped: {reason}. The helper's log has the details."
                ),
            },
        }
    }
}

impl std::error::Error for ElevationError {}

fn no_control(words: &[&str]) -> Result<(), ElevationError> {
    match words.iter().find(|w| w.chars().any(char::is_control)) {
        Some(_) => Err(ElevationError::Invalid(
            "an argument has a control character".into(),
        )),
        None => Ok(()),
    }
}

/// One shell word in single quotes (`'` becomes `'\''`).
fn shell_quote(w: &str) -> String {
    format!("'{}'", w.replace('\'', "'\\''"))
}

/// An AppleScript string literal.
fn applescript_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// macOS: `osascript -e 'do shell script "…" with prompt "…" with administrator privileges'`. The program's output
/// (standard error too) comes back in `osascript`'s error text, which is how a failure reaches the log.
///
/// # Errors
/// [`ElevationError::Invalid`] for a control character in any part.
pub fn osascript_command(
    program: &Path,
    args: &[&str],
    prompt: &str,
) -> Result<ElevatedCommand, ElevationError> {
    let program = program.to_string_lossy();
    let mut words = vec![program.as_ref(), prompt];
    words.extend_from_slice(args);
    no_control(&words)?;
    // `env -i`: the elevated program starts with no environment of the caller's, only the fixed system PATH (an inherited
    // `DYLD_*`, `PATH` or `TMPDIR` of the user's session would otherwise steer what runs as root).
    let mut shell = vec![ELEVATED_ENV.to_string(), shell_quote(&program)];
    shell.extend(args.iter().map(|a| shell_quote(a)));
    let script = format!(
        "do shell script {} with prompt {} with administrator privileges",
        applescript_quote(&format!("{} 2>&1", shell.join(" "))),
        applescript_quote(prompt)
    );
    Ok(ElevatedCommand {
        program: OSASCRIPT.into(),
        args: vec!["-e".into(), script],
    })
}

/// Linux: `pkexec <absolute program> args…` (pkexec runs only an absolute path).
///
/// # Errors
/// [`ElevationError::Invalid`] for a relative program or a control character.
pub fn pkexec_command(program: &Path, args: &[&str]) -> Result<ElevatedCommand, ElevationError> {
    let p = program.to_string_lossy();
    if !p.starts_with('/') {
        return Err(ElevationError::Invalid(
            "pkexec needs the program's absolute path".into(),
        ));
    }
    let mut words = vec![p.as_ref()];
    words.extend_from_slice(args);
    no_control(&words)?;
    let mut all = vec![p.to_string()];
    all.extend(args.iter().map(|a| (*a).to_string()));
    Ok(ElevatedCommand {
        program: PKEXEC.into(),
        args: all,
    })
}

/// What `osascript` said, in the elevated program's words: `execution error: <output> (<n>)` loses its frame.
fn mac_reason(stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let line = line
        .split_once("execution error: ")
        .map_or(line, |(_, rest)| rest);
    let line = line.rsplit_once(" (").map_or(
        line,
        |(text, tail)| if tail.ends_with(')') { text } else { line },
    );
    line.chars().take(300).collect()
}

fn classify_mac(o: &Output) -> Result<(), ElevationError> {
    if o.success() {
        return Ok(());
    }
    if o.stderr.contains(MAC_CANCELLED) || o.stderr.contains("User canceled") {
        return Err(ElevationError::Declined);
    }
    let reason = mac_reason(&o.stderr);
    Err(ElevationError::Failed {
        code: o.code,
        reason: if reason.is_empty() {
            o.reason()
        } else {
            reason
        },
    })
}

fn classify_pkexec(o: &Output) -> Result<(), ElevationError> {
    match o.code {
        Some(0) => Ok(()),
        // pkexec(1): 126 when the user dismissed the dialog, 127 when authorisation failed or was not possible.
        Some(126) => Err(ElevationError::Declined),
        Some(127) => Err(ElevationError::NotAuthorized(o.reason())),
        code => Err(ElevationError::Failed {
            code,
            reason: o.reason(),
        }),
    }
}

fn run_command(sys: &mut dyn System, cmd: &ElevatedCommand) -> Result<Output, ElevationError> {
    let args: Vec<&str> = cmd.args.iter().map(String::as_str).collect();
    sys.run(&cmd.program, &args).map_err(|e| match e {
        RunError::NotStarted(why) => ElevationError::Unavailable(why),
        RunError::TimedOut(what) => ElevationError::Failed {
            code: None,
            reason: format!("{what} did not finish in time (was the prompt left open?)"),
        },
    })
}

/// Runs `program args…` with administrator rights through the OS's own prompt for `os` and waits. `sys` must have a
/// timeout of [`ELEVATION_TIMEOUT`]; `elevator` is Windows' UAC (unused elsewhere); `prompt` is the macOS dialog's text.
///
/// # Errors
/// What the user did or what went wrong, as an [`ElevationError`].
pub fn run_elevated(
    os: Os,
    sys: &mut dyn System,
    elevator: &mut dyn Elevator,
    program: &Path,
    args: &[&str],
    prompt: &str,
) -> Result<(), ElevationError> {
    let started = std::time::Instant::now();
    let result = match os {
        Os::MacOs => osascript_command(program, args, prompt)
            .and_then(|c| classify_mac(&run_command(sys, &c)?)),
        Os::Linux => {
            pkexec_command(program, args).and_then(|c| classify_pkexec(&run_command(sys, &c)?))
        }
        // ELEVATION_TIMEOUT is the elevator's backstop ceiling, not a judge: a program still running then (or a wait that broke)
        // comes back as RegisterError::StillRunning and is reported as ElevationError::StillRunning: not a missing prompt and
        // never Failed (ER-19; before the elevated wait was ported it read as exit code 1, Failed).
        Os::Windows => match elevator.run_elevated(program, args, ELEVATION_TIMEOUT) {
            Ok(0) => Ok(()),
            Ok(code) => Err(ElevationError::Failed {
                code: Some(code),
                reason: "the elevated program reported an error".into(),
            }),
            Err(RegisterError::Declined) => Err(ElevationError::Declined),
            Err(RegisterError::StillRunning(why)) => Err(ElevationError::StillRunning(why)),
            Err(RegisterError::NeedsElevation) => Err(ElevationError::NotAuthorized(
                "Windows refused to elevate".into(),
            )),
            Err(e) => Err(ElevationError::Unavailable(e.to_string())),
        },
    };
    match &result {
        Ok(()) => {
            log::info!(os:? = os, ms = started.elapsed().as_millis() as u64; "the elevated run finished")
        }
        Err(e) => {
            log::warn!(os:? = os, ms = started.elapsed().as_millis() as u64, error:% = e; "the elevated run did not finish")
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeElevator, FakeSystem};

    #[test]
    fn the_macos_command_quotes_for_the_shell_and_for_applescript() {
        let c = osascript_command(
            Path::new("/Applications/Dyno.app/Contents/MacOS/dyno-helper"),
            &["install", "--note", "it's \"x\""],
            "Dyno wants to install its helper",
        )
        .unwrap();
        assert_eq!(c.program, "/usr/bin/osascript");
        assert_eq!(c.args[0], "-e");
        assert_eq!(
            c.args[1],
            "do shell script \"/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin '/Applications/Dyno.app/Contents/MacOS/dyno-helper' 'install' '--note' 'it'\\\\''s \\\"x\\\"' 2>&1\" \
             with prompt \"Dyno wants to install its helper\" with administrator privileges"
        );
        assert!(matches!(
            osascript_command(Path::new("/p"), &["a\nb"], "x"),
            Err(ElevationError::Invalid(_))
        ));
        assert!(matches!(
            osascript_command(Path::new("/p"), &[], "x\ny"),
            Err(ElevationError::Invalid(_))
        ));
    }

    #[test]
    fn the_linux_command_needs_an_absolute_program() {
        let c = pkexec_command(Path::new("/usr/bin/dyno-helper"), &["install"]).unwrap();
        assert_eq!(
            (c.program.as_str(), c.args),
            (
                "pkexec",
                vec!["/usr/bin/dyno-helper".to_string(), "install".to_string()]
            )
        );
        assert!(matches!(
            pkexec_command(Path::new("dyno-helper"), &[]),
            Err(ElevationError::Invalid(_))
        ));
        assert!(matches!(
            pkexec_command(Path::new("/p"), &["a\0"]),
            Err(ElevationError::Invalid(_))
        ));
    }

    #[test]
    fn macos_outcomes_are_what_the_user_did() {
        let run = |o: Output| {
            let mut sys = FakeSystem::new();
            sys.answer("/usr/bin/osascript", o);
            run_elevated(
                Os::MacOs,
                &mut sys,
                &mut FakeElevator::new(&FakeSystem::new()),
                Path::new("/p"),
                &["install"],
                "x",
            )
        };
        assert_eq!(run(Output::ok("done")), Ok(()));
        let cancelled = Output {
            code: Some(1),
            stderr: "0:80: execution error: User canceled. (-128)\n".into(),
            ..Output::default()
        };
        assert_eq!(run(cancelled), Err(ElevationError::Declined));
        let failed = Output {
            code: Some(1),
            stderr: "55:117: execution error: refused: unsafe folder (1)\n".into(),
            ..Output::default()
        };
        assert_eq!(
            run(failed),
            Err(ElevationError::Failed {
                code: Some(1),
                reason: "refused: unsafe folder".into()
            })
        );
        let silent = Output {
            code: Some(2),
            ..Output::default()
        };
        assert_eq!(
            run(silent),
            Err(ElevationError::Failed {
                code: Some(2),
                reason: "it exited with code 2".into()
            })
        );
    }

    #[test]
    fn linux_outcomes_follow_pkexecs_exit_codes() {
        let run = |code| {
            let mut sys = FakeSystem::new();
            sys.answer(
                "pkexec",
                Output {
                    code: Some(code),
                    stderr: "Error: not authorized\n".into(),
                    ..Output::default()
                },
            );
            run_elevated(
                Os::Linux,
                &mut sys,
                &mut FakeElevator::new(&FakeSystem::new()),
                Path::new("/usr/bin/p"),
                &["install"],
                "x",
            )
        };
        assert_eq!(run(0), Ok(()));
        assert_eq!(run(126), Err(ElevationError::Declined));
        assert_eq!(
            run(127),
            Err(ElevationError::NotAuthorized(
                "Error: not authorized".into()
            ))
        );
        assert!(matches!(
            run(3),
            Err(ElevationError::Failed { code: Some(3), .. })
        ));
    }

    #[test]
    fn windows_goes_through_the_elevator_and_a_missing_prompt_program_is_said() {
        let log = FakeSystem::new();
        let mut elevator = FakeElevator::new(&log);
        let mut sys = FakeSystem::new();
        let go = |sys: &mut FakeSystem, e: &mut FakeElevator| {
            run_elevated(
                Os::Windows,
                sys,
                e,
                Path::new("C:\\p\\dyno-helper.exe"),
                &["install"],
                "x",
            )
        };
        assert_eq!(go(&mut sys, &mut elevator), Ok(()));
        elevator.results([
            Ok(5),
            Err(RegisterError::Declined),
            Err(RegisterError::NeedsElevation),
            Err(RegisterError::Other("no shell".into())),
        ]);
        assert!(matches!(
            go(&mut sys, &mut elevator),
            Err(ElevationError::Failed { code: Some(5), .. })
        ));
        assert_eq!(go(&mut sys, &mut elevator), Err(ElevationError::Declined));
        assert!(matches!(
            go(&mut sys, &mut elevator),
            Err(ElevationError::NotAuthorized(_))
        ));
        assert!(matches!(
            go(&mut sys, &mut elevator),
            Err(ElevationError::Unavailable(_))
        ));
        assert!(
            log.calls()
                .iter()
                .any(|c| matches!(c, crate::testing::Call::Os(s) if s.starts_with("elevated ")))
        );
        // The elevated program still runs at the ceiling, or the wait on it broke: the prompt did run, so this is neither a
        // missing prompt nor a failure, and the words say what to do next.
        let program = Path::new("C:\\p\\dyno-helper.exe");
        for end in [
            crate::port::ElevatedEnd::StillRunning(ELEVATION_TIMEOUT),
            crate::port::ElevatedEnd::WaitFailed(6),
        ] {
            elevator.results([crate::port::elevated_result(program, end)]);
            let e = go(&mut sys, &mut elevator).unwrap_err();
            assert!(
                matches!(e, ElevationError::StillRunning(_)),
                "{end:?}: {e:?}"
            );
            let shown = e.to_string();
            // Neutral words (each app adds its own next step), said once.
            assert!(
                shown.starts_with(
                    "The elevated step is still running and was not stopped; it may still finish."
                ) && shown.matches("may still finish").count() == 1
                    && !shown.contains("Settings")
                    && !shown.contains("cannot run here"),
                "{shown}"
            );
        }
        // A prompt program that cannot run stays apart from it.
        elevator.results([Err(RegisterError::Other("no shell".into()))]);
        assert!(matches!(
            go(&mut sys, &mut elevator),
            Err(ElevationError::Unavailable(_))
        ));
        // A prompt program that is missing or hangs.
        let mut sys = FakeSystem::new();
        sys.on("pkexec", |_, _| {
            Err(RunError::NotStarted("pkexec: not found".into()))
        });
        let r = run_elevated(
            Os::Linux,
            &mut sys,
            &mut elevator,
            Path::new("/p"),
            &[],
            "x",
        );
        assert!(matches!(r, Err(ElevationError::Unavailable(_))));
        sys.on("pkexec", |_, _| Err(RunError::TimedOut("pkexec".into())));
        let r = run_elevated(
            Os::Linux,
            &mut sys,
            &mut elevator,
            Path::new("/p"),
            &[],
            "x",
        );
        assert!(matches!(r, Err(ElevationError::Failed { code: None, .. })));
    }

    #[test]
    fn every_error_says_what_to_do() {
        for e in [
            ElevationError::Declined,
            ElevationError::NotAuthorized("x".into()),
            ElevationError::Unavailable("x".into()),
            ElevationError::Invalid("x".into()),
            ElevationError::Failed {
                code: Some(1),
                reason: "x".into(),
            },
            ElevationError::Failed {
                code: None,
                reason: "x".into(),
            },
        ] {
            assert!(!e.to_string().is_empty());
        }
    }
}
