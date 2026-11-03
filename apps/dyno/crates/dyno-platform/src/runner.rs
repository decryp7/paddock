//! Running a fixed program with validated arguments: [`Tool`], the one [`ToolRunner`] trait and the real [`SystemRunner`]
//! (`paddock_proc::run_bounded` underneath: a timeout that ends the whole process tree, bounded output, no console window).
//! Nothing here builds a command from free text.
//!
//! One trait serves every reader: [`ToolRunner::run_tool`] reports the exit (`ok`, stdout, the first stderr line) so a reader
//! that needs the output of a failing run (`smartctl` exits 4 with a full health log) can have it; [`ToolRunner::run`] is the
//! convenience that turns a failing run into an error carrying the start of its error output; [`ToolRunner::run_lines`] streams
//! a heavy read and stops it at once on Cancel.

use crate::process::{command, output_text, run_streaming_lowered, run_with_timeout};
use dyno_core::issues::ReadControl;
use dyno_core::ports::PlatformError;
use std::time::Duration;

/// How long a tool may run when its reader does not say (readers pass their own).
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(15);

/// What a tool answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// Exit code zero.
    pub ok: bool,
    /// Standard output, as text.
    pub stdout: String,
    /// The first line of the error output, for messages.
    pub error: String,
}

/// One program with its arguments. Programs come from a fixed list; arguments are validated values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    /// The program (an absolute path where the OS has a fixed place for it).
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Environment variables for this call (a value that must reach a program unparsed travels here, never in text a
    /// shell or a script would read as code).
    pub env: Vec<(String, String)>,
}

/// A [`Tool`] with no environment.
pub fn tool(program: impl Into<String>, args: &[&str]) -> Tool {
    Tool {
        program: program.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        env: Vec::new(),
    }
}

/// Runs external programs. A seam: tests script it, [`SystemRunner`] spawns the real tool.
pub trait ToolRunner: Send + Sync {
    /// Runs `tool` to completion within `timeout`. A non-zero exit is an answer (`ok == false`), not an error; a timeout, a
    /// missing program or a refusal is.
    fn run_tool(&self, tool: &Tool, timeout: Duration) -> Result<ToolOutput, PlatformError>;

    /// Runs `program` with `args` and returns its standard output; a non-zero exit is an error carrying the start of the
    /// tool's error output.
    fn run(&self, program: &str, args: &[String], timeout: Duration) -> Result<String, PlatformError> {
        let t = Tool {
            program: program.to_string(),
            args: args.to_vec(),
            env: Vec::new(),
        };
        let out = self.run_tool(&t, timeout)?;
        if out.ok {
            Ok(out.stdout)
        } else {
            let name = std::path::Path::new(program)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Err(PlatformError::Failed(format!(
                "{name} failed: {}",
                if out.error.is_empty() { "no message" } else { out.error.as_str() }
            )))
        }
    }

    /// Like [`run`](Self::run) with the output handed over line by line as it arrives, and the tool stopped at once when
    /// `control` is cancelled. The default runs the tool to the end and hands over its lines (no streaming, no early stop).
    fn run_lines(
        &self,
        program: &str,
        args: &[String],
        timeout: Duration,
        control: &dyn ReadControl,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<(), PlatformError> {
        let _ = control;
        self.run(program, args, timeout)?.lines().for_each(on_line);
        Ok(())
    }
}

/// Runs real programs: no console window, a timeout that ends the tree, output bounded.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl ToolRunner for SystemRunner {
    fn run_tool(&self, t: &Tool, timeout: Duration) -> Result<ToolOutput, PlatformError> {
        let mut cmd = command(&t.program);
        cmd.args(&t.args);
        for (k, v) in &t.env {
            cmd.env(k, v);
        }
        let out = run_with_timeout(cmd, timeout)?;
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

    fn run(&self, program: &str, args: &[String], timeout: Duration) -> Result<String, PlatformError> {
        let mut cmd = command(program);
        cmd.args(args);
        output_text(cmd, timeout)
    }

    fn run_lines(
        &self,
        program: &str,
        args: &[String],
        timeout: Duration,
        control: &dyn ReadControl,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<(), PlatformError> {
        // A heavy read of a log must not slow the user's work: lower priority the family's way (not through nice(1), which strips
        // DYLD_* on macOS).
        let mut cmd = command(program);
        cmd.args(args);
        paddock_proc::lower_priority(&mut cmd);
        run_streaming_lowered(cmd, timeout, control, on_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OnlyTool(ToolOutput);
    impl ToolRunner for OnlyTool {
        fn run_tool(&self, _: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn run_turns_a_failing_exit_into_an_error_with_the_start_of_stderr() {
        let ok = OnlyTool(ToolOutput {
            ok: true,
            stdout: "a\nb\n".into(),
            error: String::new(),
        });
        assert_eq!(ok.run("/x/tool", &[], TOOL_TIMEOUT).unwrap(), "a\nb\n");
        let mut seen = Vec::new();
        struct Never;
        impl ReadControl for Never {
            fn cancelled(&self) -> bool {
                false
            }
            fn parsed(&self, _: u64) {}
            fn on_cancel(&self, _: Box<dyn Fn() + Send + Sync>) -> Box<dyn Send> {
                Box::new(())
            }
        }
        ok.run_lines("/x/tool", &[], TOOL_TIMEOUT, &Never, &mut |l| seen.push(l.to_string()))
            .unwrap();
        assert_eq!(seen, ["a", "b"]);
        let bad = OnlyTool(ToolOutput {
            ok: false,
            stdout: "out".into(),
            error: "denied".into(),
        });
        let e = bad.run("/x/tool", &[], TOOL_TIMEOUT).unwrap_err();
        assert!(matches!(&e, PlatformError::Failed(m) if m == "tool failed: denied"), "{e:?}");
        assert_eq!(
            bad.run_tool(&tool("/x/tool", &[]), TOOL_TIMEOUT).unwrap().stdout,
            "out",
            "the answer of a failing run stays readable"
        );
    }

    #[test]
    fn the_real_runner_runs_a_program_reports_a_failing_exit_and_a_missing_program() {
        let r = SystemRunner;
        let (program, ok_args, bad_args): (&str, &[&str], &[&str]) = if cfg!(windows) {
            ("cmd.exe", &["/d", "/c", "echo hi"], &["/d", "/c", "exit 3"])
        } else {
            ("/bin/sh", &["-c", "echo hi"], &["-c", "echo oops >&2; exit 3"])
        };
        let out = r.run_tool(&tool(program, ok_args), Duration::from_secs(10)).unwrap();
        assert!(out.ok && out.stdout.contains("hi"));
        let out = r.run_tool(&tool(program, bad_args), Duration::from_secs(10)).unwrap();
        assert!(!out.ok);
        assert!(matches!(
            r.run_tool(&tool("/definitely/not/a/tool", &[]), Duration::from_secs(2)),
            Err(PlatformError::Unsupported(_))
        ));
    }
}
