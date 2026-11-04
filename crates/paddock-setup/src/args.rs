//! The command line, parsed once into a typed [`Command`] (parse, do not validate: nothing downstream sees a string that
//! was never checked). Every option is `--name value` or a bare `--flag`; unknown, repeated or missing options are errors.

use semver::Version;

/// The longest argument accepted (a path is at most 32 767 UTF-16 units; this is a generous byte bound).
pub const MAX_ARG: usize = 4096;
/// The most image names `stop-all` takes.
pub const MAX_IMAGES: usize = 64;

/// Which `Run` values `clean-run` removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunScope {
    /// HKLM only, both registry views (install, and an uninstall that is an update).
    Machine,
    /// HKLM and the app's value in every loaded user hive (a real uninstall).
    MachineAndUsers,
}

/// One command of `paddock-setup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `guard --product <name> --main <exe> [--dir <install folder>] [--update] [--restart]`.
    Guard {
        /// `$INSTDIR`; when given and `check-dir` passes for it, the guard logs to `setup.log`.
        dir: Option<String>,
        /// The product name (the uninstall key's name).
        product: String,
        /// The main binary's file name.
        main: String,
        /// The installer was started with `/UPDATE`.
        update: bool,
        /// The installer was started with `/R`.
        restart: bool,
    },
    /// `check-dir --dir <install folder>`.
    CheckDir {
        /// `$INSTDIR`.
        dir: String,
    },
    /// `same-folder --a <folder> --b <folder>`: exit 0 when the same.
    SameFolder {
        /// First folder (`$EXEDIR`).
        a: String,
        /// Second folder (`$INSTDIR`).
        b: String,
    },
    /// `stop-all --dir <install folder> --images <a.exe,b.exe,...>`.
    StopAll {
        /// `$INSTDIR`.
        dir: String,
        /// File names of the programs to stop (`paddock-setup.exe` is added by the command).
        images: Vec<String>,
    },
    /// `clean-run --dir <install folder> --scope <machine|all>`.
    CleanRun {
        /// `$INSTDIR`.
        dir: String,
        /// Which values.
        scope: RunScope,
    },
    /// `clean-staging --dir <install folder>`.
    CleanStaging {
        /// `$INSTDIR`.
        dir: String,
    },
    /// `apply-update --file <download> --signature <signature file> --version <v> [--size <bytes>] --restart`.
    ApplyUpdate {
        /// The download (untrusted).
        file: String,
        /// The file with the base64 minisign signature (untrusted; read once, bounded).
        signature: String,
        /// The version the release information announced.
        version: Version,
        /// The size it announced; the copy never exceeds it (and never 1 GiB).
        size: Option<u64>,
    },
    /// `hold-installing`: creates and holds `Global\<identifier>.setup` until its parent (the installer) exits.
    HoldInstalling,
    /// `self-test`: prints the baked identifier, product and the update key's fingerprint (the ADR 0039 gate).
    SelfTest,
}

/// Why the command line is not a command.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UsageError {
    /// No command.
    #[error("no command given")]
    NoCommand,
    /// A command this program does not have.
    #[error("unknown command `{0}`")]
    UnknownCommand(String),
    /// An option the command does not take, or a stray value.
    #[error("unexpected argument `{0}`")]
    Unexpected(String),
    /// An option given twice.
    #[error("`{0}` is given twice")]
    Repeated(String),
    /// A required option is missing.
    #[error("`{0}` is required")]
    Missing(&'static str),
    /// An option has no value.
    #[error("`{0}` needs a value")]
    NoValue(String),
    /// A value is not valid for its option.
    #[error("`{option}` is not valid: {reason}")]
    Invalid {
        /// The option.
        option: &'static str,
        /// What is wrong.
        reason: &'static str,
    },
}

/// The options of one command: each is taken once by name; what is left at the end is an error.
struct Options {
    items: Vec<(String, Option<String>)>,
}

impl Options {
    /// Splits `--name value` pairs and bare `--flag`s. A token that does not start with `--` after a flag is its value.
    fn collect(rest: &[String]) -> Result<Options, UsageError> {
        for a in rest {
            if a.len() > MAX_ARG {
                return Err(UsageError::Invalid { option: "argument", reason: "too long" });
            }
        }
        let mut items: Vec<(String, Option<String>)> = Vec::new();
        let mut iter = rest.iter().peekable();
        while let Some(token) = iter.next() {
            if !token.starts_with("--") {
                return Err(UsageError::Unexpected(token.clone()));
            }
            if items.iter().any(|(n, _)| n == token) {
                return Err(UsageError::Repeated(token.clone()));
            }
            let value = match iter.peek() {
                Some(next) if !next.starts_with("--") => iter.next().cloned(),
                _ => None,
            };
            items.push((token.clone(), value));
        }
        Ok(Options { items })
    }

