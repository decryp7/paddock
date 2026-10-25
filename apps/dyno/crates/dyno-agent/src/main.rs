//! The `dyno-agent` program.
fn main() {
    std::process::exit(dyno_agent::cli::main(std::env::args().skip(1).collect()));
}
