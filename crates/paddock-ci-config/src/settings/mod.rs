//! The repository settings (ADR 0123, decision 82): one typed declaration of every `PADDOCK_*` variable and secret the tools read,
//! and the setup derived from them, returned as data.
//!
//! A pure core: the environment is passed in as [`paddock_env::Vars`] (`ProcessVars` in an adapter, `MapVars` or [`EnvMap`] in a
//! test) and nothing here reads the process, a file or the network. A secret is a [`paddock_secrets::Secret`] and never prints.
//! The adapters in `tools/paddock-dev` apply what this returns (`$GITHUB_ENV`, the machine's `.cargo/config.toml`, the S3 calls).
//!
//! * [`declared`] is the table: name, type, parser, default, secret flag, who sets it.
//! * [`Settings`] reads one declared variable through its parser.
//! * [`sccache`] is the derived setup: the compile cache's S3 level, its environment, the test cache's store address.

pub mod declared;
pub mod sccache;

pub use declared::*;

use paddock_env::Vars;
use paddock_secrets::Secret;
use std::collections::HashMap;
use std::ffi::OsString;

/// A map of strings as an environment (the shape most adapters in `paddock-dev` already hold). `paddock_env::MapVars` takes the
/// same data one `with` at a time.
#[derive(Debug, Clone, Copy)]
pub struct EnvMap<'a>(pub &'a HashMap<String, String>);

impl Vars for EnvMap<'_> {
    fn get(&self, key: &str) -> Option<OsString> {
        self.0.get(key).map(OsString::from)
    }
}

/// A lookup function as an environment (the shape the disk report and the places resolver already take).
#[derive(Clone, Copy)]
pub struct EnvFn<'a>(pub &'a dyn Fn(&str) -> Option<String>);

impl Vars for EnvFn<'_> {
    fn get(&self, key: &str) -> Option<OsString> {
        (self.0)(key).map(OsString::from)
    }
}

/// Environments read in order: the first one that holds the name wins (the process environment first, a credentials file's
/// settings after it).
pub struct Layered<'a>(pub &'a [&'a dyn Vars]);

impl Vars for Layered<'_> {
    fn get(&self, key: &str) -> Option<OsString> {
        self.0.iter().find_map(|v| v.get(key))
    }
}

/// Why a declared variable cannot be read: one sentence that names the variable and what it should hold, never the value of a
/// secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingError(pub String);

impl std::fmt::Display for SettingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SettingError {}

/// Reads declared variables from an environment through their parsers.
pub struct Settings<'v> {
    vars: &'v dyn Vars,
}

impl<'v> Settings<'v> {
    /// Settings over `vars`.
    pub fn new(vars: &'v dyn Vars) -> Self {
        Self { vars }
    }

    /// The value as text, trimmed; `None` when the variable is unset, empty, or not valid UTF-8. A secret is read with
    /// [`Settings::secret`], never as text.
    pub fn get(&self, d: &Decl) -> Option<String> {
        debug_assert!(
            !d.kind.is_secret(),
            "{} is a secret: read it with Settings::secret",
            d.name
        );
        self.vars
            .get(d.name)
            .and_then(|v| v.into_string().ok())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    /// [`Settings::get`], else the declaration's default.
    pub fn get_or_default(&self, d: &Decl) -> Option<String> {
        self.get(d).or_else(|| d.default.map(str::to_string))
    }

    /// The value as it is, not trimmed (a path with a space at its end is still that path); `None` when unset or empty.
    pub fn raw(&self, d: &Decl) -> Option<OsString> {
        self.vars.get(d.name).filter(|v| !v.is_empty())
    }

    /// A number: digits only. `None` when unset or not digits (the tools ignore a malformed number and use the default).
    pub fn number(&self, d: &Decl) -> Option<u64> {
        self.get(d)
            .filter(|v| v.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|v| v.parse().ok())
    }

    /// [`Settings::number`], else the declaration's default as a number.
    pub fn number_or_default(&self, d: &Decl) -> Option<u64> {
        self.number(d)
            .or_else(|| d.default.and_then(|v| v.parse().ok()))
    }

    /// A switch: on when the value is set, not empty and not `0` or `false` ([`Kind::Switch`]), or equals the one value a
    /// [`Kind::Exact`] declares. Anything else, unset included, is off.
    pub fn is_on(&self, d: &Decl) -> bool {
        let Some(v) = self.get(d) else { return false };
        match d.kind {
            Kind::Exact(on) => v == on,
            _ => !matches!(v.as_str(), "0" | "false"),
        }
    }

    /// A secret. `Ok(None)` when it is unset or empty; an error (without the value) when it is not a one-line credential.
    pub fn secret(&self, d: &Decl) -> Result<Option<Secret>, SettingError> {
        debug_assert!(d.kind.is_secret(), "{} is not a secret", d.name);
        let Some(v) = self.vars.get(d.name).and_then(|v| v.into_string().ok()) else {
            return Ok(None);
        };
        let v = v.trim();
        if v.is_empty() {
            return Ok(None);
        }
        Secret::new(v).map(Some).map_err(|_| {
            SettingError(format!(
                "the secret {} must be one line of 1 to {} characters",
                d.name,
                paddock_secrets::MAX_SECRET_LEN
            ))
        })
    }
}

#[cfg(test)]
mod tests;
