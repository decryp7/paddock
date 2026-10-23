//! Argument parsing: the command line becomes a typed [`Invocation`] once, at the edge (parse, don't validate).
//! Nothing here touches the environment, the file system or the network.
//!
//! Global flags may stand anywhere on the line. A key is never an argument: the admin key comes from
//! `PITWALL_ADMIN_KEY`, `--key-file <path>` or the OS credential store, and chat uses `PITWALL_API_KEY`.

use std::path::PathBuf;
use std::time::Duration;

/// The default time a management call may take before the CLI gives up.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long loading a model or a benchmark may take by default (a large model can take minutes).
pub const LONG_TIMEOUT: Duration = Duration::from_secs(600);
/// The longest `--timeout` accepted, in seconds.
pub const MAX_TIMEOUT_SECS: u64 = 24 * 60 * 60;

/// A command line the tool cannot act on. The message says what to fix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct UsageError(pub String);

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// `--help`, `help` or no arguments: print this text.
    Help(String),
    /// `--version`.
    Version,
    /// Run a command.
    Run(Invocation),
}

/// A command with the global options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// Print the server's JSON instead of text (read commands).
    pub json: bool,
    /// The server's port (`--port`); otherwise the discovery file, then 11435.
    pub port: Option<u16>,
    /// A file holding the admin key (`--key-file`).
    pub key_file: Option<PathBuf>,
    /// How long one call may take (`--timeout`); `None`: each command's own default.
    pub timeout: Option<Duration>,
    /// The command.
    pub command: Command,
}

impl Invocation {
    /// The time limit for a call: `--timeout` or the command's default.
    pub fn timeout_or(&self, default: Duration) -> Duration {
        self.timeout.unwrap_or(default)
    }
}

/// The subcommands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Server, memory, loaded models and queue.
    Status,
    /// Loaded models.
    Ps,
    /// Installed models.
    Models,
    /// Load a model into memory.
    Load {
        /// The model id.
        model: String,
    },
    /// Release a loaded model.
    Unload {
        /// The model id.
        model: String,
    },
    /// Download a model file.
    Pull {
        /// `owner/name`.
        repo: String,
        /// The file in the repository.
        file: String,
        /// Wait for the download and show progress (the default).
        wait: bool,
    },
    /// App keys.
    Keys(KeysCommand),
    /// Model presets and aliases.
    Presets(PresetsCommand),
    /// Chat with a model or alias.
    Chat {
        /// Model id or alias.
        model: String,
        /// The prompt; `None` reads standard input.
        prompt: Option<String>,
        /// A system prompt.
        system: Option<String>,
        /// Most tokens to generate.
        max_tokens: Option<u32>,
    },
    /// Measure a model's speed.
    Bench {
        /// The model id.
        model: String,
    },
}

/// `pitwall keys ...`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeysCommand {
    /// List apps and their keys.
    List,
    /// Create a key for an app (or a new app with `--new`).
    Create {
        /// The app's id or name (the new app's name with `new_app`).
        app: String,
        /// Create the app too.
        new_app: bool,
    },
    /// Revoke one key.
    Revoke {
        /// The app's id or name.
        app: String,
        /// The key id from `pitwall keys`.
        key: String,
    },
}

/// `pitwall presets ...`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetsCommand {
    /// List presets.
    List,
    /// One preset in detail.
    Show {
        /// The alias.
        alias: String,
    },
}

