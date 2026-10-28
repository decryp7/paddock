//! Running a fixed program with validated arguments: [`Tool`], [`ToolRunner`] and the real [`SystemRunner`] (no console
//! window, a timeout, bounded output). Used to read what only a program can tell (the helper's install steps and
//! its checks). Nothing here builds a command from free text.

use crate::process::{command, run_with_timeout};
use dyno_core::ports::PlatformError;
use std::time::Duration;

/// Every tool call is bounded.
const TOOL_TIMEOUT: Duration = Duration::from_secs(15);

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

/// Runs a [`Tool`]. Implemented for real processes and, in tests, by a script.
pub trait ToolRunner: Send {
    /// Runs the tool to completion.
    fn run(&mut self, tool: &Tool) -> Result<ToolOutput, PlatformError>;
}

/// Runs real programs: no console window, a timeout, output bounded.
pub struct SystemRunner;

impl ToolRunner for SystemRunner {
    fn run(&mut self, t: &Tool) -> Result<ToolOutput, PlatformError> {
        let mut cmd = command(&t.program);
        cmd.args(&t.args);
        for (k, v) in &t.env {
            cmd.env(k, v);
        }
        let out = run_with_timeout(cmd, TOOL_TIMEOUT)?;
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
