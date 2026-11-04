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
    /// Check that this build can run (release gate: started from the extracted artifact).
    SelfTest,
}

const USAGE: &str = "usage: dyno-helper serve | service | install --for <uid:N|sid:S-1-…> | uninstall | --self-test | --version";

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter().map(String::as_str);
    match it.next() {
        Some("serve") if it.next().is_none() => Ok(Command::Serve),
        Some("service" | "--service") if it.next().is_none() => Ok(Command::Service),
        Some("uninstall") if it.next().is_none() => Ok(Command::Uninstall),
        Some("--version" | "version") if it.next().is_none() => Ok(Command::Version),
        Some("--self-test") if it.next().is_none() => Ok(Command::SelfTest),
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

/// The release gate's probe: nothing is installed, started or changed. It checks that the program can hash itself, that its
/// paths, configuration, policy and log formats work, and says which API it speaks.
///
/// # Errors
/// What does not work, in words.
pub fn self_test() -> Result<String, String> {
    use dyno_core::helper_wire::{HelperUser, HELPER_API};
    use paddock_service::privileged::{ProtectedFiles, RealFiles};
    let exe = std::env::current_exe().map_err(|e| format!("own path: {e}"))?;
    let hash = RealFiles.sha256(&exe).map_err(|e| format!("cannot read itself: {e}"))?;
    let paths = crate::elevate::helper_paths();
    if paths.endpoint.is_empty() || paths.config.as_os_str().is_empty() {
        return Err("the helper's places are empty".into());
    }
    let config = crate::config::HelperConfig::new(HelperUser::Uid(1), crate::VERSION);
    if crate::config::HelperConfig::parse(&config.to_text()).as_ref() != Ok(&config) {
        return Err("the configuration does not survive a round trip".into());
    }
    crate::policy::parse("others = \"total\"").map_err(|e| format!("policy: {e}"))?;
    Ok(format!(
        "dyno-helper {} ok: api {HELPER_API}, sha256 {}",
        crate::VERSION,
        hash.chars().take(12).collect::<String>()
    ))
}

/// Runs the daemon under `paddock_service::service_main` (the OS log from the first line, the exit code that names a
/// refusal, a panic caught at the boundary) and returns the process exit status.
fn serve(hosting: paddock_service::Hosting) -> i32 {
    let paths = crate::elevate::helper_paths();
    let result = paddock_service::service_main(&dyno_helper_service::spec(), hosting, move |context| {
        let early = context.early_sink();
        crate::serve::serve_with(&paths, context.stop, Some(early))
    });
    exit_status_of(result)
}

/// The process exit status of a finished service run: the body's code as a Unix status (0 stays 0), or 1 when the service
/// manager could not be reached (the reason is in the OS log and on standard error).
fn exit_status_of(result: Result<u32, paddock_service::ServiceMainError>) -> i32 {
    match result {
        Ok(code) => paddock_service::service_main::exit_status(code),
        Err(e) => {
            eprintln!("dyno-helper: {e}");
            1
        }
    }
}

/// Runs `command` and returns the process exit code (0 done, 1 failed, 2 usage).
pub fn run(command: Command) -> i32 {
    let result = match command {
        Command::Version => {
            println!("dyno-helper {}", crate::VERSION);
            Ok(())
        }
        Command::SelfTest => self_test().map(|line| println!("{line}")),
        Command::Serve => return serve(paddock_service::Hosting::Foreground),
        Command::Service => return serve(paddock_service::Hosting::ServiceManager),
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
        assert_eq!(parse(&args(&["--self-test"])), Ok(Command::SelfTest));
        assert_eq!(
            parse(&args(&["install", "--for", "uid:501"])),
            Ok(Command::Install {
                owner: HelperUser::Uid(501)
            })
        );
        assert!(parse(&args(&["install", "--for", "sid:S-1-5-21-1"])).is_ok());
    }

    #[test]
    fn a_finished_service_run_exits_with_its_code_and_a_missing_manager_exits_with_one() {
        use paddock_service::ServiceMainError;
        assert_eq!(exit_status_of(Ok(0)), 0);
        assert_eq!(exit_status_of(Ok(3)), 3);
        assert_eq!(exit_status_of(Ok(256)), 1, "never reads as success");
        assert_eq!(exit_status_of(Err(ServiceMainError::NotStartedByManager("x".into()))), 1);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_service_command_is_refused_off_windows_with_the_way_out() {
        // The refusal is the shared entry point's: the body never runs and the process exits 1.
        assert_eq!(run(Command::Service), 1);
    }

    #[test]
    fn the_self_test_passes_without_touching_anything() {
        let line = self_test().unwrap();
        assert!(line.contains(&format!("ok: api {}", dyno_core::helper_wire::HELPER_API)), "{line}");
        assert_eq!(run(Command::SelfTest), 0);
        assert_eq!(run(Command::Version), 0);
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