/// The help text.
pub fn help() -> String {
    "\
pitwall: control the local Pitwall server from a terminal or script

Usage: pitwall [options] <command> [arguments]

Commands:
  status                       Server, memory, loaded models and queue
  ps                           Models loaded in memory
  models                       Installed models
  load <model>                 Load a model into memory
  unload <model>               Release a loaded model
  pull <owner/repo/file>       Download a model file (progress on stderr; run again to resume)
  keys                         List apps and their keys
  keys create <app> [--new]    Create a key (--new: create the app too); the key is shown once
  keys revoke <app> <key-id>   Revoke a key
  presets                      List presets and aliases
  presets show <alias>         One preset in detail
  chat <model> [prompt]        Stream an answer to stdout (reads stdin without a prompt)
  bench <model>                Measure prompt and generation speed

Options:
  --json                       Print JSON (read commands)
  --port <n>                   The server's port (default: the discovery file, then 11435)
  --key-file <path>            A file holding the admin key
  --timeout <seconds>          Give up on a call after this long (default 30; load and bench wait up to 10 minutes)
  --system <text>              chat: a system prompt
  --max-tokens <n>             chat: most tokens to generate
  --no-wait                    pull: start the download and return
  -h, --help                   This text
  -V, --version                The version

Keys: the admin key comes from PITWALL_ADMIN_KEY, --key-file or the OS credential store (the same entry the
Pitwall window uses); chat uses a per-app key from PITWALL_API_KEY. Keys are never command-line arguments.

Exit codes: 0 ok, 1 error, 2 usage, 3 server not running, 4 refused (not enough memory, offline, busy).
"
    .to_string()
}

fn usage(m: impl Into<String>) -> UsageError {
    UsageError(m.into())
}

/// Splits `--flag=value`.
fn split_eq(a: &str) -> (&str, Option<&str>) {
    match a.split_once('=') {
        Some((k, v)) if k.starts_with("--") => (k, Some(v)),
        _ => (a, None),
    }
}

/// Parses the command line (without the program name).
pub fn parse(args: &[String]) -> Result<Parsed, UsageError> {
    let mut json = false;
    let (mut port, mut key_file, mut timeout) = (None, None, None);
    let (mut system, mut max_tokens, mut no_wait, mut new_app) = (None, None, false, false);
    let mut words: Vec<String> = Vec::new();
    let mut it = args.iter();
    let mut only_words = false;
    while let Some(raw) = it.next() {
        if only_words || !raw.starts_with('-') || raw == "-" {
            words.push(raw.clone());
            continue;
        }
        let (flag, inline) = split_eq(raw);
        let mut value = |name: &str| -> Result<String, UsageError> {
            match inline {
                Some(v) => Ok(v.to_string()),
                None => it.next().cloned().ok_or_else(|| usage(format!("{name} needs a value"))),
            }
        };
        match flag {
            "--" => only_words = true,
            "-h" | "--help" => return Ok(Parsed::Help(help())),
            "-V" | "--version" => return Ok(Parsed::Version),
            "--json" => json = true,
            "--no-wait" => no_wait = true,
            "--new" => new_app = true,
            "--port" => {
                let v = value("--port")?;
                port = Some(
                    v.parse::<u16>()
                        .ok()
                        .filter(|p| *p >= 1024)
                        .ok_or_else(|| usage(format!("--port must be a number from 1024 to 65535, not \"{v}\"")))?,
                );
            }
            "--key-file" => key_file = Some(PathBuf::from(value("--key-file")?)),
            "--timeout" => {
                let v = value("--timeout")?;
                let s = v
                    .parse::<u64>()
                    .ok()
                    .filter(|s| (1..=MAX_TIMEOUT_SECS).contains(s))
                    .ok_or_else(|| {
                        usage(format!(
                            "--timeout must be a whole number of seconds from 1 to {MAX_TIMEOUT_SECS}, not \"{v}\""
                        ))
                    })?;
                timeout = Some(Duration::from_secs(s));
            }
            "--system" => system = Some(value("--system")?),
            "--max-tokens" => {
                let v = value("--max-tokens")?;
                max_tokens = Some(
                    v.parse::<u32>()
                        .ok()
                        .filter(|n| *n >= 1)
                        .ok_or_else(|| usage(format!("--max-tokens must be a positive whole number, not \"{v}\"")))?,
                );
            }
            "--key" | "--admin-key" | "--api-key" => {
                return Err(usage(
                    "keys are never command-line arguments (they show up in process lists and shell history): set PITWALL_ADMIN_KEY, \
                     PITWALL_API_KEY or use --key-file <path>",
                ))
            }
            other => return Err(usage(format!("unknown option {other}. Run `pitwall --help`."))),
        }
    }
    let Some((cmd, rest)) = words.split_first() else {
        return Ok(Parsed::Help(help()));
    };
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    let command = match (cmd.as_str(), rest.as_slice()) {
        ("help", _) => return Ok(Parsed::Help(help())),
        ("status", []) => Command::Status,
        ("ps", []) => Command::Ps,
        ("models", []) => Command::Models,
        ("load", [m]) => Command::Load { model: (*m).into() },
        ("unload", [m]) => Command::Unload { model: (*m).into() },
        ("pull", [target]) => {
            let (repo, file) = split_pull(target)?;
            Command::Pull {
                repo,
                file,
                wait: !no_wait,
            }
        }
        ("keys", []) | ("keys", ["list"]) => Command::Keys(KeysCommand::List),
        ("keys", ["create", app]) => Command::Keys(KeysCommand::Create {
            app: (*app).into(),
            new_app,
        }),
        ("keys", ["revoke", app, key]) => Command::Keys(KeysCommand::Revoke {
            app: (*app).into(),
            key: (*key).into(),
        }),
        ("presets", []) | ("presets", ["list"]) => Command::Presets(PresetsCommand::List),
        ("presets", ["show", alias]) => Command::Presets(PresetsCommand::Show { alias: (*alias).into() }),
        ("chat", [model, prompt @ ..]) => Command::Chat {
            model: (*model).into(),
            prompt: (!prompt.is_empty()).then(|| prompt.join(" ")),
            system,
            max_tokens,
        },
        ("bench", [m]) => Command::Bench { model: (*m).into() },
        (c @ ("status" | "ps" | "models"), _) => return Err(usage(format!("`pitwall {c}` takes no arguments"))),
        (c @ ("load" | "unload" | "bench"), _) => return Err(usage(format!("`pitwall {c}` takes one model id: pitwall {c} <model>"))),
        ("pull", _) => return Err(usage("`pitwall pull` takes one argument: pitwall pull <owner>/<repo>/<file.gguf>")),
        ("keys", _) => {
            return Err(usage(
                "usage: pitwall keys | pitwall keys create <app> [--new] | pitwall keys revoke <app> <key-id>",
            ))
        }
        ("presets", _) => return Err(usage("usage: pitwall presets | pitwall presets show <alias>")),
        ("chat", []) => return Err(usage("`pitwall chat` needs a model or alias: pitwall chat <model> [prompt]")),
        (other, _) => return Err(usage(format!("unknown command \"{other}\". Run `pitwall --help`."))),
    };
    Ok(Parsed::Run(Invocation {
        json,
        port,
        key_file,
        timeout,
        command,
    }))
}

