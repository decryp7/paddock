//! The command line, parsed once into a [`Command`] (parse, don't validate). Pure: nothing here touches the disk.

use paddock_cli::CliError;
use std::path::PathBuf;

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `new <name>`: a plugin project.
    New(NewArgs),
    /// `build`: the release program for one or more targets.
    Build(BuildArgs),
    /// `pack`: a `.paddockplugin`.
    Pack(PackArgs),
    /// `keygen --out <file>`: a publisher key pair.
    Keygen(KeygenArgs),
    /// `sign <package> --key <file>`.
    Sign(SignArgs),
    /// `verify <package>`.
    Verify(VerifyArgs),
    /// `run`: start the built plugin and print what it describes.
    Run(CheckArgs),
    /// `test`: start the built plugin and run the conformance checks.
    Test(CheckArgs),
}

/// `new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewArgs {
    /// The crate and program name.
    pub name: String,
    /// The folder the project folder is made in.
    pub parent: Option<PathBuf>,
    /// The plugin id (default `example.<name>`).
    pub id: Option<String>,
    /// The publisher shown to users (default `Example`).
    pub publisher: Option<String>,
    /// A checkout's `crates` folder: depend on the platform crates by path instead of the registry.
    pub api_path: Option<PathBuf>,
}

/// `build`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildArgs {
    /// The project folder (default: the current folder).
    pub dir: Option<PathBuf>,
    /// Targets to build for (default: this computer).
    pub targets: Vec<String>,
}

/// `pack`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackArgs {
    /// The project folder (default: the current folder).
    pub dir: Option<PathBuf>,
    /// Pack only these targets (default: every target the manifest lists).
    pub targets: Vec<String>,
    /// Programs that were built elsewhere: `(target, path)`.
    pub programs: Vec<(String, PathBuf)>,
    /// The secret key file to sign with.
    pub key: Option<PathBuf>,
    /// The package file (default: `<id>-<version>.paddockplugin` in the project's `dist` folder).
    pub out: Option<PathBuf>,
}

/// `keygen`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeygenArgs {
    /// The key file to create.
    pub out: PathBuf,
}

/// `sign`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignArgs {
    /// The package to sign.
    pub package: PathBuf,
    /// The secret key file.
    pub key: PathBuf,
    /// Write the signed package here (default: replace the package).
    pub out: Option<PathBuf>,
}

/// `verify`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyArgs {
    /// The package.
    pub package: PathBuf,
    /// Trusted public keys, as key text or a file holding it. With none, the signature is only checked, not trusted.
    pub trust: Vec<String>,
}

/// `run` and `test`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckArgs {
    /// The project folder (default: the current folder).
    pub dir: Option<PathBuf>,
    /// Start this program instead of the project's own build.
    pub program: Option<PathBuf>,
    /// Do not build first.
    pub no_build: bool,
}

/// The words `cargo-paddock-plugin` knows, for the help text and the error for a wrong one.
pub const COMMANDS: [&str; 8] = [
    "new", "build", "pack", "keygen", "sign", "verify", "run", "test",
];

/// What a command accepts.
struct Spec {
    /// Options that take a value (repeated ones are all kept).
    values: &'static [&'static str],
    /// Options without a value.
    flags: &'static [&'static str],
    /// The positional arguments, by name (all required).
    positional: &'static [&'static str],
}

/// The arguments of one command after the spec was applied.
struct Opts {
    values: Vec<(String, String)>,
    flags: Vec<String>,
    positional: Vec<String>,
}

