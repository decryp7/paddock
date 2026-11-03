//! The three commands of the `dyno-helper` program, parsed once at the edge.
//!
//! - `serve`: the daemon (launchd and systemd run it; a person can run it in a terminal for debugging).
//! - `service`: the daemon under the Windows Service Control Manager.
//! - `install --user <uid:N|sid:S-1-…> --agent-exe <path>`: the elevated installer Dyno starts after the user clicked.
//! - `uninstall`: removes the helper (elevated, for when the daemon cannot be asked).

use crate::install::{execute, install_plan, uninstall_plan, InstallParams, SystemMachine};
use dyno_core::helper_wire::HelperUser;
use std::path::PathBuf;

/// What the program was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Run the daemon.
    Serve,
    /// Run the daemon as a Windows service.
    Service,
    /// Install for one user.
    Install {
        /// The user.
        user: HelperUser,
        /// The agent program that may call.
        agent_exe: PathBuf,
    },
    /// Remove the helper.
    Uninstall,
    /// Print the version.
    Version,
}

const USAGE: &str = "usage: dyno-helper serve | service | install --user <uid:N|sid:S-1-…> --agent-exe <path> | uninstall | --version";

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter().map(String::as_str);
    match it.next() {
        Some("serve") if it.next().is_none() => Ok(Command::Serve),
        Some("service") if it.next().is_none() => Ok(Command::Service),
        Some("uninstall") if it.next().is_none() => Ok(Command::Uninstall),
        Some("--version" | "version") if it.next().is_none() => Ok(Command::Version),
        Some("install") => {
            let (mut user, mut agent) = (None, None);
            while let Some(flag) = it.next() {
                let value = it.next().ok_or_else(|| format!("{flag} needs a value. {USAGE}"))?;
                match flag {
                    "--user" if user.is_none() => user = Some(HelperUser::parse(value)?),
                    "--agent-exe" if agent.is_none() => agent = Some(PathBuf::from(value)),
                    other => return Err(format!("Unexpected {other}. {USAGE}")),
                }
            }
            match (user, agent) {
                (Some(user), Some(agent_exe)) => Ok(Command::Install { user, agent_exe }),
                _ => Err(format!("install needs --user and --agent-exe. {USAGE}")),
            }
        }
        _ => Err(USAGE.to_string()),
    }
}

fn install(user: HelperUser, agent_exe: PathBuf) -> Result<(), String> {
    #[cfg(unix)]
    if !crate::sys::is_elevated() {
        return Err("Installing the helper needs administrator rights; start it from Dyno (Settings > Helper).".into());
    }
    let agent_exe = std::fs::canonicalize(&agent_exe)
        .ok()
        .filter(|p| p.is_file())
        .ok_or_else(|| format!("{} is not a program.", agent_exe.display()))?;
    let windows = crate::elevate::windows_roots();
    let paths = crate::elevate::helper_paths();
    let plan = install_plan(
        &paths,
        &windows,
        &InstallParams {
            user,
            agent_exe,
            version: crate::VERSION.to_string(),
        },
    )?;
    let mut machine = SystemMachine::new()?;
    if let Err(e) = execute(&mut machine, &plan) {
        // Leave nothing half installed.
        let _ = execute(&mut machine, &uninstall_plan(&paths, &windows));
        return Err(e);
    }
    Ok(())
}

fn uninstall() -> Result<(), String> {
    #[cfg(unix)]
    if !crate::sys::is_elevated() {
        return Err("Removing the helper needs administrator rights; use Dyno's Settings > Helper, or run this with sudo.".into());
    }
    let mut machine = SystemMachine::new()?;
    execute(
        &mut machine,
        &uninstall_plan(&crate::elevate::helper_paths(), &crate::elevate::windows_roots()),
    )
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
        Command::Install { user, agent_exe } => install(user, agent_exe),
        Command::Uninstall => uninstall(),
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
        assert_eq!(parse(&args(&["uninstall"])), Ok(Command::Uninstall));
        assert_eq!(parse(&args(&["--version"])), Ok(Command::Version));
        assert_eq!(
            parse(&args(&["install", "--user", "uid:501", "--agent-exe", "/a/dyno-agent"])),
            Ok(Command::Install {
                user: HelperUser::Uid(501),
                agent_exe: PathBuf::from("/a/dyno-agent")
            })
        );
        // Order of the flags does not matter.
        assert!(parse(&args(&["install", "--agent-exe", "/a", "--user", "sid:S-1-5-21-1"])).is_ok());
    }

    #[test]
    fn anything_else_is_a_usage_error_that_says_how_to_call_it() {
        assert!(parse(&args(&[])).unwrap_err().contains("usage: dyno-helper"));
        for bad in [
            vec![],
            vec!["serve", "now"],
            vec!["install"],
            vec!["install", "--user", "uid:1"],
            vec!["install", "--user", "root", "--agent-exe", "/a"],
            vec!["install", "--user", "uid:1", "--user", "uid:2", "--agent-exe", "/a"],
            vec!["install", "--user"],
            vec!["install", "--shell", "x", "--user", "uid:1", "--agent-exe", "/a"],
            vec!["rm", "-rf"],
        ] {
            let e = parse(&args(&bad)).unwrap_err();
            assert!(!e.is_empty(), "{bad:?}");
        }
    }
}