/// `owner/repo/path/to/file.gguf` into the repository and the file.
pub fn split_pull(target: &str) -> Result<(String, String), UsageError> {
    let parts: Vec<&str> = target.splitn(3, '/').collect();
    match parts.as_slice() {
        [owner, name, file] if !owner.is_empty() && !name.is_empty() && !file.is_empty() => Ok((format!("{owner}/{name}"), (*file).to_string())),
        _ => Err(usage(format!(
            "\"{target}\" is not <owner>/<repo>/<file>: for example pitwall pull unsloth/Qwen3.5-2B-GGUF/Qwen3.5-2B-Q4_K_M.gguf"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Parsed, UsageError> {
        let v: Vec<String> = s.split_whitespace().map(String::from).collect();
        parse(&v)
    }
    fn run(s: &str) -> Invocation {
        match p(s) {
            Ok(Parsed::Run(i)) => i,
            other => panic!("{other:?}"),
        }
    }
    fn err(s: &str) -> String {
        p(s).unwrap_err().0
    }

    #[test]
    fn help_and_version() {
        assert!(matches!(p(""), Ok(Parsed::Help(h)) if h.contains("Exit codes")));
        assert!(matches!(p("help"), Ok(Parsed::Help(_))));
        assert!(matches!(p("status --help"), Ok(Parsed::Help(_))));
        assert_eq!(p("-V"), Ok(Parsed::Version));
        assert_eq!(p("--version"), Ok(Parsed::Version));
    }

    #[test]
    fn read_commands_and_global_flags_anywhere() {
        let i = run("--json status");
        assert!(i.json && i.command == Command::Status && i.timeout.is_none() && i.port.is_none());
        let i = run("ps --port 12000 --timeout=5 --key-file /tmp/k --json");
        assert_eq!(i.command, Command::Ps);
        assert_eq!((i.port, i.timeout, i.json), (Some(12000), Some(Duration::from_secs(5)), true));
        assert_eq!(i.key_file, Some(PathBuf::from("/tmp/k")));
        assert_eq!(run("models").command, Command::Models);
        assert_eq!(i.timeout_or(DEFAULT_TIMEOUT), Duration::from_secs(5));
        assert_eq!(run("ps").timeout_or(LONG_TIMEOUT), LONG_TIMEOUT);
    }

    #[test]
    fn model_commands() {
        assert_eq!(run("load a-b").command, Command::Load { model: "a-b".into() });
        assert_eq!(run("unload a-b").command, Command::Unload { model: "a-b".into() });
        assert_eq!(run("bench a-b").command, Command::Bench { model: "a-b".into() });
        assert_eq!(
            run("pull o/r/dir/f.gguf --no-wait").command,
            Command::Pull {
                repo: "o/r".into(),
                file: "dir/f.gguf".into(),
                wait: false
            }
        );
        assert!(matches!(run("pull o/r/f.gguf").command, Command::Pull { wait: true, .. }));
    }

    #[test]
    fn keys_and_presets() {
        assert_eq!(run("keys").command, Command::Keys(KeysCommand::List));
        assert_eq!(run("keys list").command, Command::Keys(KeysCommand::List));
        assert_eq!(
            run("keys create web --new").command,
            Command::Keys(KeysCommand::Create {
                app: "web".into(),
                new_app: true
            })
        );
        assert_eq!(
            run("keys revoke web k1").command,
            Command::Keys(KeysCommand::Revoke {
                app: "web".into(),
                key: "k1".into()
            })
        );
        assert_eq!(run("presets").command, Command::Presets(PresetsCommand::List));
        assert_eq!(
            run("presets show fast").command,
            Command::Presets(PresetsCommand::Show { alias: "fast".into() })
        );
    }

    #[test]
    fn chat_prompt_and_options() {
        assert_eq!(
            run("chat fast hello there --system be-brief --max-tokens 20").command,
            Command::Chat {
                model: "fast".into(),
                prompt: Some("hello there".into()),
                system: Some("be-brief".into()),
                max_tokens: Some(20)
            }
        );
        assert!(matches!(run("chat fast").command, Command::Chat { prompt: None, .. }));
        // after `--` a leading dash is text
        assert!(matches!(run("chat fast -- -v").command, Command::Chat { prompt: Some(ref t), .. } if t == "-v"));
    }

    #[test]
    fn usage_errors_say_what_to_fix() {
        assert!(err("--bogus").contains("unknown option --bogus"));
        assert!(err("frobnicate").contains("unknown command"));
        assert!(err("status now").contains("no arguments"));
        assert!(err("load").contains("one model id"));
        assert!(err("pull nonsense").contains("<owner>/<repo>/<file>"));
        assert!(err("pull a/b").contains("<owner>/<repo>/<file>"));
        assert!(err("pull a//f").contains("<owner>/<repo>/<file>"));
        assert!(err("pull a/b/c d").contains("pitwall pull"));
        assert!(err("keys frob").contains("pitwall keys"));
        assert!(err("presets show").contains("presets show"));
        assert!(err("chat").contains("model or alias"));
        assert!(err("--port").contains("needs a value"));
        assert!(err("--port abc status").contains("1024"));
        assert!(err("--port 80 status").contains("1024"));
        assert!(err("--timeout 0 status").contains("--timeout"));
        assert!(err("--max-tokens x chat m").contains("--max-tokens"));
        assert!(err("--max-tokens 0 chat m").contains("--max-tokens"));
    }

    #[test]
    fn a_key_is_never_an_argument() {
        for f in ["--key", "--admin-key", "--api-key"] {
            let e = err(&format!("status {f} secret"));
            assert!(e.contains("PITWALL_ADMIN_KEY") && !e.contains("secret"));
        }
    }
}
