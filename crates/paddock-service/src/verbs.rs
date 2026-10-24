//! The words a background program understands as its first argument, the same in every app, because the installers
//! ([`crate::render::nsis_hooks`]), the app and the OS call them: `install`, `uninstall`, `status`, `stop`,
//! `print-unit`, `print-plist` and `--service` (the Windows Service Control Manager's entry).

/// A service verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Register with the OS and start (the installer, the app's Install or Repair, an administrator).
    Install,
    /// Stop and remove the registration; removing what is not there succeeds.
    Uninstall,
    /// Print whether it is registered and running.
    Status,
    /// Ask a running instance to stop (before its binary is replaced).
    Stop,
    /// Print the systemd unit `install` would write.
    PrintUnit,
    /// Print the launchd plist `install` would write.
    PrintPlist,
    /// Run under the Windows Service Control Manager.
    WindowsService,
}

impl Verb {
    /// Every verb.
    pub const ALL: [Verb; 7] = [
        Verb::Install,
        Verb::Uninstall,
        Verb::Status,
        Verb::Stop,
        Verb::PrintUnit,
        Verb::PrintPlist,
        Verb::WindowsService,
    ];

    /// The word on the command line.
    pub fn word(self) -> &'static str {
        match self {
            Verb::Install => "install",
            Verb::Uninstall => "uninstall",
            Verb::Status => "status",
            Verb::Stop => "stop",
            Verb::PrintUnit => "print-unit",
            Verb::PrintPlist => "print-plist",
            Verb::WindowsService => "--service",
        }
    }

    /// The verb in the arguments after the program name, if the first one is a verb (`pitwall-server --port 1 install`
    /// is not: options come after a verb, never before).
    pub fn parse<S: AsRef<str>>(args: &[S]) -> Option<Verb> {
        let first = args.first()?.as_ref();
        Verb::ALL.into_iter().find(|v| v.word() == first)
    }
}

impl Verb {
    /// What the verb does, in one line (the text of `--help`).
    pub fn description(self) -> &'static str {
        match self {
            Verb::Install => "register with the OS and start",
            Verb::Uninstall => "stop and remove the registration (succeeds when there is none)",
            Verb::Status => {
                "print whether it is registered and running (exit 3 when it is not running)"
            }
            Verb::Stop => "ask a running instance to stop",
            Verb::PrintUnit => "print the systemd unit `install` would write",
            Verb::PrintPlist => "print the launchd plist `install` would write",
            Verb::WindowsService => {
                "run under the Windows Service Control Manager (the OS starts this)"
            }
        }
    }

    /// Adds every verb to a program's `--help` in the family's style ([`paddock_cli::Help`]).
    pub fn add_to_help(help: paddock_cli::Help) -> paddock_cli::Help {
        Verb::ALL
            .into_iter()
            .fold(help, |h, v| h.command(v.word(), v.description()))
    }

    /// The exit code of `status`: `Ok` while it is running, `NotRunning` otherwise (scripts and the installer branch on it).
    pub fn status_exit(running: bool) -> paddock_cli::Exit {
        if running {
            paddock_cli::Exit::Ok
        } else {
            paddock_cli::Exit::NotRunning
        }
    }

    /// The error for a verb given arguments it does not take (`install --now`), naming the verb and the next step.
    pub fn reject_extra<S: AsRef<str>>(self, extra: &[S]) -> Result<(), paddock_cli::CliError> {
        match extra.first() {
            None => Ok(()),
            Some(a) => Err(paddock_cli::CliError::usage(format!(
                "`{}` takes no arguments, but got `{}`.",
                self.word(),
                a.as_ref()
            ))
            .with_hint(format!("Run it as just `{}`.", self.word()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbs_speak_the_cli_conventions() {
        let help =
            Verb::add_to_help(paddock_cli::Help::new("app-service", "background service")).render();
        for v in Verb::ALL {
            assert!(
                help.contains(v.word()) && help.contains(v.description()),
                "{help}"
            );
        }
        assert_eq!(Verb::status_exit(true).code(), 0);
        assert_eq!(Verb::status_exit(false).code(), 3);
        assert!(Verb::Install.reject_extra::<&str>(&[]).is_ok());
        let e = Verb::Install.reject_extra(&["--now"]).unwrap_err();
        assert_eq!(e.exit, paddock_cli::Exit::Usage);
        assert!(
            e.text().contains("`install` takes no arguments") && e.text().contains("--now"),
            "{e}"
        );
    }

    #[test]
    fn only_a_leading_verb_is_taken() {
        for v in Verb::ALL {
            assert_eq!(Verb::parse(&[v.word()]), Some(v));
        }
        assert_eq!(Verb::parse(&["--port", "1", "install"]), None);
        assert_eq!(Verb::parse::<&str>(&[]), None);
        assert_eq!(Verb::parse(&["service"]), None);
    }
}
