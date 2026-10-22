//! The service binary's command line, parsed into a typed command once.

/// What the binary was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Supervise the runners in the foreground (console, launchd, or development).
    Run,
    /// Entry point used by the Windows Service Control Manager.
    Service,
    /// Register the service with the OS (Windows: needs an elevated caller; the app asks Windows for it).
    Install,
    /// Remove the service from the OS.
    Uninstall,
    /// Print the effective configuration and its problems.
    CheckConfig,
    /// Print the version.
    Version,
    /// Print the usage.
    Help,
}

/// The usage text.
pub const USAGE: &str = "usage: pitcrew-service <run | service | install | uninstall | check-config | --version | --help>";

/// A command line that is not understood.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}\n{USAGE}")]
pub struct UsageError(String);

/// Parses the arguments after the program name.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Command, UsageError> {
    let args: Vec<String> = args.into_iter().collect();
    let cmd = match args.first().map(String::as_str) {
        None | Some("help" | "--help" | "-h") => Command::Help,
        Some("run") => Command::Run,
        Some("service") => Command::Service,
        Some("install") => Command::Install,
        Some("uninstall") => Command::Uninstall,
        Some("check-config") => Command::CheckConfig,
        Some("version" | "--version" | "-V") => Command::Version,
        Some(other) => return Err(UsageError(format!("unknown command {other:?}"))),
    };
    if args.len() > 1 {
        return Err(UsageError(format!("unexpected argument {:?}", args[1])));
    }
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, UsageError> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_every_command() {
        assert_eq!(p(&[]), Ok(Command::Help));
        assert_eq!(p(&["run"]), Ok(Command::Run));
        assert_eq!(p(&["service"]), Ok(Command::Service));
        assert_eq!(p(&["install"]), Ok(Command::Install));
        assert_eq!(p(&["uninstall"]), Ok(Command::Uninstall));
        assert_eq!(p(&["check-config"]), Ok(Command::CheckConfig));
        assert_eq!(p(&["--version"]), Ok(Command::Version));
        assert_eq!(p(&["-h"]), Ok(Command::Help));
    }

    #[test]
    fn refuses_unknown_commands_and_extra_arguments_with_the_usage() {
        let e = p(&["start"]).unwrap_err();
        assert!(e.to_string().contains("unknown command \"start\"") && e.to_string().contains("usage:"));
        assert!(p(&["run", "--now"]).is_err());
    }
}
