//! The `dyno` program.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env = dyno_cli::Env::from_process();
    let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr().lock());
    std::process::exit(dyno_cli::run(&args, &env, &mut out, &mut err));
}
