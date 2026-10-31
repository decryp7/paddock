//! The command line: `run [--high] [--no-gate] [--low] [--gate-load N] [--slots N] [--verbose] [--log-file F] <agent> [--] <cmd...>`
//! and `status`. Hand parsed to keep plock's exact shape (options first, the agent name, an optional `--`, the command).

/// What was asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Cli {
    /// `status`.
    Status,
    /// `run ...`.
    Run(RunArgs),
    /// `--help`.
    Help,
    /// `--version`.
    Version,
}

/// The arguments of `run`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunArgs {
    /// Skip the queue.
    pub high: bool,
    /// Skip the load gate.
    pub no_gate: bool,
    /// Run the command at lower priority.
    pub low: bool,
    /// Gate threshold override.
    pub gate_load: Option<f64>,
    /// Slot count override.
    pub slots: Option<u32>,
    /// Echo log events to stderr.
    pub verbose: bool,
    /// Log file override.
    pub log_file: Option<String>,
    /// Agent name.
    pub name: String,
    /// The command and its arguments.
    pub command: Vec<String>,
}

/// Usage text.
pub const USAGE: &str = "usage: heavy run [--high] [--no-gate] [--low] [--gate-load N] [--slots N] [--verbose] [--log-file FILE] <agent-name> -- <command...>\n       heavy status\nExit codes: the command's own; 64 usage, 75 lock not taken, 78 refused (home guard), 126/127 cannot run it, 129/130/143 stopped by a signal.\n";

/// Parses the arguments after the program name. Err is the message to print (exit 64).
pub fn parse(args: &[String]) -> Result<Cli, String> {
    let mut it = args.iter().peekable();
    match it.next().map(String::as_str) {
        Some("status") => Ok(Cli::Status),
        Some("--help" | "-h") => Ok(Cli::Help),
        Some("--version" | "-V") => Ok(Cli::Version),
        Some("run") => {
            let mut r = RunArgs::default();
            while let Some(a) = it.peek().map(|s| s.as_str()) {
                if !a.starts_with("--") || a == "--" {
                    break;
                }
                it.next();
                let mut value = |what: &str| it.next().cloned().ok_or_else(|| format!("{a} needs {what}"));
                match a {
                    "--high" => r.high = true,
                    "--no-gate" => r.no_gate = true,
                    "--low" => r.low = true,
                    "--verbose" => r.verbose = true,
                    "--gate-load" => {
                        let v = value("a number")?;
                        r.gate_load = Some(
                            v.parse::<f64>()
                                .ok()
                                .filter(|x| x.is_finite() && *x >= 0.0)
                                .ok_or(format!("--gate-load {v:?} is not a number >= 0"))?,
                        );
                    }
                    "--slots" => {
                        let v = value("a number")?;
                        r.slots = Some(
                            v.parse::<u32>()
                                .ok()
                                .filter(|n| (1..=64).contains(n))
                                .ok_or(format!("--slots {v:?} is not 1..64"))?,
                        );
                    }
                    "--log-file" => r.log_file = Some(value("a file")?),
                    _ => return Err(format!("unknown option {a}")),
                }
            }
            let name = it.next().cloned().unwrap_or_default();
            if it.peek().is_some_and(|s| s.as_str() == "--") {
                it.next();
            }
            r.name = name;
            r.command = it.cloned().collect();
            if r.name.is_empty() || r.command.is_empty() {
                return Err("missing agent name or command".into());
            }
            Ok(Cli::Run(r))
        }
        _ => Err("expected run or status".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Cli, String> {
        parse(&s.split_whitespace().map(String::from).collect::<Vec<_>>())
    }

    #[test]
    fn plock_shapes() {
        assert_eq!(p("status"), Ok(Cli::Status));
        let Ok(Cli::Run(r)) = p("run --high --no-gate A -- cargo test -p x") else {
            panic!("not run")
        };
        assert!(r.high && r.no_gate);
        assert_eq!((r.name.as_str(), r.command.join(" ")), ("A", "cargo test -p x".into()));
        let Ok(Cli::Run(r)) = p("run A cargo build") else {
            panic!("not run")
        };
        assert_eq!(r.command, ["cargo", "build"]);
    }

    #[test]
    fn errors() {
        assert!(p("run A").is_err());
        assert!(p("run --bogus A -- x").is_err());
        assert!(p("run --slots 0 A -- x").is_err());
        assert!(p("run --gate-load nope A -- x").is_err());
        assert!(p("").is_err());
        assert!(p("run --log-file").is_err());
    }

    #[test]
    fn new_options() {
        let Ok(Cli::Run(r)) = p("run --gate-load 12.5 --slots 3 --low --verbose --log-file /x/y A -- c") else {
            panic!("not run")
        };
        assert_eq!(
            (r.gate_load, r.slots, r.low, r.verbose, r.log_file.as_deref()),
            (Some(12.5), Some(3), true, true, Some("/x/y"))
        );
    }
}
