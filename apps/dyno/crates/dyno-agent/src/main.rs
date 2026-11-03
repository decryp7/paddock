//! The `dyno-agent` program.
//!
//! A Windows release build is a GUI-subsystem program: the per-user `Run` entry starts it at logon with `run`, and a
//! console-subsystem program would open a visible command prompt for as long as it runs. A prompt does not wait for a
//! GUI program, so its output can come after the prompt and its exit code is not set in `%ERRORLEVEL%`: for
//! `--version`, `status`, `stop`, `once` and `service ...` in a terminal use the console program `dyno agent <command>`
//! (same commands, same exit codes). The release workflow checks this file's promise on the built `dyno-agent.exe`
//! (`apps/dyno/scripts/check-pe-subsystem.mjs`: PE Subsystem is 2, GUI).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Redirected output (the app, CI, `Start-Process -RedirectStandardOutput`) works as is; a terminal's console is attached.
    paddock_proc::console::attach_parent_console();
    std::process::exit(dyno_agent::cli::main(std::env::args().skip(1).collect()));
}
