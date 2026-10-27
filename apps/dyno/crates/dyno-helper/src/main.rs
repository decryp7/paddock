//! `dyno-helper`: Dyno's privileged helper (ADR 0006, 0016). See the library for what each command does.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match dyno_helper::cli::parse(&args) {
        Ok(command) => dyno_helper::cli::run(command),
        Err(usage) => {
            eprintln!("dyno-helper: {usage}");
            2
        }
    };
    std::process::exit(code);
}
