//! `paddock-dev`, the developer and CI tool (ADR 0022). `./dev` builds or fetches this binary and runs it.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = paddock_dev::run(&args, &mut std::io::stdout(), &mut std::io::stderr());
    std::process::exit(code);
}
