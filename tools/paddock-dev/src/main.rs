//! `paddock-dev`, the developer and CI tool (ADR 0022). `./dev` builds or fetches this binary and runs it.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Run from the tool cache, the tool holds its own binary until it ends, so its children start a path no prune removes. Unix
    // only: Windows cannot remove a running program's link (the hold would stay for the week-long sweep) nor its cache entry.
    #[cfg(unix)]
    paddock_dev::tool_cache::hold_self(&std::env::vars().collect());
    let code = paddock_dev::run(&args, &mut std::io::stdout(), &mut std::io::stderr());
    #[cfg(unix)]
    paddock_dev::tool_cache::release_self();
    std::process::exit(code);
}