impl Opts {
    fn parse(command: &str, spec: &Spec, args: &[String]) -> Result<Opts, CliError> {
        let mut o = Opts {
            values: Vec::new(),
            flags: Vec::new(),
            positional: Vec::new(),
        };
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if let Some(rest) = a.strip_prefix("--").filter(|r| !r.is_empty()) {
                let (name, inline) = match rest.split_once('=') {
                    Some((n, v)) => (n, Some(v.to_string())),
                    None => (rest, None),
                };
                if spec.flags.contains(&name) && inline.is_none() {
                    o.flags.push(name.to_string());
                } else if spec.values.contains(&name) {
                    let value = match inline.or_else(|| it.next().cloned()) {
                        Some(v) if !v.is_empty() => v,
                        _ => {
                            return Err(CliError::usage(format!("--{name} needs a value."))
                                .with_hint(format!("Write --{name} <value>.")));
                        }
                    };
                    o.values.push((name.to_string(), value));
                } else {
                    return Err(
                        CliError::usage(format!("{command} does not take --{name}.")).with_hint(
                            format!(
                                "Run `cargo paddock-plugin --help` to see the options of {command}."
                            ),
                        ),
                    );
                }
            } else if a.starts_with('-') && a.len() > 1 {
                return Err(
                    CliError::usage(format!("{command} does not take {a}.")).with_hint(
                        "Options are written --like-this; see `cargo paddock-plugin --help`.",
                    ),
                );
            } else {
                o.positional.push(a.clone());
            }
        }
        if o.positional.len() < spec.positional.len() {
            let missing = spec.positional[o.positional.len()];
            return Err(
                CliError::usage(format!("{command} needs the {missing}.")).with_hint(format!(
                    "Write `cargo paddock-plugin {command} <{missing}>`."
                )),
            );
        }
        if o.positional.len() > spec.positional.len() {
            return Err(CliError::usage(format!(
                "{command} got an unexpected argument \"{}\".",
                o.positional[spec.positional.len()]
            ))
            .with_hint("Paths with spaces need quotes."));
        }
        Ok(o)
    }

    fn one(&self, name: &str) -> Option<String> {
        self.values
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.one(name).map(PathBuf::from)
    }

    fn all(&self, name: &str) -> Vec<String> {
        self.values
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .collect()
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f == name)
    }
}

fn require(o: &Opts, command: &str, name: &str) -> Result<PathBuf, CliError> {
    o.path(name).ok_or_else(|| {
        CliError::usage(format!("{command} needs --{name}."))
            .with_hint(format!("Write --{name} <file>."))
    })
}

fn program(text: &str) -> Result<(String, PathBuf), CliError> {
    match text.split_once('=') {
        Some((t, p)) if !t.is_empty() && !p.is_empty() => Ok((t.to_string(), PathBuf::from(p))),
        _ => Err(
            CliError::usage(format!("--program \"{text}\" is not <target>=<file>.")).with_hint(
                "Write for example --program x86_64-unknown-linux-gnu=target/release/my-plugin.",
            ),
        ),
    }
}

