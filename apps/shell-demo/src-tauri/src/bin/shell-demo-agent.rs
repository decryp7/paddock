//! The demo's background agent (see `paddock_shell_demo::agent`): `shell-demo-agent [run|check|install|uninstall|status|stop|print-plist|print-unit]`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match paddock_shell_demo::agent_dirs() {
        Ok(dirs) => {
            if let Some(path) = paddock_service::supervised::supervised_path(
                paddock_shell_demo::agent::SUPERVISED_ENV,
            ) {
                // SAFETY: set once at start, before the agent starts any thread.
                unsafe { std::env::set_var("PATH", path) };
            }
            paddock_shell_demo::agent::main_with(&args, &dirs, paddock_shell_demo::VERSION)
        }
        Err(e) => {
            eprintln!("shell-demo-agent: the data folder cannot be found ({e})");
            1
        }
    };
    std::process::exit(code);
}