    fn take(&mut self, name: &str) -> Option<Option<String>> {
        let at = self.items.iter().position(|(n, _)| n == name)?;
        Some(self.items.remove(at).1)
    }

    fn required(&mut self, name: &'static str) -> Result<String, UsageError> {
        match self.take(name) {
            None => Err(UsageError::Missing(name)),
            Some(None) => Err(UsageError::NoValue(name.to_string())),
            Some(Some(v)) if v.is_empty() => Err(UsageError::NoValue(name.to_string())),
            Some(Some(v)) => Ok(v),
        }
    }

    fn optional(&mut self, name: &'static str) -> Result<Option<String>, UsageError> {
        match self.take(name) {
            None => Ok(None),
            Some(None) => Err(UsageError::NoValue(name.to_string())),
            Some(Some(v)) => Ok(Some(v)),
        }
    }

    fn flag(&mut self, name: &str) -> bool {
        matches!(self.take(name), Some(None))
    }

    fn finish(self) -> Result<(), UsageError> {
        match self.items.into_iter().next() {
            None => Ok(()),
            Some((name, _)) => Err(UsageError::Unexpected(name)),
        }
    }
}

/// Parses `args` (without the program name).
pub fn parse(args: &[String]) -> Result<Command, UsageError> {
    let (name, rest) = args.split_first().ok_or(UsageError::NoCommand)?;
    let mut o = Options::collect(rest)?;
    let command = match name.as_str() {
        "guard" => Command::Guard {
            product: o.required("--product")?,
            main: o.required("--main")?,
            dir: o.optional("--dir")?,
            update: o.flag("--update"),
            restart: o.flag("--restart"),
        },
        "check-dir" => Command::CheckDir { dir: o.required("--dir")? },
        "same-folder" => Command::SameFolder { a: o.required("--a")?, b: o.required("--b")? },
        "stop-all" => {
            let dir = o.required("--dir")?;
            let list = o.required("--images")?;
            let images: Vec<String> = list.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
            if images.is_empty() {
                return Err(UsageError::Missing("--images"));
            }
            if images.len() > MAX_IMAGES {
                return Err(UsageError::Invalid { option: "--images", reason: "too many programs" });
            }
            if images.iter().any(|i| i.contains(['\\', '/', ':'])) {
                return Err(UsageError::Invalid { option: "--images", reason: "file names only, no paths" });
            }
            Command::StopAll { dir, images }
        }
        "clean-run" => {
            let dir = o.required("--dir")?;
            let scope = match o.required("--scope")?.as_str() {
                "machine" => RunScope::Machine,
                "all" => RunScope::MachineAndUsers,
                _ => return Err(UsageError::Invalid { option: "--scope", reason: "expected `machine` or `all`" }),
            };
            Command::CleanRun { dir, scope }
        }
        "clean-staging" => Command::CleanStaging { dir: o.required("--dir")? },
        "apply-update" => {
            let file = o.required("--file")?;
            let signature = o.required("--signature")?;
            let version = Version::parse(&o.required("--version")?)
                .map_err(|_| UsageError::Invalid { option: "--version", reason: "not a semantic version" })?;
            let size = match o.optional("--size")? {
                None => None,
                Some(s) => Some(s.parse::<u64>().map_err(|_| UsageError::Invalid { option: "--size", reason: "not a number of bytes" })?),
            };
            if !o.flag("--restart") {
                return Err(UsageError::Missing("--restart"));
            }
            Command::ApplyUpdate { file, signature, version, size }
        }
        "hold-installing" => Command::HoldInstalling,
        "self-test" => Command::SelfTest,
        other => return Err(UsageError::UnknownCommand(other.to_string())),
    };
    o.finish()?;
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(line: &str) -> Result<Command, UsageError> {
        let v: Vec<String> = line.split('|').filter(|s| !s.is_empty()).map(String::from).collect();
        parse(&v)
    }

    #[test]
    fn guard_with_flags() {
        assert_eq!(
            p("guard|--product|Dyno|--main|dyno.exe|--update|--restart"),
            Ok(Command::Guard { dir: None, product: "Dyno".into(), main: "dyno.exe".into(), update: true, restart: true })
        );
        assert_eq!(
            p("guard|--product|Dyno|--main|dyno.exe"),
            Ok(Command::Guard { dir: None, product: "Dyno".into(), main: "dyno.exe".into(), update: false, restart: false })
        );
        assert_eq!(
            p("guard|--product|Dyno|--main|dyno.exe|--dir|D"),
            Ok(Command::Guard { dir: Some("D".into()), product: "Dyno".into(), main: "dyno.exe".into(), update: false, restart: false })
        );
    }

    #[test]
    fn simple_commands() {
        assert_eq!(p("check-dir|--dir|C:\\Program Files\\Dyno"), Ok(Command::CheckDir { dir: "C:\\Program Files\\Dyno".into() }));
        assert_eq!(p("same-folder|--a|X|--b|Y"), Ok(Command::SameFolder { a: "X".into(), b: "Y".into() }));
        assert_eq!(p("clean-staging|--dir|D"), Ok(Command::CleanStaging { dir: "D".into() }));
        assert_eq!(p("hold-installing"), Ok(Command::HoldInstalling));
        assert_eq!(p("self-test"), Ok(Command::SelfTest));
        assert_eq!(
            p("clean-run|--dir|D|--scope|all"),
            Ok(Command::CleanRun { dir: "D".into(), scope: RunScope::MachineAndUsers })
        );
        assert_eq!(p("clean-run|--dir|D|--scope|machine"), Ok(Command::CleanRun { dir: "D".into(), scope: RunScope::Machine }));
    }

    #[test]
    fn stop_all_images_split_and_bounded() {
        assert_eq!(
            p("stop-all|--dir|D|--images|dyno.exe,dyno-agent.exe, x.exe"),
            Ok(Command::StopAll { dir: "D".into(), images: vec!["dyno.exe".into(), "dyno-agent.exe".into(), "x.exe".into()] })
        );
        let many = (0..=MAX_IMAGES).map(|i| format!("a{i}.exe")).collect::<Vec<_>>().join(",");
        assert!(matches!(p(&format!("stop-all|--dir|D|--images|{many}")), Err(UsageError::Invalid { option: "--images", .. })));
        assert!(matches!(p("stop-all|--dir|D|--images|a\\b.exe"), Err(UsageError::Invalid { option: "--images", .. })));
        assert!(matches!(p("stop-all|--dir|D|--images|"), Err(UsageError::Missing("--images")) | Err(UsageError::NoValue(_))));
    }

    #[test]
    fn apply_update_parses_a_version_and_size() {
        let c = p("apply-update|--file|F|--signature|S|--version|1.2.3|--size|99|--restart").unwrap();
        assert_eq!(
            c,
            Command::ApplyUpdate { file: "F".into(), signature: "S".into(), version: Version::new(1, 2, 3), size: Some(99) }
        );
        assert!(matches!(
            p("apply-update|--file|F|--signature|S|--version|nonsense|--restart"),
            Err(UsageError::Invalid { option: "--version", .. })
        ));
        assert!(matches!(p("apply-update|--file|F|--signature|S|--restart"), Err(UsageError::Missing("--version"))));
        assert!(matches!(
            p("apply-update|--file|F|--signature|S|--version|1.0.0|--size|x|--restart"),
            Err(UsageError::Invalid { option: "--size", .. })
        ));
    }

    #[test]
    fn apply_update_requires_the_restart_marker() {
        assert!(matches!(p("apply-update|--file|F|--signature|S|--version|1.0.0"), Err(UsageError::Missing("--restart"))));
    }

    #[test]
    fn errors() {
        assert_eq!(p(""), Err(UsageError::NoCommand));
        assert_eq!(p("frobnicate"), Err(UsageError::UnknownCommand("frobnicate".into())));
        assert_eq!(p("check-dir"), Err(UsageError::Missing("--dir")));
        assert_eq!(p("check-dir|--dir"), Err(UsageError::NoValue("--dir".into())));
        assert_eq!(p("check-dir|--dir|A|--dir|B"), Err(UsageError::Repeated("--dir".into())));
        assert_eq!(p("check-dir|--dir|A|--oops"), Err(UsageError::Unexpected("--oops".into())));
        assert_eq!(p("check-dir|stray"), Err(UsageError::Unexpected("stray".into())));
        assert_eq!(p("clean-run|--dir|D|--scope|nope").unwrap_err(), UsageError::Invalid { option: "--scope", reason: "expected `machine` or `all`" });
    }

    #[test]
    fn long_and_empty_values_are_refused() {
        let long = "x".repeat(MAX_ARG + 1);
        assert!(matches!(parse(&["check-dir".into(), "--dir".into(), long]), Err(UsageError::Invalid { option: "argument", .. })));
        assert!(matches!(parse(&["check-dir".into(), "--dir".into(), String::new()]), Err(UsageError::NoValue(_))));
    }

    #[test]
    fn never_panics_on_odd_input() {
        for s in ["--", "-", "guard|--product", "\u{0}|\u{1f600}", "same-folder|--a|--b|--a"] {
            let _ = p(s);
        }
    }
}
