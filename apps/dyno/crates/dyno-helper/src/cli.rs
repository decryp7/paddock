//! The commands of the `dyno-helper` program, parsed once at the edge.
//!
//! - `serve`: the daemon (launchd and systemd run it; a person can run it in a terminal for debugging).
//! - `service`: the daemon under the Windows Service Control Manager.
//! - `install --for <uid:N|sid:S-1-…>`: the elevated installer Dyno starts after the user clicked (also the update).
//! - `uninstall`: removes the helper (elevated, for when the daemon cannot be asked).

use dyno_core::helper_wire::HelperUser;

/// What the program was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Run the daemon.
    Serve,
    /// Run the daemon as a Windows service.
    Service,
    /// Install for one user.
    Install {
        /// The account that turned the helper on (its owner).
        owner: HelperUser,
    },
    /// Remove the helper.
    Uninstall,
    /// Print the version.
    Version,
}

const USAGE: &str = "usage: dyno-helper serve | service | install --for <uid:N|sid:S-1-…> | uninstall | --version";

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter().map(String::as_str);
    match it.next() {
        Some("serve") if it.next().is_none() => Ok(Command::Serve),
        Some("service" | "--service") if it.next().is_none() => Ok(Command::Service),
        Some("uninstall") if it.next().is_none() => Ok(Command::Uninstall),
        Some("--version" | "version") if it.next().is_none() => Ok(Command::Version),
        Some("install") => match (it.next(), it.next(), it.next()) {
            (Some("--for"), Some(owner), None) => Ok(Command::Install {
                owner: HelperUser::parse(owner)?,
            }),
            _ => Err(format!("install needs --for <owner>. {USAGE}")),
        },
        _ => Err(USAGE.to_string()),
    }
}

fn report(done: &str, r: &paddock_service::privileged::Report) {
    for p in &r.phases {
        println!("{done}: {:?} ({} ms) {}", p.phase, p.ms, p.detail);
    }
}

fn serve() -> Result<(), String> {
    crate::serve::serve(
        &crate::elevate::helper_paths(),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
}

/// Runs `command` and returns the process exit code (0 done, 1 failed, 2 usage).
pub fn run(command: Command) -> i32 {
    let result = match command {
        Command::Version => {
            println!("dyno-helper {}", crate::VERSION);
            Ok(())
        }
        Command::Serve => serve(),
        #[cfg(windows)]
        Command::Service => crate::winsvc::run_as_service(),
        #[cfg(not(windows))]
        Command::Service => Err("The service command is for Windows; use `serve`.".to_string()),
        Command::Install { owner } => crate::setup::install(owner).map(|r| report("install", &r)),
        Command::Uninstall => crate::setup::uninstall().map(|r| report("uninstall", &r)),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("dyno-helper: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_commands_parse() {
        assert_eq!(parse(&args(&["serve"])), Ok(Command::Serve));
        assert_eq!(parse(&args(&["service"])), Ok(Command::Service));
        // The SCM starts the service with the verb paddock-service gives every background program.
        assert_eq!(parse(&args(&["--service"])), Ok(Command::Service));
        assert_eq!(parse(&args(&["uninstall"])), Ok(Command::Uninstall));
        assert_eq!(parse(&args(&["--version"])), Ok(Command::Version));
        assert_eq!(
            parse(&args(&["install", "--for", "uid:501"])),
            Ok(Command::Install {
                owner: HelperUser::Uid(501)
            })
        );
        assert!(parse(&args(&["install", "--for", "sid:S-1-5-21-1"])).is_ok());
    }

    #[test]
    fn anything_else_is_a_usage_error_that_says_how_to_call_it() {
        assert!(parse(&args(&[])).unwrap_err().contains("usage: dyno-helper"));
        for bad in [
            vec![],
            vec!["serve", "now"],
            vec!["install"],
            vec!["install", "--for"],
            vec!["install", "--for", "root"],
            vec!["install", "--for", "uid:1", "--for", "uid:2"],
            vec!["install", "--user", "uid:1"],
            vec!["rm", "-rf"],
        ] {
            let e = parse(&args(&bad)).unwrap_err();
            assert!(!e.is_empty(), "{bad:?}");
        }
    }
}
