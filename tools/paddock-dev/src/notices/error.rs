//! What can go wrong in `dev notices`, typed; the command edge turns each into a message with the next step.

use paddock_cli::CliError;
use std::fmt;

/// A failure of `dev notices` (not a finding: a refused licence or a stale file is reported, not raised).
#[derive(Debug, PartialEq, Eq)]
pub enum NoticesError {
    /// The command line or an argument value is wrong.
    Usage(String),
    /// The app's `paddock.ci.json` cannot be used.
    Config {
        /// The file as given.
        file: String,
        /// What is wrong with it.
        why: String,
    },
    /// An input file cannot be read, parsed or is over its size limit.
    Read {
        /// The file.
        path: String,
        /// What is wrong with it.
        why: String,
    },
    /// The notices file cannot be written.
    Write {
        /// The file.
        path: String,
        /// What went wrong.
        why: String,
    },
    /// No `package-lock.json` covers the npm folder.
    NoLock(String),
    /// The root lock has no entry for the npm folder's workspace.
    NoWorkspace(String),
    /// Neither `--rust-json` nor a Rust manifest is known.
    NoRustSource,
    /// An external program (cargo about, cargo, npm) failed.
    Tool {
        /// The command line, without secrets (there are none here).
        command: String,
        /// Why it failed.
        why: String,
    },
}

impl fmt::Display for NoticesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NoticesError::Usage(m) => f.write_str(m),
            NoticesError::Config { file, why } => write!(f, "{file}: {why}"),
            NoticesError::Read { path, why } => write!(f, "{path}: {why}"),
            NoticesError::Write { path, why } => write!(f, "{path} cannot be written: {why}"),
            NoticesError::NoLock(path) => write!(f, "{path} not found"),
            NoticesError::NoWorkspace(ws) => write!(f, "the lock has no workspace {ws}"),
            NoticesError::NoRustSource => f.write_str("no Rust manifest: pass --manifest <Cargo.toml> or --rust-json <file>"),
            NoticesError::Tool { command, why } => write!(f, "{command} failed: {why}"),
        }
    }
}

impl std::error::Error for NoticesError {}

impl From<NoticesError> for CliError {
    fn from(e: NoticesError) -> CliError {
        let message = e.to_string();
        match e {
            NoticesError::Usage(_) => CliError::usage(message).with_hint("Run `./dev notices --help`."),
            NoticesError::Config { .. } => CliError::usage(message)
                .with_hint(r#"The app's paddock.ci.json needs a "notices" block: { "name": "<product>", "manifests": [...], "npm": "app", "out": [...] }."#),
            NoticesError::NoLock(_) => CliError::failure(message).with_hint("Run it from the repository, or pass --npm <the app's npm folder>."),
            NoticesError::NoWorkspace(_) => CliError::failure(message).with_hint("Check `npm` in the app's notices block names a workspace of the root package.json."),
            NoticesError::NoRustSource => CliError::usage(message),
            NoticesError::Read { .. } | NoticesError::Write { .. } => CliError::failure(message),
            NoticesError::Tool { .. } => CliError::failure(message)
                .with_hint("Install cargo-about 0.9.2 (`cargo install cargo-about --version 0.9.2 --locked --features cli`) and run `npm ci --ignore-scripts` at the root."),
        }
    }
}
