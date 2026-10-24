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

#[cfg(test)]
mod tests {
    use super::*;

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
