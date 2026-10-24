//! The demo's background agent (see the `shell_demo_agent` library):
//! `shell-demo-agent [run|check|--self-test|install|uninstall|status|stop|print-plist|print-unit]`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The bundle check (rule 23) starts the agent from inside the built app: it must load and answer, touching nothing.
    if args.first().map(String::as_str) == Some(shell_demo_agent::SELF_TEST) {
        println!("{}", shell_demo_agent::self_test());
        std::process::exit(0);
    }
    let code = match shell_demo_agent::dirs() {
        Ok(dirs) => {
            if let Some(path) =
                paddock_service::supervised::supervised_path(shell_demo_agent::SUPERVISED_ENV)
            {
                // SAFETY: set once at start, before the agent starts any thread.
                unsafe { std::env::set_var("PATH", path) };
            }
            shell_demo_agent::main_with(&args, &dirs, shell_demo_agent::VERSION)
        }
        Err(e) => {
            eprintln!("shell-demo-agent: the data folder cannot be found ({e})");
            1
        }
    };
    std::process::exit(code);
}