/// Parses the arguments after `cargo paddock-plugin` (the common flags are already taken out).
pub fn parse(args: &[String]) -> Result<Command, CliError> {
    let Some((word, rest)) = args.split_first() else {
        return Err(CliError::usage("Say what to do.")
            .with_hint(format!("One of: {}. See --help.", COMMANDS.join(", "))));
    };
    let command = word.as_str();
    let check = |spec: &Spec| Opts::parse(command, spec, rest);
    match command {
        "new" => {
            let o = check(&Spec {
                values: &["dir", "id", "publisher", "api-path"],
                flags: &[],
                positional: &["name"],
            })?;
            Ok(Command::New(NewArgs {
                name: o.positional[0].clone(),
                parent: o.path("dir"),
                id: o.one("id"),
                publisher: o.one("publisher"),
                api_path: o.path("api-path"),
            }))
        }
        "build" => {
            let o = check(&Spec {
                values: &["dir", "target"],
                flags: &[],
                positional: &[],
            })?;
            Ok(Command::Build(BuildArgs {
                dir: o.path("dir"),
                targets: o.all("target"),
            }))
        }
        "pack" => {
            let o = check(&Spec {
                values: &["dir", "target", "program", "key", "out"],
                flags: &[],
                positional: &[],
            })?;
            Ok(Command::Pack(PackArgs {
                dir: o.path("dir"),
                targets: o.all("target"),
                programs: o
                    .all("program")
                    .iter()
                    .map(|p| program(p))
                    .collect::<Result<_, _>>()?,
                key: o.path("key"),
                out: o.path("out"),
            }))
        }
        "keygen" => {
            let o = check(&Spec {
                values: &["out"],
                flags: &[],
                positional: &[],
            })?;
            Ok(Command::Keygen(KeygenArgs {
                out: require(&o, command, "out")?,
            }))
        }
        "sign" => {
            let o = check(&Spec {
                values: &["key", "out"],
                flags: &[],
                positional: &["package"],
            })?;
            Ok(Command::Sign(SignArgs {
                package: PathBuf::from(&o.positional[0]),
                key: require(&o, command, "key")?,
                out: o.path("out"),
            }))
        }
        "verify" => {
            let o = check(&Spec {
                values: &["trust"],
                flags: &[],
                positional: &["package"],
            })?;
            Ok(Command::Verify(VerifyArgs {
                package: PathBuf::from(&o.positional[0]),
                trust: o.all("trust"),
            }))
        }
        "run" | "test" => {
            let o = check(&Spec {
                values: &["dir", "program"],
                flags: &["no-build"],
                positional: &[],
            })?;
            let args = CheckArgs {
                dir: o.path("dir"),
                program: o.path("program"),
                no_build: o.flag("no-build"),
            };
            Ok(if command == "run" {
                Command::Run(args)
            } else {
                Command::Test(args)
            })
        }
        other => Err(CliError::usage(format!("\"{other}\" is not a command."))
            .with_hint(format!("One of: {}. See --help.", COMMANDS.join(", ")))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, CliError> {
        parse(&args.iter().map(ToString::to_string).collect::<Vec<_>>())
    }

    fn usage(args: &[&str]) -> String {
        let e = p(args).unwrap_err();
        assert_eq!(e.exit, paddock_cli::Exit::Usage);
        assert!(!e.hint.is_empty(), "{e:?}");
        e.message
    }

    #[test]
    fn every_command_is_read_into_typed_values() {
        assert_eq!(
            p(&[
                "new",
                "hello",
                "--dir",
                "/w",
                "--id=com.x.hello",
                "--publisher",
                "Me",
                "--api-path",
                "/c"
            ])
            .unwrap(),
            Command::New(NewArgs {
                name: "hello".into(),
                parent: Some("/w".into()),
                id: Some("com.x.hello".into()),
                publisher: Some("Me".into()),
                api_path: Some("/c".into()),
            })
        );
        assert_eq!(
            p(&["build", "--target", "a-b-c", "--target=d-e-f"]).unwrap(),
            Command::Build(BuildArgs {
                dir: None,
                targets: vec!["a-b-c".into(), "d-e-f".into()]
            })
        );
        assert_eq!(
            p(&[
                "pack",
                "--dir",
                "d",
                "--program",
                "a-b-c=bin/x",
                "--key",
                "k",
                "--out",
                "o",
                "--target",
                "a-b-c"
            ])
            .unwrap(),
            Command::Pack(PackArgs {
                dir: Some("d".into()),
                targets: vec!["a-b-c".into()],
                programs: vec![("a-b-c".into(), "bin/x".into())],
                key: Some("k".into()),
                out: Some("o".into()),
            })
        );
        assert_eq!(
            p(&["keygen", "--out", "k"]).unwrap(),
            Command::Keygen(KeygenArgs { out: "k".into() })
        );
        assert_eq!(
            p(&["sign", "x.paddockplugin", "--key", "k"]).unwrap(),
            Command::Sign(SignArgs {
                package: "x.paddockplugin".into(),
                key: "k".into(),
                out: None
            })
        );
        assert_eq!(
            p(&["verify", "x", "--trust", "k1", "--trust", "k2"]).unwrap(),
            Command::Verify(VerifyArgs {
                package: "x".into(),
                trust: vec!["k1".into(), "k2".into()]
            })
        );
        let check = CheckArgs {
            dir: None,
            program: Some("prog".into()),
            no_build: true,
        };
        assert_eq!(
            p(&["run", "--program", "prog", "--no-build"]).unwrap(),
            Command::Run(check.clone())
        );
        assert_eq!(
            p(&["test", "--no-build", "--program", "prog"]).unwrap(),
            Command::Test(check)
        );
    }

    #[test]
    fn a_wrong_command_line_says_what_to_write() {
        assert!(usage(&[]).contains("Say what to do"));
        assert!(usage(&["frobnicate"]).contains("not a command"));
        assert!(usage(&["new"]).contains("needs the name"));
        assert!(usage(&["new", "a", "b"]).contains("unexpected argument \"b\""));
        assert!(usage(&["build", "--nope"]).contains("does not take --nope"));
        assert!(usage(&["build", "-x"]).contains("does not take -x"));
        assert!(usage(&["build", "--target"]).contains("needs a value"));
        assert!(usage(&["build", "--target="]).contains("needs a value"));
        assert!(usage(&["test", "--no-build=1"]).contains("does not take"));
        assert!(usage(&["keygen"]).contains("needs --out"));
        assert!(usage(&["sign", "p"]).contains("needs --key"));
        assert!(usage(&["pack", "--program", "nonsense"]).contains("not <target>=<file>"));
        assert!(usage(&["pack", "--program", "=x"]).contains("not <target>=<file>"));
        // A lone dash is a positional (a file name), not an option.
        assert_eq!(
            p(&["verify", "-"]).unwrap(),
            Command::Verify(VerifyArgs {
                package: "-".into(),
                trust: vec![]
            })
        );
    }
}
