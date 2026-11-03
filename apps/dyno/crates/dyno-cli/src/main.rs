//! The `dyno` program.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The agent's own commands (`dyno agent --version`, `dyno agent service install`) with a console and real exit codes.
    if let Some(rest) = dyno_cli::agent_args(&args) {
        std::process::exit(dyno_agent::cli::main(rest.to_vec()));
    }
    let env = dyno_cli::Env::from_process();
    let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr().lock());
    std::process::exit(dyno_cli::run(&args, &env, &mut out, &mut err));
}
