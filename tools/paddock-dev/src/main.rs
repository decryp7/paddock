//! `paddock-dev`, the developer and CI tool (ADR 0022). `./dev` builds or fetches this binary and runs it.
fn main() {
    // First, before any thread: a detached child (a slot holder, the trial profile's slow phase) closes the descriptors it inherited
    // from the step that started it, so a pipe the runner waits on never outlives the step (run 8019). `spawn_detached` marks it.
    paddock_proc::close_inherited_if_marked();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Run from the tool cache, the tool holds its own binary until it ends, so its children start a path no prune removes. Unix
    // only: Windows cannot remove a running program's link (the hold would stay for the week-long sweep) nor its cache entry.
    #[cfg(unix)]
    paddock_dev::tool_cache::hold_self(&paddock_dev::process_env::vars().collect());
    let code = paddock_dev::run(&args, &mut std::io::stdout(), &mut std::io::stderr());
    #[cfg(unix)]
    paddock_dev::tool_cache::release_self();
    std::process::exit(code);
}
