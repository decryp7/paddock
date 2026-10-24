//! `paddock-bench check|propose`: see [`paddock_bench::run`].

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exit = paddock_bench::run(&args, &mut std::io::stdout(), &mut std::io::stderr());
    std::process::ExitCode::from(u8::try_from(exit.code()).unwrap_or(1))
}
