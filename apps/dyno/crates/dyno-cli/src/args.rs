//! The command line, parsed once into a typed [`Invocation`] (parse, don't validate).

/// What to sort `top` by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBy {
    /// CPU.
    Cpu,
    /// Memory.
    Memory,
}

/// A command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `dyno status`: how the computer is doing right now.
    Status,
    /// `dyno why-slow`: the diagnosis.
    WhySlow,
    /// `dyno top`: the busiest apps.
    Top {
        /// Sort key.
        by: TopBy,
        /// How many rows.
        count: usize,
    },
    /// `dyno alarms`: open alarms.
    Alarms {
        /// Include shelved ones.
        all: bool,
    },
    /// `dyno snapshot`: everything measured right now, as one document (nothing is stored).
    Snapshot,
    /// `dyno counters list [object]`.
    CountersList {
        /// An object to open (the objects when absent).
        object: Option<String>,
    },
    /// `dyno counters search <words>`.
    CountersSearch {
        /// The words.
        query: String,
    },
    /// `dyno counters get <path>...`.
    CountersGet {
        /// The counter paths.
        paths: Vec<String>,
    },
    /// `dyno --help`.
    Help,
    /// `dyno --version`.
    Version,
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// What to do.
    pub command: Command,
    /// Machine-readable output.
    pub json: bool,
}

/// The usage text.
pub const USAGE: &str = "dyno: look at how your computer is doing, from a terminal or a script\n\n\
usage: dyno <command> [options]\n\n\
commands:\n  \
status [--json]                       how the computer is doing right now\n  \
why-slow [--json]                     why it feels slow, in plain words\n  \
top [--by cpu|memory] [-n N] [--json] the busiest apps\n  \
alarms [--all] [--json]               alarms that need attention\n  \
snapshot                              everything measured now, as JSON (nothing is stored)\n  \
counters list [OBJECT] [--json]       the performance counters this computer offers\n  \
counters search WORDS... [--json]     find a counter by name\n  \
counters get PATH... [--json]         read counters now (PATH looks like \\Memory\\Free)\n\n\
Dyno only looks: no command changes anything on your computer.\n\
exit codes: 0 done, 1 failed, 2 wrong usage, 3 versions differ, 4 Dyno is not running\n";

/// Why a command line is not usable (the message says what to type instead).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError(pub String);

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

fn err<T>(msg: impl Into<String>) -> Result<T, UsageError> {
    Err(UsageError(msg.into()))
}

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Invocation, UsageError> {
    let mut json = false;
    let mut all = false;
    let mut by = TopBy::Cpu;
    let mut count = 10usize;
    let mut words: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--all" => all = true,
            "--by" => {
                by = match it.next().map(String::as_str) {
                    Some("cpu") => TopBy::Cpu,
                    Some("memory") => TopBy::Memory,
                    _ => return err("--by takes cpu or memory."),
                }
            }
            "-n" | "--count" => {
                count = match it.next().map(|n| n.parse::<usize>()) {
                    Some(Ok(n)) if (1..=100).contains(&n) => n,
                    _ => return err("-n takes a number from 1 to 100."),
                }
            }
            "-h" | "--help" | "help" => {
                return Ok(Invocation {
                    command: Command::Help,
                    json,
                })
            }
            "-V" | "--version" => {
                return Ok(Invocation {
                    command: Command::Version,
                    json,
                })
            }
            flag if flag.starts_with('-') && flag.len() > 1 => return err(format!("Unknown option {flag}. Try: dyno --help")),
            _ => words.push(a.clone()),
        }
    }
    let mut w = words.iter();
    let command = match w.next().map(String::as_str) {
        None => return err("Say what to show, for example: dyno status. Try: dyno --help"),
        Some("status") => Command::Status,
        Some("why-slow") => Command::WhySlow,
        Some("top") => Command::Top { by, count },
        Some("alarms") => Command::Alarms { all },
        Some("snapshot") => Command::Snapshot,
        Some("counters") => match w.next().map(String::as_str) {
            Some("list") => Command::CountersList {
                object: {
                    let rest: Vec<&str> = w.by_ref().map(String::as_str).collect();
                    (!rest.is_empty()).then(|| rest.join(" "))
                },
            },
            Some("search") => {
                let q = w.by_ref().map(String::as_str).collect::<Vec<_>>().join(" ");
                if q.trim().is_empty() {
                    return err("Type the words to look for, for example: dyno counters search disk queue");
                }
                Command::CountersSearch { query: q }
            }
            Some("get") => {
                let paths: Vec<String> = w.by_ref().cloned().collect();
                if paths.is_empty() {
                    return err("Give at least one counter path, for example: dyno counters get \"\\Memory\\Free\"");
                }
                Command::CountersGet { paths }
            }
            _ => return err("counters takes list, search or get. Try: dyno --help"),
        },
        Some(other) => return err(format!("Unknown command \"{other}\". Try: dyno --help")),
    };
    // Anything left over is a mistake worth saying: a typo must never silently change what a command does.
    let extra: Vec<&String> = w.collect();
    if !extra.is_empty() && !matches!(command, Command::CountersGet { .. } | Command::CountersSearch { .. }) {
        return err(format!("Unexpected \"{}\". Try: dyno --help", extra[0]));
    }
    Ok(Invocation { command, json })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Invocation, UsageError> {
        parse(&s.split_whitespace().map(String::from).collect::<Vec<_>>())
    }

    fn cmd(s: &str) -> Command {
        p(s).unwrap().command
    }

    #[test]
    fn commands_and_their_options() {
        assert_eq!(cmd("status"), Command::Status);
        assert!(p("status --json").unwrap().json);
        assert_eq!(cmd("why-slow"), Command::WhySlow);
        assert_eq!(cmd("top"), Command::Top { by: TopBy::Cpu, count: 10 });
        assert_eq!(
            cmd("top --by memory -n 3"),
            Command::Top {
                by: TopBy::Memory,
                count: 3
            }
        );
        assert_eq!(cmd("alarms --all"), Command::Alarms { all: true });
        assert_eq!(cmd("snapshot"), Command::Snapshot);
        assert_eq!(cmd("counters list"), Command::CountersList { object: None });
        assert_eq!(
            cmd("counters list Network Interface"),
            Command::CountersList {
                object: Some("Network Interface".into())
            }
        );
        assert_eq!(
            cmd("counters search disk queue"),
            Command::CountersSearch {
                query: "disk queue".into()
            }
        );
        assert_eq!(
            cmd("counters get \\Memory\\Free \\A\\B"),
            Command::CountersGet {
                paths: vec!["\\Memory\\Free".into(), "\\A\\B".into()]
            }
        );
    }

    #[test]
    fn help_and_version_win_over_a_missing_command() {
        assert_eq!(cmd("--help"), Command::Help);
        assert_eq!(cmd("status -h"), Command::Help);
        assert_eq!(cmd("--version"), Command::Version);
    }

    #[test]
    fn mistakes_are_reported_with_what_to_type() {
        for bad in [
            "",
            "frobnicate",
            "status --nope",
            "status extra",
            "end 4242",
            "cap 1 5",
            "undo abc",
            "top --by speed",
            "top -n 0",
            "top -n",
            "top --by",
            "counters",
            "counters search",
            "counters get",
            "counters nope",
        ] {
            let e = p(bad).unwrap_err();
            assert!(!e.0.is_empty(), "{bad}");
        }
        assert!(p("status --yes").is_err(), "there is nothing to confirm any more");
    }
}
