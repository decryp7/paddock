//! The `dyno` program.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The agent's own commands (`dyno agent --version`, `dyno agent service install`) with this console and the agent's exit code.
    if let Some(rest) = dyno_cli::agent_args(&args) {
        std::process::exit(run_agent(rest));
    }
    let env = dyno_cli::Env::from_process();
    let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr().lock());
    std::process::exit(dyno_cli::run(&args, &env, &mut out, &mut err));
}

/// Runs the `dyno-agent` program next to this one with `args`, on this console, and returns its exit code. The CLI never
/// links the agent (ADR 0014); on Windows the agent is a GUI-subsystem program and a prompt neither waits for it nor shows its
/// output, but a console program that starts it and waits does.
#[expect(clippy::disallowed_methods, reason = "the CLI has no process helper crate; a fixed sibling program, inherited stdio")]
fn run_agent(args: &[String]) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(p) => p.with_file_name(if cfg!(windows) { "dyno-agent.exe" } else { "dyno-agent" }),
        Err(e) => {
            eprintln!("dyno: cannot find the agent program ({e})");
            return 1;
        }
    };
    match std::process::Command::new(&exe).args(args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("dyno: could not run {} ({e})", exe.display());
            1
        }
    }
}
