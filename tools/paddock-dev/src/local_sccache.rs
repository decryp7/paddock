//! A local sccache for agents' Rust builds (user decision 2026-11-16; docs/research/sccache-basedirs-2026-11-16.md).
//!
//! `RUSTC_WRAPPER` is set by this tool for the builds it starts (the push gates, `./dev gen check`, heavy's own build) and by
//! `eval "$(./dev sccache env)"` for an agent's own cargo calls; never in the user's shell. One server on sccache's default port,
//! started on demand, with sccache's local disk cache (`SCCACHE_DIR`, `SCCACHE_CACHE_SIZE` from the environment, which the
//! machine's shell profile already sets). No service. The backend is one setting, `PADDOCK_SCCACHE_BACKEND`: `auto` (the default: the
//! local disk first with the shared RustFS store behind it, like CI's `s3` backend, when the credentials file is there and the store
//! answers its health check; the local disk alone, with a notice or warning naming the cause, when not; [`choose`]), `local`
//! (the disk only) or `s3` (sccache's own S3 support alone, for a shared cache such as RustFS; [`S3`]). The server is started with a listed environment
//! ([`PASS`]) so no other remote-backend variable (`SCCACHE_REDIS*` and the like) ever reaches it, and a server already running on the
//! port with another cache or folder is left alone and not used. S3 credentials are read from an env file outside the repository
//! (or the caller's `AWS_*` variables), handed to the server only, never printed or logged: the output says `configured` or `missing`.
//!
//! The pinned release (the same version and checksums as CI, [`crate::ci_setup_rust`]) is installed under
//! `<tool cache>/tools/sccache/<version>/`, never into `~/.cargo`. Any failure (no pin for this OS, no network, a bad checksum, a server
//! that does not start or answers with another cache) turns the wrapper off with one `::warning::` and the build runs without it
//! (ER-27). CI is untouched: `CI` or `GITHUB_ACTIONS` in the environment turns this off, and so does `PADDOCK_SCCACHE=off`.
//! The build folder reaches cargo as `--config build.target-dir` ([`crate::cargo_target`]): the wrapper only pays when no
//! `CARGO_*` variable names the folder.

use crate::ci_setup_rust::{SCCACHE_VERSION, pin_for, sha256_file};
use crate::ci_tool::{NETWORK_ENV, Output, ToolCall, tail};
use crate::tool_cache;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What the sccache children get from the caller besides the base list: only the local-cache settings and logging.
const PASS: [&str; 1] = ["SCCACHE_SERVER_PORT"];

/// The cache size when the environment names none (the machine's profile sets 60G; the research note's start size is 30 GiB).
pub const DEFAULT_SIZE: &str = "30G";

/// The shared store's S3 API (`host:port`) when `PADDOCK_SCCACHE_S3_ENDPOINT` names none: the user's RustFS (docs/dev-setup.md, "Local sccache").
pub const DEFAULT_S3_ENDPOINT: &str = "172.16.1.220:15000";

/// The shared store's bucket when `PADDOCK_SCCACHE_S3_BUCKET` names none.
pub const DEFAULT_S3_BUCKET: &str = "sccache-paddock";

/// What `--show-adv-stats` prints as the cache location of a server started with the two-level chain (sccache 0.19.0, measured).
pub const MULTILEVEL_LOCATION: &str = "Multi-level (2 levels)";

/// The server's port: the caller's `SCCACHE_SERVER_PORT`, else sccache's default.
pub fn server_port(env: &HashMap<String, String>) -> String {
    env.get("SCCACHE_SERVER_PORT")
        .filter(|p| !p.is_empty())
        .cloned()
        .unwrap_or_else(|| "4226".into())
}

/// Whether a server listens on [`server_port`] on this machine (sccache listens on 127.0.0.1). A connect has no side effect, unlike
/// `--show-stats`, which starts a server when none runs (ER-45). Which server it is, ours or not, is decided by its stats.
pub fn listening(env: &HashMap<String, String>) -> bool {
    let Ok(port) = server_port(env).parse::<u16>() else {
        return false;
    };
    std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(300)).is_ok()
}

/// Whether [`prepare`] may download the pinned release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// `./dev sccache env`: install the pinned release when it is not there.
    OnDemand,
    /// A tool that starts builds (the push gates, `gen check`): use the installed release; never fetch anything in a gate's path.
    Never,
}

/// A working wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    /// The sccache binary.
    pub bin: PathBuf,
    /// The variables a build gets: `RUSTC_WRAPPER` and the cache settings the server runs with.
    pub vars: Vec<(String, String)>,
}

/// Whether the wrapper is on for this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Build with it.
    Ready(Ready),
    /// Build without it. `warn`: say so (a failure); not for a choice (CI, `PADDOCK_SCCACHE=off`, a caller's own wrapper).
    Off {
        /// Why, and what to do next.
        reason: String,
        /// A failure the person should see.
        warn: bool,
    },
}

impl Outcome {
    /// The wrapper, when on.
    pub fn ready(&self) -> Option<&Ready> {
        match self {
            Outcome::Ready(r) => Some(r),
            Outcome::Off { .. } => None,
        }
    }
}

fn off(reason: impl Into<String>, warn: bool) -> Outcome {
    Outcome::Off {
        reason: reason.into(),
        warn,
    }
}

/// A choice that turns the wrapper off before anything is touched: CI, `PADDOCK_SCCACHE=off`, a wrapper the caller set.
pub fn declined(env: &HashMap<String, String>) -> Option<String> {
    let set = |k: &str| env.get(k).is_some_and(|v| !v.is_empty());
    if env.get("PADDOCK_SCCACHE").is_some_and(|v| v == "off") {
        Some("PADDOCK_SCCACHE=off".into())
    } else if set("CI") || set("GITHUB_ACTIONS") {
        Some("this is a CI run (CI keeps its own setup-rust)".into())
    } else if set("RUSTC_WRAPPER") {
        let wrapper = &env["RUSTC_WRAPPER"];
        let path = Path::new(wrapper);
        // Our own shim (a nested `./dev` in a shell that loaded `sccache env`) is also named `sccache`, and is the fix, not the cause.
        let ours = path.parent().and_then(Path::file_name).is_some_and(|d| d == SHIM_DIR);
        let sccache = !ours && path.file_stem().is_some_and(|s| s.eq_ignore_ascii_case("sccache"));
        Some(if sccache {
            format!(
                "RUSTC_WRAPPER is already set ({wrapper}) to sccache itself: cc-rs wraps clang-cl in it and sccache 0.19 cannot probe clang-cl, so every `cargo xwin` lint that compiles C fails. Next: remove it from the shell profile and run `eval \"$(./dev sccache env)\"`, whose shim leaves clang-cl alone"
            )
        } else {
            format!("RUSTC_WRAPPER is already set ({wrapper})")
        })
    } else {
        None
    }
}

/// The cache folder: the caller's `SCCACHE_DIR` (the machine's profile), else `<tool cache>/sccache`.
pub fn cache_dir(env: &HashMap<String, String>) -> Result<String, String> {
    match env.get("SCCACHE_DIR").filter(|d| !d.is_empty()) {
        Some(d) => Ok(d.clone()),
        None => Ok(tool_cache::home(env)?.join("sccache").display().to_string()),
    }
}

/// The cache size: the caller's `SCCACHE_CACHE_SIZE`, else [`DEFAULT_SIZE`].
pub fn cache_size(env: &HashMap<String, String>) -> String {
    env.get("SCCACHE_CACHE_SIZE")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| DEFAULT_SIZE.into())
}

/// Where the pinned binary lives: `<tool cache>/tools/sccache/<version>/sccache`.
pub fn install_path(env: &HashMap<String, String>) -> Result<PathBuf, String> {
    Ok(tool_cache::home(env)?
        .join("tools")
        .join("sccache")
        .join(SCCACHE_VERSION)
        .join(tool_cache::exe("sccache")))
}

/// The S3 settings of the shared cache (all but the credentials are plain settings, safe to print). Variables:
/// `PADDOCK_SCCACHE_S3_BUCKET` (required), `PADDOCK_SCCACHE_S3_ENDPOINT` (`host:port`, required), `PADDOCK_SCCACHE_S3_REGION` (default
/// `auto`), `PADDOCK_SCCACHE_S3_SSL` (`true` or `false`, default `false`), `PADDOCK_SCCACHE_S3_PREFIX` (default `paddock`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3 {
    /// The bucket.
    pub bucket: String,
    /// `host:port` of the S3 API.
    pub endpoint: String,
    /// The region (`auto` for a custom endpoint).
    pub region: String,
    /// Whether the endpoint speaks TLS.
    pub ssl: bool,
    /// The key prefix inside the bucket.
    pub prefix: String,
}

/// Where the cache lives: one setting, `PADDOCK_SCCACHE_BACKEND`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// sccache's disk cache.
    Local {
        /// `SCCACHE_DIR`.
        dir: String,
        /// `SCCACHE_CACHE_SIZE`.
        size: String,
    },
    /// An S3-compatible object store (RustFS, MinIO), through sccache's own S3 support.
    S3(S3),
    /// The local disk first with the S3 store behind it (sccache's multi-level chain `disk,s3`), as CI's `s3` backend. The disk is
    /// a sibling folder of the plain one ([`crate::ci_setup_rust::s3_tier_of`]), so what S3 backfills never reaches the plain folder.
    Tiered {
        /// `SCCACHE_DIR` of the first level.
        dir: String,
        /// `SCCACHE_CACHE_SIZE`.
        size: String,
        /// The second level.
        s3: S3,
    },
}

impl Backend {
    /// The settings the server runs with (never a credential).
    pub fn vars(&self) -> Vec<(String, String)> {
        match self {
            Backend::Tiered { dir, size, s3 } => multilevel_settings(dir, size, &s3.bucket, &s3.endpoint, s3.ssl, &s3.prefix),
            Backend::Local { dir, size } => vec![("SCCACHE_DIR".into(), dir.clone()), ("SCCACHE_CACHE_SIZE".into(), size.clone())],
            Backend::S3(s) => vec![
                ("SCCACHE_BUCKET".into(), s.bucket.clone()),
                ("SCCACHE_ENDPOINT".into(), s.endpoint.clone()),
                ("SCCACHE_REGION".into(), s.region.clone()),
                ("SCCACHE_S3_USE_SSL".into(), s.ssl.to_string()),
                ("SCCACHE_S3_KEY_PREFIX".into(), s.prefix.clone()),
            ],
        }
    }

    /// The settings a build sees: the same, except that a tiered server's builds get the local disk only (no chain, no bucket, no key),
    /// so a server a later rustc starts itself serves from the disk instead of failing for lack of keys (as CI's `s3_build_settings`).
    pub fn build_vars(&self) -> Vec<(String, String)> {
        match self {
            Backend::Tiered { dir, size, .. } => {
                vec![("SCCACHE_DIR".into(), dir.clone()), ("SCCACHE_CACHE_SIZE".into(), size.clone())]
            }
            other => other.vars(),
        }
    }

    /// A line for a person: no credential in it.
    pub fn describe(&self) -> String {
        match self {
            Backend::Tiered { dir, size, s3 } => {
                format!(
                    "local disk {dir} ({size}), then S3 {} bucket {} prefix {}",
                    s3.endpoint, s3.bucket, s3.prefix
                )
            }
            Backend::Local { dir, size } => format!("local disk {dir} ({size})"),
            Backend::S3(s) => format!("S3 {} bucket {} prefix {}", s.endpoint, s.bucket, s.prefix),
        }
    }

    /// Whether `--show-stats` says the running server uses this very cache (a server with another backend, bucket, prefix or folder is
    /// somebody else's, and is left alone). The comparison is exact: the quoted folder as sccache prints it
    /// (`Local disk: "<dir>"`), and for S3 `s3, name: <bucket>, prefix: /<prefix>/`. A prefix of its own never matches another's, and
    /// the folder `/a/b` never matches `/a/b2` or `/x/a/b`. (sccache's stats do not print the S3 endpoint.)
    pub fn matches_stats(&self, stats: &str) -> bool {
        if let Backend::Tiered { dir, s3, .. } = self {
            // The location line alone names no setup (any chain says it): the disk folder and the second level are read from the level
            // lines, so another folder, bucket or prefix is not reported as ours (ER-33).
            return location(stats).as_deref() == Some(MULTILEVEL_LOCATION)
                && level(stats, "L0 (disk)").as_deref() == Some(format!("Local disk: \"{dir}\"").as_str())
                && level(stats, "L1 (s3)").as_deref()
                    == Some(format!("s3, name: {}, prefix: /{}/", s3.bucket, s3.prefix.trim_matches('/')).as_str());
        }
        let want = match self {
            Backend::Tiered { .. } => unreachable!("answered above"),
            Backend::Local { dir, .. } => format!("Local disk: \"{dir}\""),
            Backend::S3(s) => format!("s3, name: {}, prefix: /{}/", s.bucket, s.prefix.trim_matches('/')),
        };
        location(stats).as_deref() == Some(want.as_str())
    }
}

/// The `Cache location` line of `--show-adv-stats`.
fn location(stats: &str) -> Option<String> {
    stats
        .lines()
        .find_map(|l| l.strip_prefix("Cache location"))
        .map(|r| r.trim().to_string())
}

/// The description line of one level of a multi-level server (`  L0 (disk)   Local disk: "<dir>"`), without its label. The descriptions
/// follow the `Cache location` line; the per-level counter lines (`  L0 (disk) hits   0`) come before it and start with the same label,
/// so only the lines after the location line are read (sccache 0.19.0, measured), and a line whose rest is not separated from the
/// label by a gap is a counter, not a description.
fn level(stats: &str, label: &str) -> Option<String> {
    stats
        .lines()
        .skip_while(|l| !l.starts_with("Cache location"))
        .skip(1)
        .find_map(|l| l.trim_start().strip_prefix(label).filter(|r| r.starts_with("  ")))
        .map(|r| r.trim().to_string())
}

/// Whether the stats are those of a server of ours that runs the chain on `plain_dir`'s sibling folder (what an `auto` run that fell back
/// to the disk keeps while the store is down: its second level fails soft). Another disk folder, or a chain whose second level is not an
/// S3 bucket, is not ours.
fn chain_on(stats: &str, plain_dir: &str) -> bool {
    location(stats).as_deref() == Some(MULTILEVEL_LOCATION)
        && level(stats, "L0 (disk)").as_deref() == Some(format!("Local disk: \"{}\"", crate::ci_setup_rust::s3_tier_of(plain_dir)).as_str())
        && level(stats, "L1 (s3)").is_some_and(|l| l.starts_with("s3, name: "))
}

/// The backend the environment asks for, or the cause and next step.
pub fn backend(env: &HashMap<String, String>) -> Result<Backend, String> {
    let get = |k: &str| env.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
    match get("PADDOCK_SCCACHE_BACKEND").unwrap_or("local") {
        "local" => Ok(Backend::Local {
            dir: cache_dir(env)?,
            size: cache_size(env),
        }),
        "s3" => {
            let need = |k: &str| get(k).map(str::to_string).ok_or(format!("PADDOCK_SCCACHE_BACKEND=s3 needs {k}"));
            Ok(Backend::S3(S3 {
                bucket: need("PADDOCK_SCCACHE_S3_BUCKET")?,
                endpoint: need("PADDOCK_SCCACHE_S3_ENDPOINT")?,
                region: get("PADDOCK_SCCACHE_S3_REGION").unwrap_or("auto").to_string(),
                ssl: get("PADDOCK_SCCACHE_S3_SSL") == Some("true"),
                prefix: get("PADDOCK_SCCACHE_S3_PREFIX").unwrap_or("paddock").to_string(),
            }))
        }
        other => Err(format!("PADDOCK_SCCACHE_BACKEND is `{other}`; it is auto, local or s3")),
    }
}

/// Why the shared store is not used by an `auto` run, and how loudly: no credentials file is a choice (a notice); a file that is
/// wrong or a store that does not answer is a warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fallback {
    /// `::warning::` rather than `::notice::`.
    pub warn: bool,
    /// One sentence, never with a key in it.
    pub why: String,
}

/// The settings of the two-level chain the SERVER starts with: the local disk, then the S3 store, a failed S3 write never fatal
/// (`l0`; sccache v0.19.0 docs/MultiLevel.md). One definition for CI's `s3` backend ([`crate::ci_setup_rust::s3_server_settings`]) and this one.
pub(crate) fn multilevel_settings(dir: &str, size: &str, bucket: &str, endpoint: &str, ssl: bool, prefix: &str) -> Vec<(String, String)> {
    [
        ("SCCACHE_DIR", dir),
        ("SCCACHE_CACHE_SIZE", size),
        ("SCCACHE_MULTILEVEL_CHAIN", "disk,s3"),
        ("SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY", "l0"),
        ("SCCACHE_BUCKET", bucket),
        ("SCCACHE_ENDPOINT", endpoint),
        ("SCCACHE_REGION", "auto"),
        ("SCCACHE_S3_USE_SSL", if ssl { "true" } else { "false" }),
        ("SCCACHE_S3_KEY_PREFIX", prefix),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// The health check of the store before a run uses it: CI's own ([`crate::ci_setup_rust::s3_health_call`]), so the two never differ. It
/// is bounded in full (connect, DNS included, and total time) by curl's limits.
pub fn s3_alive(s3: &S3, env: &HashMap<String, String>) -> Result<(), String> {
    let url = crate::ci_setup_rust::s3_health_url(s3.ssl, &s3.endpoint);
    let ran = crate::ci_setup_rust::s3_health_call(&url).run(env, &mut std::io::sink());
    crate::ci_setup_rust::s3_health_verdict(&url, ran)
}

/// The backend this run uses. `auto` (the default) is the local disk first with the shared store behind it ([`Backend::Tiered`])
/// when the credentials are there and `probe` says the store answers, else the local disk with the [`Fallback`] cause; `local` and
/// `s3` are as asked ([`backend`]). The endpoint and bucket are `PADDOCK_SCCACHE_S3_ENDPOINT` and `PADDOCK_SCCACHE_S3_BUCKET` (the
/// same names CI's variables carry), else [`DEFAULT_S3_ENDPOINT`] and [`DEFAULT_S3_BUCKET`].
pub fn choose(env: &HashMap<String, String>, probe: &dyn Fn(&S3) -> Result<(), String>) -> Result<(Backend, Option<Fallback>), String> {
    let get = |k: &str| env.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
    if get("PADDOCK_SCCACHE_BACKEND").is_some_and(|b| b != "auto") {
        return backend(env).map(|b| (b, None));
    }
    let (dir, size) = (cache_dir(env)?, cache_size(env));
    let local = |why: String, warn: bool| {
        Ok((
            Backend::Local {
                dir: dir.clone(),
                size: size.clone(),
            },
            Some(Fallback { warn, why }),
        ))
    };
    match read_creds(env) {
        Ok(c) if c.status() == "configured" => {}
        Ok(_) => {
            return local(
                "no S3 credentials (AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY in the environment or in ~/.config/paddock/sccache-s3.env); the shared cache is not set up on this machine".into(),
                false,
            );
        }
        Err(e) => return local(e, true),
    }
    // The environment first, then the credentials file, then the defaults.
    let from_file = file_settings(env);
    let setting = |k: &str| get(k).or_else(|| from_file.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str()));
    let s3 = S3 {
        bucket: setting("PADDOCK_SCCACHE_S3_BUCKET").unwrap_or(DEFAULT_S3_BUCKET).to_string(),
        endpoint: setting("PADDOCK_SCCACHE_S3_ENDPOINT")
            .unwrap_or(DEFAULT_S3_ENDPOINT)
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string(),
        region: "auto".into(),
        ssl: false,
        prefix: get("PADDOCK_SCCACHE_S3_PREFIX").unwrap_or("paddock").to_string(),
    };
    if let Err(e) = probe(&s3) {
        return local(e, true);
    }
    Ok((
        Backend::Tiered {
            dir: crate::ci_setup_rust::s3_tier_of(&dir),
            size,
            s3,
        },
        None,
    ))
}

/// The S3 credentials: where they are read from, and whether they are there. A value never appears in this type's `Debug` output
/// or in any message.
#[derive(Clone, PartialEq, Eq)]
pub struct Creds {
    pairs: Vec<(String, String)>,
    /// The key file was used while the caller's environment also held `AWS_*` variables, which are ignored for sccache.
    env_ignored: bool,
}

impl std::fmt::Debug for Creds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Creds({})", self.status())
    }
}

impl Creds {
    /// Whether the caller's own `AWS_*` variables were left out because the key file's keys were used (said once by [`prepare`]).
    pub fn env_ignored(&self) -> bool {
        self.env_ignored
    }

    /// `configured` or `missing`: the only thing ever printed about them.
    pub fn status(&self) -> &'static str {
        if self.pairs.iter().any(|(k, v)| k == "AWS_ACCESS_KEY_ID" && !v.is_empty())
            && self.pairs.iter().any(|(k, v)| k == "AWS_SECRET_ACCESS_KEY" && !v.is_empty())
        {
            "configured"
        } else {
            "missing"
        }
    }

    /// The variables for the server, and the values to mask in any logged command.
    pub fn pairs(&self) -> &[(String, String)] {
        &self.pairs
    }
}

/// The credentials file, outside the repository: `PADDOCK_SCCACHE_ENV_FILE`, else `$XDG_CONFIG_HOME/paddock/sccache-s3.env`, else
/// `~/.config/paddock/sccache-s3.env` (mode 600; two lines, `AWS_ACCESS_KEY_ID=...` and `AWS_SECRET_ACCESS_KEY=...`).
pub fn env_file(env: &HashMap<String, String>) -> Result<PathBuf, String> {
    let get = |k: &str| env.get(k).filter(|v| !v.is_empty());
    if let Some(f) = get("PADDOCK_SCCACHE_ENV_FILE") {
        return Ok(PathBuf::from(f));
    }
    let base = match (get("XDG_CONFIG_HOME"), get("HOME")) {
        (Some(x), _) => PathBuf::from(x),
        (None, Some(h)) => Path::new(h).join(".config"),
        _ => return Err("neither PADDOCK_SCCACHE_ENV_FILE, XDG_CONFIG_HOME nor HOME is set, so there is no credentials file".into()),
    };
    Ok(base.join("paddock").join("sccache-s3.env"))
}

/// Parses `NAME=value` lines (comments and blank lines skipped, an optional `export ` and quotes dropped), keeping only the two
/// AWS names: a file with other lines gives the server nothing else.
pub fn parse_env_file(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim().strip_prefix("export ").unwrap_or(l.trim());
            let (k, v) = l.split_once('=')?;
            let (k, v) = (k.trim(), v.trim().trim_matches(['"', '\'']));
            (matches!(k, "AWS_ACCESS_KEY_ID" | "AWS_SECRET_ACCESS_KEY") && !v.is_empty()).then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// The store's own settings in the credentials file: the optional lines `PADDOCK_SCCACHE_S3_ENDPOINT` and `PADDOCK_SCCACHE_S3_BUCKET`
/// (not secret; any other line is left to [`parse_env_file`] or ignored). They rank after the environment and before the defaults.
pub fn parse_env_settings(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim().strip_prefix("export ").unwrap_or(l.trim());
            let (k, v) = l.split_once('=')?;
            let (k, v) = (k.trim(), v.trim().trim_matches(['"', '\'']));
            (matches!(k, "PADDOCK_SCCACHE_S3_ENDPOINT" | "PADDOCK_SCCACHE_S3_BUCKET") && !v.is_empty())
                .then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// [`parse_env_settings`] of the credentials file; none when there is no file or it cannot be read (the credentials check says why).
fn file_settings(env: &HashMap<String, String>) -> Vec<(String, String)> {
    env_file(env)
        .ok()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|t| parse_env_settings(&t))
        .unwrap_or_default()
}

/// Reads the credentials: the key file first (the machine's declared key: `~/.config/paddock/sccache-s3.env`; a file that group or others
/// can read is refused, it holds a secret), and only when it holds none the caller's `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` (for
/// example exported from the macOS keychain: `security find-generic-password -s paddock-sccache-s3 -w`). A stray `AWS_*` in the
/// caller's shell, another project's login, never outranks the file ([`Creds::env_ignored`]).
pub fn read_creds(env: &HashMap<String, String>) -> Result<Creds, String> {
    let from_env: Vec<(String, String)> = ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"]
        .iter()
        .filter_map(|k| env.get(*k).filter(|v| !v.is_empty()).map(|v| (k.to_string(), v.clone())))
        .collect();
    let from_env = Creds {
        pairs: from_env,
        env_ignored: false,
    };
    let file = env_file(env)?;
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(from_env),
        Err(e) => return Err(format!("cannot read the credentials file {}: {e}", file.display())),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).map_err(|e| e.to_string())?.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(format!(
                "the credentials file {} is readable by others (mode {:o}); run `chmod 600` on it",
                file.display(),
                mode & 0o777
            ));
        }
    }
    let from_file = Creds {
        pairs: parse_env_file(&text),
        env_ignored: !from_env.pairs.is_empty(),
    };
    Ok(if from_file.status() == "configured" { from_file } else { from_env })
}

/// The folder the server runs in, uses as its temporary folder and writes its error log in: `<tool cache>/sccache-run`, tool-owned and
/// never deleted by a job. A server that inherits the starter's `TMPDIR` (a job's or an agent's scratch folder) outlives it, and every
/// later build fails once that folder is gone (measured in review of 872ab6743). It is short on purpose: macOS caps a socket path at
/// 104 bytes and a long `PADDOCK_DEV_HOME` below the old `tools/sccache/<version>/run` could pass it.
pub fn run_folder(env: &HashMap<String, String>) -> Result<PathBuf, String> {
    Ok(tool_cache::home(env)?.join("sccache-run"))
}

/// Where the server writes its errors (`SCCACHE_ERROR_LOG`), named in every warning (ER-27).
pub fn error_log(run: &Path) -> PathBuf {
    run.join("sccache.log")
}

/// The longest run folder path that leaves room for a socket name under macOS's 104-byte cap.
const SHORT_RUN_PATH: usize = 80;

/// One server's settings, and the calls that start, ask and stop it.
struct Server<'a> {
    bin: &'a Path,
    run: &'a Path,
    backend: &'a Backend,
    creds: Option<&'a Creds>,
    /// `SCCACHE_LOG`: the caller's, else `warn`.
    log_level: String,
    /// [`basedir_vars`] for this checkout: the server reads them once, when it starts.
    basedirs: Vec<(String, String)>,
}

impl Server<'_> {
    fn call(&self, arg: &str) -> ToolCall {
        let run = self.run.to_string_lossy();
        let c = ToolCall::new(self.bin.to_string_lossy())
            .args([arg])
            .pass(PASS)
            .cwd(self.run.to_path_buf())
            .without("TMPDIR")
            .without("TMP")
            .without("TEMP")
            .env("TMPDIR", run.as_ref())
            .env("TMP", run.as_ref())
            .env("TEMP", run.as_ref())
            .env("SCCACHE_ERROR_LOG", error_log(self.run).to_string_lossy())
            .env("SCCACHE_LOG", self.log_level.as_str())
            .timeout(Duration::from_secs(60));
        let creds = self.creds.map(Creds::pairs).unwrap_or_default();
        server_env(
            c,
            &[self.backend.vars(), self.basedirs.clone()].concat(),
            true,
            matches!(self.backend, Backend::S3(_) | Backend::Tiered { .. }),
            creds,
        )
    }
}

/// The two settings that keep the server we start the only one: it never exits on its own (a build that finds none would start one
/// with the build's environment, and for S3 without the keys, and that one would replace ours and fail), and with S3 there is no
/// instance-metadata lookup (with no keys it hangs for about 75 s before it fails). Builds get these too (`ready_vars`, setup-rust's
/// `$GITHUB_ENV`), so a server a build starts anyway behaves the same way. Never a credential.
pub(crate) fn survival_vars(s3: bool) -> Vec<(String, String)> {
    let mut v = vec![("SCCACHE_IDLE_TIMEOUT".to_string(), "0".to_string())];
    if s3 {
        v.push(("AWS_EC2_METADATA_DISABLED".to_string(), "true".to_string()));
    }
    v
}

/// The one place a sccache server gets its environment (ER-22, rule 55: the agents' local server and CI's setup-rust both start
/// through it): `settings` (what the server runs with), [`survival_vars`] when `survive`, and the credentials in `creds`, which only
/// the server gets and the logged command masks. `s3` turns the metadata lookup off.
pub(crate) fn server_env(mut c: ToolCall, settings: &[(String, String)], survive: bool, s3: bool, creds: &[(String, String)]) -> ToolCall {
    c = c.env("SCCACHE_IGNORE_SERVER_IO_ERROR", "1");
    for (k, v) in settings {
        c = c.env(k.as_str(), v.as_str());
    }
    if survive {
        for (k, v) in survival_vars(s3) {
            c = c.env(k.as_str(), v.as_str());
        }
    }
    for (k, v) in creds {
        c = c.env(k.as_str(), v.as_str()).secret(v.as_str());
    }
    c
}

/// The names of the variables whose values a Rust crate bakes into its outputs with `env!` and that sccache must hash verbatim, so a cache
/// entry is shared across checkouts only for crates that do not embed the checkout's path (sccache docs/Rust.md at v0.20.1).
pub(crate) const VERBATIM_ENV_DEPS: &str = "CARGO_MANIFEST_DIR,CARGO_MANIFEST_PATH";

/// `SCCACHE_BASEDIRS` and `SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS` for the given checkout and build-folder roots (ER-22: the one place CI's
/// setup-rust and the agents' local server both get them). sccache 0.20 strips the longest matching base from every path in a Rust
/// compile's key, so the same source at another absolute path (a CI slot, a random work folder, a git worktree) hits the same entry.
/// Separator: `;` on Windows hosts, `:` elsewhere (docs/Configuration.md at v0.20.1); only absolute paths (a relative one stops the
/// server from starting), each once, in the given order. No usable root gives no variables. `CARGO_MANIFEST_DIR` and
/// `CARGO_MANIFEST_PATH` stay verbatim in the key: 31 crates read them with `env!`, and a cached binary must not carry another
/// checkout's path.
pub(crate) fn basedir_vars(roots: &[String], windows: bool) -> Vec<(String, String)> {
    let absolute = |r: &str| {
        if windows {
            let b = r.as_bytes();
            b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
        } else {
            r.starts_with('/')
        }
    };
    let mut seen: Vec<&str> = Vec::new();
    for r in roots.iter().map(|r| r.trim()).filter(|r| !r.is_empty() && absolute(r)) {
        let r = if r.len() > 1 { r.trim_end_matches(['/', '\\']) } else { r };
        if !seen.contains(&r) {
            seen.push(r);
        }
    }
    if seen.is_empty() {
        return Vec::new();
    }
    let sep = if windows { ";" } else { ":" };
    vec![
        ("SCCACHE_BASEDIRS".to_string(), seen.join(sep)),
        ("SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS".to_string(), VERBATIM_ENV_DEPS.to_string()),
    ]
}

/// The checkouts a local server should treat as one: the worktree `cwd` is in (the nearest folder with a `.git`) first, then its
/// sibling worktrees (the folders next to it with a `.git`; agents' worktrees all live under one root, docs/dev-setup.md). One server
/// serves every worktree and reads its base folders only when it starts, so a worktree made later is not in the list until the
/// server restarts; it then builds correctly, only without sharing its own crates' entries.
pub(crate) fn worktree_roots(cwd: &Path) -> Vec<String> {
    let Some(top) = cwd.ancestors().find(|a| a.join(".git").exists()) else {
        return Vec::new();
    };
    let mut roots = vec![top.display().to_string()];
    if let Some(parent) = top.parent()
        && let Ok(rd) = std::fs::read_dir(parent)
    {
        let mut sibs: Vec<String> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p != top && p.join(".git").exists())
            .map(|p| p.display().to_string())
            .collect();
        sibs.sort();
        roots.extend(sibs);
    }
    roots
}

/// The `CARGO_*` variables CI's jobs have that sccache hashes into every Rust key (sccache 0.20.1 `generate_hash_key`: all of them but a
/// few): `CARGO_INCREMENTAL=0` (setup-rust, [`crate::ci_setup_rust::env_lines`]) and `CARGO_TERM_COLOR=always` (the workflows' job
/// `env`). The shim exports them for a rustc compile, so a local key equals CI's and the two share entries; cargo's own environment
/// is left alone (no colour codes in piped output, incremental builds stay). docs/benchmarks.md, "Local keys against CI's".
pub(crate) const CI_KEY_ENV: [(&str, &str); 2] = [("CARGO_INCREMENTAL", "0"), ("CARGO_TERM_COLOR", "always")];

/// The folder beside the sccache binary that holds the shim, so the shim can be called `sccache`.
const SHIM_DIR: &str = "shim";

/// The shim's text: a program named `sccache` that runs the real one for everything except an MSVC-target C compile, which it runs
/// directly (`sccache clang-cl ...` becomes `clang-cl ...`).
///
/// Policy (ER-47, the one place that decides it): sccache 0.19 cannot probe `clang-cl` (it reads the `/Users/...` paths of the probe as
/// `/U` options: "Compiler not supported"), and cc-rs puts `RUSTC_WRAPPER` in front of the C compiler whenever its file name is
/// `sccache` (also `cachepot`, `buildcache`, `kache`). cc-rs has no switch for one target and cargo gives every build script
/// `RUSTC_WRAPPER`, so `RUSTC_WRAPPER=sccache` failed every `cargo xwin` lint that compiles C (ring). The shim keeps the name cc-rs
/// wraps, so native C (libsqlite3-sys, aws-lc-sys, zstd-sys, ring) stays cached on macOS and Linux, and sends only the cross
/// `clang-cl` (and `cl`, or any compiler with `--target=...-windows-msvc`) past sccache. A `rustc` call always goes to sccache. The
/// cost: the Windows cross C compiles are not cached. The documented cc-rs range is 1.8 to 1.11 ([`CC_RANGE`]; a test reads every
/// lockfile).
fn shim_text(bin: &Path) -> String {
    let real = bin.display().to_string().replace('\'', r"'\''");
    let key_env: String = CI_KEY_ENV.iter().map(|(k, v)| format!("export {k}={v}; ")).collect();
    format!(
        "#!/bin/sh\n\
# Written by `./dev sccache env` (tools/paddock-dev/src/local_sccache.rs, shim_text): sccache for everything but a Windows (MSVC) C compile.\n\
case \"$(basename -- \"$1\")\" in\n\
  rustc | rustc.exe) {key_env}exec '{real}' \"$@\" ;;\n\
  clang-cl | clang-cl.exe | cl | cl.exe) exec \"$@\" ;;\n\
esac\n\
for a in \"$@\"; do\n\
  case \"$a\" in --target=*-windows-msvc) exec \"$@\" ;; esac\n\
done\n\
exec '{real}' \"$@\"\n"
    )
}

/// The cc-rs versions the shim policy was written for, as (major, minor) bounds: 1.8 to 1.11.
pub const CC_RANGE: ((u64, u64), (u64, u64)) = ((1, 8), (1, 11));

/// Where the shim is: `<bin dir>/shim/sccache` on Unix; on any other OS the binary itself (the local sccache is Unix only).
fn shim_for(bin: &Path, unix: bool) -> PathBuf {
    if unix {
        bin.with_file_name(SHIM_DIR).join("sccache")
    } else {
        bin.to_path_buf()
    }
}

/// What `RUSTC_WRAPPER` names on this OS.
fn rustc_wrapper_for(bin: &Path) -> PathBuf {
    shim_for(bin, cfg!(unix))
}

/// Writes the shim at install time (`Install::OnDemand`); the same content is left alone.
fn write_shim(bin: &Path) -> std::io::Result<()> {
    let target = rustc_wrapper_for(bin);
    if target == bin {
        return Ok(());
    }
    let text = shim_text(bin);
    if std::fs::read_to_string(&target).is_ok_and(|t| t == text) {
        return Ok(());
    }
    Ok(paddock_env::write_atomic_created(&target, text.as_bytes(), &mut |tmp| {
        let f = std::fs::OpenOptions::new().write(true).create_new(true).open(tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(f)
    })?)
}

/// A gate's path (`Install::Never`) writes nothing (ER-45): a missing or stale shim is a reason with its next step.
fn check_shim(bin: &Path) -> Result<(), String> {
    let target = rustc_wrapper_for(bin);
    if target == bin || std::fs::read_to_string(&target).is_ok_and(|t| t == shim_text(bin)) {
        return Ok(());
    }
    Err(format!(
        "the sccache shim {} is missing or out of date. Next: run `./dev sccache env` once on this machine (it writes the shim)",
        target.display()
    ))
}

/// The variables a build gets: the wrapper, the settings the server runs with and the two that keep a server a build starts itself
/// from replacing ours or hanging (never a credential).
fn ready_vars(bin: &Path, backend: &Backend, basedirs: &[(String, String)]) -> Vec<(String, String)> {
    let mut vars = vec![("RUSTC_WRAPPER".to_string(), rustc_wrapper_for(bin).display().to_string())];
    vars.extend(backend.build_vars());
    vars.extend(basedirs.iter().cloned());
    vars.push(("SCCACHE_IGNORE_SERVER_IO_ERROR".into(), "1".into()));
    vars.extend(survival_vars(matches!(backend, Backend::S3(_) | Backend::Tiered { .. })));
    vars
}

/// The mode the running server says it runs in (`--show-adv-stats`: `Effective cache mode            ReadWrite`).
pub fn cache_mode(stats: &str) -> Option<&str> {
    stats.lines().find_map(|l| l.strip_prefix("Effective cache mode")).map(str::trim)
}

/// `--start-server` on a port that already has a server (the next agent's or the Mac runner's) fails with "Address in use": that is
/// a running server, and the answer of `--show-stats` decides whether it is ours.
pub fn already_running(ran: &crate::ci_tool::Ran) -> bool {
    !ran.success && ran.stderr.contains("Address in use")
}

fn version_of(bin: &Path, env: &HashMap<String, String>) -> Option<String> {
    let ran = ToolCall::new(bin.to_string_lossy())
        .args(["--version"])
        .timeout(Duration::from_secs(30))
        .run(env, &mut std::io::sink())
        .ok()?;
    ran.stdout.split_whitespace().nth(1).map(str::to_string)
}

/// Downloads and installs the pinned release into `bin` after checking its SHA-256 (`base` is the release URL base: tests pass a
/// `file://` folder). The archive is unpacked in a folder of its own and the binary is renamed into place whole.
pub fn install_from(bin: &Path, base: &str, target: &str, ext: &str, sha256: &str, env: &HashMap<String, String>) -> Result<(), String> {
    if ext != "tar.gz" {
        return Err(format!(
            "the pinned release for this OS is a .{ext}, which the local install does not unpack yet"
        ));
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let work = bin
        .parent()
        .ok_or("the install path has no folder")?
        .join(format!(".dl-{}-{nanos:x}", std::process::id()));
    // The owner record lets `safe_remove` prove the folder is ours, so the removal at the end is allowed (and never silently refused).
    std::fs::create_dir_all(&work)
        .map_err(|e| format!("cannot make {}: {e}", work.display()))
        .and_then(|()| {
            crate::disk::record::write(
                &work,
                &crate::disk::record::Record::new(crate::disk::record::Kind::Build, "dev:sccache-install"),
            )
        })?;
    let parent = work.parent().map(Path::to_path_buf).unwrap_or_default();
    let done = (|| {
        let name = format!("sccache-v{SCCACHE_VERSION}-{target}");
        let archive = work.join(format!("{name}.{ext}"));
        let url = format!("{base}/v{SCCACHE_VERSION}/{name}.{ext}");
        let curl = ToolCall::new("curl")
            .args(["-fsSL", "--retry", "3", "-o"])
            .args([archive.to_string_lossy().into_owned(), url.clone()])
            .pass(NETWORK_ENV.iter().copied())
            .timeout(Duration::from_secs(300))
            .run(env, &mut std::io::sink())?;
        if !curl.success {
            return Err(format!("download of {url} failed ({}): {}", curl.how, tail(&curl.stderr, 3).trim()));
        }
        let got = sha256_file(&archive).map_err(|e| format!("cannot read the download: {e}"))?;
        if got != sha256 {
            return Err(format!(
                "checksum mismatch for {name}.{ext} (got {got}, pinned {sha256}); the download is not used"
            ));
        }
        let tar = ToolCall::new("tar")
            .args(["-xzf", &archive.to_string_lossy(), "-C", &work.to_string_lossy()])
            .timeout(Duration::from_secs(120))
            .run(env, &mut std::io::sink())?;
        if !tar.success {
            return Err(format!("cannot unpack the download ({}): {}", tar.how, tail(&tar.stderr, 3).trim()));
        }
        let from = work.join(&name).join(tool_cache::exe("sccache"));
        let part = bin.with_extension(format!("part.{}", std::process::id()));
        std::fs::copy(&from, &part).map_err(|e| format!("cannot copy {}: {e}", from.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&part, bin).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            format!("cannot put sccache into {}: {e}", bin.display())
        })
    })();
    // Our own unique folder, made above: nothing else is removed (ER-14).
    if let Err(why) = crate::safe_remove::remove(&work, std::slice::from_ref(&parent)) {
        eprintln!(
            "::warning::local-sccache: the download folder {} was not removed: {why}. Next: delete it by hand",
            work.display()
        );
    }
    done
}

/// Installs the pinned release for this machine when it is not there, and returns the binary.
fn ensure_binary(env: &HashMap<String, String>, install: Install) -> Result<PathBuf, String> {
    let bin = install_path(env)?;
    if bin.is_file() && version_of(&bin, env).as_deref() == Some(SCCACHE_VERSION) {
        return Ok(bin);
    }
    if install == Install::Never {
        return Err(format!(
            "it is not installed at {}. Next: run `./dev sccache env` once on this machine (it installs the pinned release)",
            bin.display()
        ));
    }
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    };
    let arch = std::env::consts::ARCH;
    let pin = pin_for(os, arch).ok_or_else(|| format!("there is no pinned sccache {SCCACHE_VERSION} for {os} {arch}"))?;
    std::fs::create_dir_all(bin.parent().unwrap_or(Path::new("."))).map_err(|e| format!("cannot make the tool folder: {e}"))?;
    let base = env
        .get("PADDOCK_SCCACHE_RELEASES")
        .cloned()
        .unwrap_or_else(|| "https://github.com/mozilla/sccache/releases/download".into());
    install_from(&bin, &base, pin.target, pin.ext, pin.sha256, env)?;
    match version_of(&bin, env) {
        Some(v) if v == SCCACHE_VERSION => Ok(bin),
        other => Err(format!("the installed sccache answers {other:?}, not {SCCACHE_VERSION}")),
    }
}

/// Makes the wrapper ready: the pinned binary, one server on the default port for the chosen backend (started when none runs). Never
/// fails a build: every problem is an [`Outcome::Off`] with its cause and next step.
pub fn prepare(env: &HashMap<String, String>, install: Install) -> Outcome {
    if let Some(why) = declined(env) {
        return off(why, false);
    }
    let started = std::time::Instant::now();
    let (backend, fallback) = match choose(env, &|s3| s3_alive(s3, env)) {
        Ok(b) => b,
        Err(e) => {
            return off(
                format!("{e}. Next: fix the PADDOCK_SCCACHE_* settings (docs/dev-setup.md, \"Local sccache\")"),
                true,
            );
        }
    };
    if let Some(f) = &fallback {
        eprintln!(
            "::{}::local-sccache: the shared S3 cache is not used: {}; this build uses the local disk cache only. Next: docs/dev-setup.md, \"Local sccache\"",
            if f.warn { "warning" } else { "notice" },
            f.why
        );
    }
    let creds = match &backend {
        Backend::S3(_) | Backend::Tiered { .. } => match read_creds(env) {
            Ok(c) if c.status() == "configured" => {
                if c.env_ignored() {
                    eprintln!(
                        "::notice::local-sccache: AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY in your environment are ignored for sccache; the key file's keys are used (docs/dev-setup.md, \"Local sccache\")"
                    );
                }
                Some(c)
            }
            Ok(_) => {
                return off(
                    "the S3 credentials are missing (AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY in the environment or in ~/.config/paddock/sccache-s3.env). Next: see docs/dev-setup.md, \"Local sccache\", or use PADDOCK_SCCACHE_BACKEND=local".to_string(),
                    true,
                );
            }
            Err(e) => return off(format!("{e}. Next: fix the credentials file"), true),
        },
        Backend::Local { .. } => None,
    };
    let bin = match ensure_binary(env, install) {
        Ok(b) => b,
        Err(e) => {
            return off(
                format!(
                    "sccache {SCCACHE_VERSION} is not available: {e}. Next: `./dev sccache env` installs it (needs a network); PADDOCK_SCCACHE=off silences this"
                ),
                true,
            );
        }
    };
    let run = match run_folder(env) {
        Ok(r) => r,
        Err(e) => return off(format!("{e}. Next: set PADDOCK_DEV_HOME"), true),
    };
    if let Err(e) = std::fs::create_dir_all(&run) {
        return off(
            format!("cannot make the server's folder {}: {e}. Next: fix PADDOCK_DEV_HOME", run.display()),
            true,
        );
    }
    if let Backend::Local { dir, .. } | Backend::Tiered { dir, .. } = &backend
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return off(format!("cannot make the cache folder {dir}: {e}. Next: fix SCCACHE_DIR"), true);
    }
    let log = error_log(&run);
    let basedirs = basedir_vars(
        &std::env::current_dir().map(|c| worktree_roots(&c)).unwrap_or_default(),
        cfg!(windows),
    );
    let server = Server {
        bin: &bin,
        run: &run,
        backend: &backend,
        creds: creds.as_ref(),
        log_level: env
            .get("SCCACHE_LOG")
            .filter(|l| !l.is_empty())
            .cloned()
            .unwrap_or_else(|| "warn".into()),
        basedirs: basedirs.clone(),
    };
    let where_log = format!("The server's log: {}", log.display());
    let long_path = if run.as_os_str().len() > SHORT_RUN_PATH {
        format!(
            " The server's folder path is {} bytes and macOS caps socket paths at 104 bytes: use a shorter PADDOCK_DEV_HOME.",
            run.as_os_str().len()
        )
    } else {
        String::new()
    };
    // At most two starts: a server whose startup write check failed once stays read-only for its whole life (sccache 0.19.0
    // `RemoteStorage::check`), so it is stopped and started once more; still read-only, the wrapper is off.
    let mut restarted = false;
    let mut upgraded = false;
    loop {
        let start_log = run.join(format!("start-{}.log", std::process::id()));
        // The server outlives this process and would hold a pipe open, so its starter writes to a file.
        let mut started_here = false;
        // A server already listening is not started again: the start would only fail with "Address in use" and write that to the
        // running server's error log, the log every warning names (247 times in two days, docs/research/2026-11-19-sccache.md).
        // After a restart of our own server the port is started without asking.
        let start = if !restarted && !upgraded && listening(env) {
            Ok(crate::ci_tool::Ran {
                success: true,
                how: "already listening".into(),
                stdout: String::new(),
                stderr: String::new(),
            })
        } else {
            server
                .call("--start-server")
                .output(Output::File(start_log.clone()))
                .run(env, &mut std::io::sink())
                .map(|mut s| {
                    started_here = s.success;
                    s.success |= already_running(&s);
                    s
                })
        };
        let _ = std::fs::remove_file(&start_log);
        let stats = server.call("--show-adv-stats").run(env, &mut std::io::sink());
        let (start, stats) = match (start, stats) {
            (Ok(s), Ok(t)) if s.success && t.success => (s, t),
            (Ok(s), _) if !s.success => {
                return off(
                    format!(
                        "the sccache server did not start ({}): {}. {where_log}.{long_path}",
                        s.how,
                        tail(&s.stderr, 4).trim()
                    ),
                    true,
                );
            }
            (Err(e), _) | (_, Err(e)) => return off(format!("sccache could not be run: {e}. {where_log}."), true),
            _ => {
                return off(
                    format!("the sccache server did not answer `--show-adv-stats`. {where_log}.{long_path}"),
                    true,
                );
            }
        };
        let _ = start;
        // A plain-disk server on this tool's own folder, while this run wants the chain: it was started when the store was down (an `auto`
        // run that fell back), so it is replaced once, now that the store answers; left alone, every later build would be uncached
        // for good (the server never exits on its own).
        if !upgraded
            && let Backend::Tiered { dir, .. } = &backend
            && location(&stats.stdout) == Some(format!("Local disk: \"{}\"", dir.strip_suffix("-s3").unwrap_or(dir)))
        {
            upgraded = true;
            eprintln!(
                "::notice::local-sccache: the server on port {} runs the plain local disk (it was started while the shared store was down); stopping it and starting it with the store behind the disk",
                server_port(env)
            );
            let _ = server.call("--stop-server").run(env, &mut std::io::sink());
            continue;
        }
        // An `auto` run that fell back to the disk (the store is down now) keeps a server of ours that already runs the chain: its
        // second level fails soft (`l0`), and stopping it would throw away a working cache.
        let keeps_chain = matches!(&backend, Backend::Local { dir, .. } if fallback.is_some() && chain_on(&stats.stdout, dir));
        if !backend.matches_stats(&stats.stdout) && !keeps_chain {
            return off(
                format!(
                    "the sccache server on port {} does not use this cache ({}); it is not touched. Next: stop it yourself (`sccache --stop-server`) or leave the wrapper off. {where_log}",
                    server_port(env),
                    backend.describe()
                ),
                true,
            );
        }
        if cache_mode(&stats.stdout) != Some("ReadOnly") {
            break;
        }
        if !started_here {
            // Another agent's or the user's server: stopping it would break their caching. Reads still hit, so it is kept.
            eprintln!("{}", already_running_read_only_warning(env, &log));
            break;
        }
        if restarted {
            return off(
                format!(
                    "the sccache server came up read-only twice: its startup write check against the cache failed, and sccache 0.19.0 keeps that result for the server's whole life, so no compile would be stored. {where_log}. Next: read the log for the failed write (the bucket, the key's rights, the folder's permissions), fix it, then `sccache --stop-server`"
                ),
                true,
            );
        }
        restarted = true;
        eprintln!("local-sccache: the server came up read-only; stopping it (this call started it) and starting it once more");
        let _ = server.call("--stop-server").run(env, &mut std::io::sink());
    }
    eprintln!(
        "local-sccache: sccache {SCCACHE_VERSION} on, {}{}, ready in {:.1} s; server log {}",
        backend.describe(),
        creds.as_ref().map_or(String::new(), |c| format!(", credentials {}", c.status())),
        started.elapsed().as_secs_f64(),
        log.display()
    );
    let shim = match install {
        Install::OnDemand => write_shim(&bin).map_err(|e| {
            format!(
                "the sccache shim {} could not be written: {e}. Next: make the folder writable or remove the file, then run `./dev sccache env` again",
                rustc_wrapper_for(&bin).display()
            )
        }),
        Install::Never => check_shim(&bin),
    };
    if let Err(why) = shim {
        return off(why, true);
    }
    Outcome::Ready(Ready {
        vars: ready_vars(&bin, &backend, &basedirs),
        bin,
    })
}

/// The one warning for a server that was already running (not started by this call) and is read-only: it is somebody else's, so it
/// is neither stopped nor restarted, and its reads still give hits.
fn already_running_read_only_warning(env: &HashMap<String, String>, log: &Path) -> String {
    format!(
        "::warning::local-sccache: the sccache server already running on port {} is read-only (its startup write check failed), so no compile is stored, only read. It is not this call's, so it is not stopped. Its log: {}. Next: stop it yourself (`sccache --stop-server`) or wait for its owner; this build keeps using it read-only.",
        server_port(env),
        log.display()
    )
}

/// [`prepare`], and the warning for a failure (once per call), as the tools that start builds use it.
pub fn prepare_and_say(env: &HashMap<String, String>, install: Install) -> Option<Ready> {
    match prepare(env, install) {
        Outcome::Ready(r) => Some(r),
        Outcome::Off { reason, warn } => {
            if warn {
                eprintln!("::warning::local-sccache: building without the compile cache: {reason}");
            }
            None
        }
    }
}

/// The wrapper, prepared on first use and never before: a tool that only plans, lists or runs a non-cargo step must not start a
/// server (ER-45), and a test that spawns `paddock-dev` must not reach [`prepare`] unless it runs a cargo step through it.
pub struct Lazy {
    env: HashMap<String, String>,
    cell: std::sync::OnceLock<Option<Ready>>,
}

impl Lazy {
    /// No wrapper, ever (tests).
    pub fn off() -> Lazy {
        let cell = std::sync::OnceLock::new();
        let _ = cell.set(None);
        Lazy { env: HashMap::new(), cell }
    }

    /// A wrapper that is already there (tests of the call's environment).
    pub fn ready(r: Ready) -> Lazy {
        let cell = std::sync::OnceLock::new();
        let _ = cell.set(Some(r));
        Lazy { env: HashMap::new(), cell }
    }

    /// Prepared with [`Install::Never`] from `env` when [`Lazy::get`] is first called.
    pub fn on_demand(env: HashMap<String, String>) -> Lazy {
        Lazy {
            env,
            cell: std::sync::OnceLock::new(),
        }
    }

    /// The wrapper, preparing it now when this is the first call.
    pub fn get(&self) -> Option<&Ready> {
        self.cell.get_or_init(|| prepare_and_say(&self.env, Install::Never)).as_ref()
    }
}

/// Warnings for a shell that is about to build: a `CARGO_TARGET_DIR` (or `CARGO_BUILD_TARGET_DIR`) in its environment defeats the
/// cache (sccache hashes it, and the variable outranks the configuration the tools pass).
pub fn shell_warnings(env: &HashMap<String, String>) -> Vec<String> {
    ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"]
        .iter()
        .filter(|k| env.get(**k).is_some_and(|v| !v.is_empty()))
        .map(|k| {
            format!(
                "::warning::local-sccache: your shell exports {k}; sccache hashes it, so builds miss the cache across worktrees. Next: `unset {k}` and pass the folder as `--config build.target-dir=...`"
            )
        })
        .collect()
}

/// Adds the wrapper's variables to a call.
pub fn apply(call: ToolCall, ready: &Ready) -> ToolCall {
    ready.vars.iter().fold(call, |c, (k, v)| c.env(k, v))
}

/// `export` lines for a shell (`eval "$(./dev sccache env)"`): single-quoted, so a path never splits or expands.
pub fn shell_lines(ready: &Ready) -> Vec<String> {
    ready
        .vars
        .iter()
        .map(|(k, v)| format!("export {k}='{}'", v.replace('\'', r"'\''")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn ci_an_explicit_off_and_a_callers_own_wrapper_leave_the_build_alone_without_a_warning() {
        for e in [
            env(&[("CI", "true")]),
            env(&[("GITHUB_ACTIONS", "true")]),
            env(&[("PADDOCK_SCCACHE", "off")]),
            env(&[("RUSTC_WRAPPER", "/x/sccache")]),
        ] {
            assert!(declined(&e).is_some(), "{e:?}");
            assert!(matches!(prepare(&e, Install::OnDemand), Outcome::Off { warn: false, .. }));
        }
        assert!(declined(&env(&[])).is_none());
    }

    #[test]
    fn the_cache_folder_and_size_are_the_machines_profile_when_set() {
        let e = env(&[("SCCACHE_DIR", "/v/sccache"), ("SCCACHE_CACHE_SIZE", "60G")]);
        assert_eq!(cache_dir(&e).unwrap(), "/v/sccache");
        assert_eq!(cache_size(&e), "60G");
        let e = env(&[("PADDOCK_DEV_HOME", "/h")]);
        // Joined by the OS's separator (`/h\\sccache` on Windows, run 8209): compare as paths, not as text.
        assert_eq!(Path::new(&cache_dir(&e).unwrap()), Path::new("/h").join("sccache"));
        assert_eq!(cache_size(&e), DEFAULT_SIZE);
        assert_eq!(
            install_path(&e).unwrap(),
            Path::new("/h/tools/sccache").join(SCCACHE_VERSION).join(tool_cache::exe("sccache"))
        );
    }

    #[test]
    fn a_server_with_another_backend_or_folder_is_not_ours() {
        let local = "Cache location                  Local disk: \"/v/sccache\"\nCache size  0 bytes\n";
        let me = Backend::Local {
            dir: "/v/sccache".into(),
            size: "1G".into(),
        };
        assert!(me.matches_stats(local));
        assert!(
            !Backend::Local {
                dir: "/other".into(),
                size: "1G".into()
            }
            .matches_stats(local)
        );
        assert!(!me.matches_stats("Cache location                  Redis: redis://x\n"));
        let s3 = backend(&env(&[
            ("PADDOCK_SCCACHE_BACKEND", "s3"),
            ("PADDOCK_SCCACHE_S3_BUCKET", "b1"),
            ("PADDOCK_SCCACHE_S3_ENDPOINT", "h:1"),
        ]))
        .unwrap();
        assert!(s3.matches_stats("Cache location                  s3, name: b1, prefix: /paddock/\n"));
        assert!(!s3.matches_stats(local));
    }

    #[test]
    fn the_stats_must_name_exactly_this_folder_bucket_and_prefix() {
        let line = |loc: &str| format!("Compile requests  0\nCache location                  {loc}\n");
        let local = |d: &str| Backend::Local {
            dir: d.into(),
            size: "1G".into(),
        };
        assert!(local("/a/b").matches_stats(&line("Local disk: \"/a/b\"")));
        for other in [
            "Local disk: \"/a/b2\"",
            "Local disk: \"/x/a/b\"",
            "Local disk: \"/a\"",
            "Local disk: /a/b",
        ] {
            assert!(!local("/a/b").matches_stats(&line(other)), "{other}");
        }
        let s3 = |prefix: &str| {
            backend(&env(&[
                ("PADDOCK_SCCACHE_BACKEND", "s3"),
                ("PADDOCK_SCCACHE_S3_BUCKET", "b1"),
                ("PADDOCK_SCCACHE_S3_ENDPOINT", "h:1"),
                ("PADDOCK_SCCACHE_S3_PREFIX", prefix),
            ]))
            .unwrap()
        };
        assert!(s3("paddock").matches_stats(&line("s3, name: b1, prefix: /paddock/")));
        assert!(!s3("paddock").matches_stats(&line("s3, name: b1, prefix: /paddock2/")));
        assert!(!s3("paddock").matches_stats(&line("s3, name: b10, prefix: /paddock/")));
        assert!(!s3("paddock").matches_stats(&line("s3, name: b1, prefix: /")));
    }

    fn child_of(arg: &str, backend: &Backend, parent: &HashMap<String, String>) -> Vec<(String, String)> {
        let server = Server {
            bin: Path::new("/h/tools/sccache/0.20.1/sccache"),
            run: Path::new("/h/sccache-run"),
            backend,
            creds: None,
            log_level: "warn".into(),
            basedirs: Vec::new(),
        };
        server
            .call(arg)
            .pass(["TMPDIR", "TMP", "TEMP", "SCCACHE_ERROR_LOG", "SCCACHE_IDLE_TIMEOUT"])
            .child_env(parent)
    }

    fn last(child: &[(String, String)], k: &str) -> Option<String> {
        child.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }

    /// The server outlives the process that starts it, so it must not inherit that process's temporary folder: builds fail once
    /// a job's folder is deleted (measured in review of 872ab6743).
    #[test]
    fn the_server_runs_in_a_tool_owned_folder_whatever_the_starters_tmpdir_is() {
        let backend = Backend::Local {
            dir: "/c".into(),
            size: "1G".into(),
        };
        let parent = env(&[
            ("TMPDIR", "/jobs/gone"),
            ("TMP", "/jobs/gone"),
            ("TEMP", "/jobs/gone"),
            ("PATH", "/usr/bin"),
        ]);
        for arg in ["--start-server", "--show-adv-stats"] {
            let child = child_of(arg, &backend, &parent);
            for k in ["TMPDIR", "TMP", "TEMP"] {
                assert_eq!(last(&child, k).as_deref(), Some("/h/sccache-run"), "{arg} {k}: {child:?}");
                assert_eq!(child.iter().filter(|(n, _)| n == k).count(), 1, "one value, no inherited copy: {k}");
            }
        }
        let home = env(&[("PADDOCK_DEV_HOME", "/h")]);
        assert_eq!(run_folder(&home).unwrap(), Path::new("/h/sccache-run"));
        assert!(
            run_folder(&home).unwrap().as_os_str().len() < SHORT_RUN_PATH,
            "short, for macOS's 104-byte socket paths"
        );
    }

    /// ER-27: the server always logs its errors to a file in its own folder, whatever the caller's environment says, and never
    /// exits on its own (a build that found none would start one with the build's environment, S3 without keys).
    #[test]
    fn the_server_logs_to_its_folder_never_idles_out_and_asks_no_metadata_service() {
        let local = Backend::Local {
            dir: "/c".into(),
            size: "1G".into(),
        };
        let parent = env(&[
            ("SCCACHE_ERROR_LOG", "/elsewhere/x.log"),
            ("SCCACHE_IDLE_TIMEOUT", "600"),
            ("PATH", "/usr/bin"),
        ]);
        let child = child_of("--start-server", &local, &parent);
        assert_eq!(
            last(&child, "SCCACHE_ERROR_LOG").map(std::path::PathBuf::from),
            Some(Path::new("/h/sccache-run").join("sccache.log")),
            "{child:?}"
        );
        assert_eq!(last(&child, "SCCACHE_IDLE_TIMEOUT").as_deref(), Some("0"), "{child:?}");
        assert_eq!(last(&child, "SCCACHE_LOG").as_deref(), Some("warn"));
        assert!(last(&child, "AWS_EC2_METADATA_DISABLED").is_none(), "only for S3");
        let s3 = backend(&env(&[
            ("PADDOCK_SCCACHE_BACKEND", "s3"),
            ("PADDOCK_SCCACHE_S3_BUCKET", "b"),
            ("PADDOCK_SCCACHE_S3_ENDPOINT", "h:1"),
        ]))
        .unwrap();
        assert_eq!(
            last(&child_of("--start-server", &s3, &parent), "AWS_EC2_METADATA_DISABLED").as_deref(),
            Some("true")
        );
        // A build that has to start a server itself gets the same two protections.
        for b in [&local, &s3] {
            let vars = ready_vars(Path::new("/b/sccache"), b, &[]);
            assert!(vars.contains(&("SCCACHE_IDLE_TIMEOUT".into(), "0".into())), "{vars:?}");
            assert!(
                vars.iter()
                    .all(|(k, _)| !k.starts_with("AWS_ACCESS") && !k.starts_with("AWS_SECRET")),
                "never a key"
            );
        }
        assert!(ready_vars(Path::new("/b/sccache"), &s3, &[]).contains(&("AWS_EC2_METADATA_DISABLED".into(), "true".into())));
    }

    #[test]
    fn the_cache_mode_is_read_from_the_advanced_stats() {
        assert_eq!(
            cache_mode("Cache location  x\nEffective cache mode            ReadOnly\n"),
            Some("ReadOnly")
        );
        assert_eq!(cache_mode("Effective cache mode            ReadWrite"), Some("ReadWrite"));
        assert_eq!(cache_mode("Cache location  x\n"), None);
    }

    /// A stand-in sccache: `--version`, start and stop are recorded, and the stats say read-only for the first `readonly_starts`
    /// starts (sccache 0.19.0 keeps a failed startup write check for the server's whole life). Each start makes a file
    /// `starts-<n>` beside it (up to four), so the number of starts is which of them exist.
    #[cfg(unix)]
    fn fake(
        readonly_starts: u32,
        foreign_location: Option<&str>,
    ) -> (tempfile::TempDir, HashMap<String, String>, paddock_testkit::Installed) {
        use paddock_testkit::{Actions, Case, StandIn};
        let home = tempfile::tempdir().unwrap();
        let e = env(&[
            ("PADDOCK_DEV_HOME", home.path().to_str().unwrap()),
            ("SCCACHE_DIR", home.path().join("cache").to_str().unwrap()),
            ("PATH", "/usr/bin:/bin"),
            // A port nothing listens on: the default 4226 is the machine's own server, which a test must never see (ER-14).
            ("SCCACHE_SERVER_PORT", free_port().to_string().as_str()),
        ]);
        let bin = install_path(&e).unwrap();
        let dir = bin.parent().unwrap();
        std::fs::create_dir_all(dir).unwrap();
        let starts = |n: u32| dir.join(format!("starts-{n}"));
        let location = match foreign_location {
            Some(l) if l.starts_with(MULTILEVEL_LOCATION) => format!("{CHAIN_COUNTERS}Cache location                  {l}"),
            Some(l) => format!("Cache location                  {l}"),
            None => "Cache location                  Local disk: \"{env:SCCACHE_DIR}\"".to_string(),
        };
        let stats = |mode: &str| {
            Actions::new()
                .stdout_with(&location)
                .stdout(&format!("Effective cache mode            {mode}"))
        };
        let mut fake = StandIn::new(dir, "sccache")
            .records_args()
            .stdout("sccache 0.20.1")
            .when_args(&["--stop-server"], Actions::new())
            .when(
                &Case::new()
                    .args_start_with(&["--show-adv-stats"])
                    .file_missing(&starts(readonly_starts + 1)),
                stats("ReadOnly"),
            )
            .when(
                &Case::new()
                    .args_start_with(&["--show-adv-stats"])
                    .file_exists(&starts(readonly_starts + 1)),
                stats("ReadWrite"),
            );
        for n in 1..=4 {
            let mut case = Case::new().args_start_with(&["--start-server"]).file_missing(&starts(n));
            if n > 1 {
                case = case.file_exists(&starts(n - 1));
            }
            fake = fake.when(&case, Actions::new().writes(&starts(n), ""));
        }
        // Last, so it is asked first: a server that is already up answers every start with this.
        fake = fake.when(
            &Case::new()
                .args_start_with(&["--start-server"])
                .file_exists(&dir.join("address-in-use")),
            Actions::new().stderr("Address in use (os error 48)").exit(1),
        );
        let installed = fake.install().unwrap();
        write_shim(installed.program()).unwrap(); // what `./dev sccache env` did once on this machine
        (home, e, installed)
    }

    /// A port that is taken for the rest of the test process and nothing listens on (HeldPort::refusing, kept by `forget`): no
    /// other process can be given it while a test reads it.
    fn free_port() -> u16 {
        let held = paddock_testkit::net::HeldPort::refusing();
        let port = held.port();
        std::mem::forget(held);
        port
    }

    #[test]
    fn listening_sees_a_listener_and_not_a_free_port() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port().to_string();
        assert!(listening(&env(&[("SCCACHE_SERVER_PORT", port.as_str())])));
        drop(l);
        assert!(!listening(&env(&[("SCCACHE_SERVER_PORT", free_port().to_string().as_str())])));
        assert!(!listening(&env(&[("SCCACHE_SERVER_PORT", "not-a-port")])));
    }

    /// A server already listening on the port is not started again (its error log would get "Address in use" on every call),
    /// and it is not ours, so a read-only one is not stopped either.
    #[cfg(unix)]
    #[test]
    fn a_listening_server_is_not_started_again() {
        let (_h, mut e, bin) = fake(0, None);
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        e.insert("SCCACHE_SERVER_PORT".into(), l.local_addr().unwrap().port().to_string());
        assert!(matches!(prepare(&e, Install::Never), Outcome::Ready(_)));
        assert_eq!(calls(&bin), Vec::<String>::new(), "no --start-server, no --stop-server");
    }

    #[cfg(unix)]
    fn calls(bin: &paddock_testkit::Installed) -> Vec<String> {
        bin.calls()
            .unwrap()
            .into_iter()
            .filter_map(|a| a.into_iter().next())
            .filter(|a| a.starts_with("--start") || a.starts_with("--stop"))
            .collect()
    }

    #[cfg(unix)]
    #[test]
    fn a_server_that_comes_up_read_only_is_stopped_and_started_once_more() {
        let (_h, e, bin) = fake(1, None);
        assert!(matches!(prepare(&e, Install::Never), Outcome::Ready(_)));
        assert_eq!(calls(&bin), ["--start-server", "--stop-server", "--start-server"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_server_that_is_read_only_twice_turns_the_wrapper_off_and_names_the_cause_and_the_log() {
        let (h, e, bin) = fake(2, None);
        match prepare(&e, Install::Never) {
            Outcome::Off { reason, warn } => {
                assert!(warn);
                assert!(
                    reason.contains("read-only twice") && reason.contains("keeps that result"),
                    "{reason}"
                );
                let log = h.path().join("sccache-run/sccache.log");
                assert!(reason.contains(&log.display().to_string()), "the log path is named: {reason}");
                assert!(reason.contains("Next:"), "{reason}");
            }
            Outcome::Ready(_) => panic!("a read-only server stores nothing"),
        }
        assert_eq!(calls(&bin), ["--start-server", "--stop-server", "--start-server"], "no third start");
    }

    /// ER-45: a gate (`Install::Never`) never writes the shim; with none it is off, with the next step.
    #[cfg(unix)]
    #[test]
    fn a_gate_without_the_shim_turns_the_wrapper_off_and_writes_nothing() {
        let (_h, e, bin) = fake(0, None);
        std::fs::remove_file(rustc_wrapper_for(bin.program())).unwrap();
        match prepare(&e, Install::Never) {
            Outcome::Off { reason, warn } => assert!(warn && reason.contains("Next: run `./dev sccache env`"), "{reason}"),
            Outcome::Ready(_) => panic!("no shim"),
        }
        assert!(!rustc_wrapper_for(bin.program()).exists());
        assert!(
            matches!(prepare(&e, Install::OnDemand), Outcome::Ready(_)),
            "the install path writes it"
        );
        assert!(rustc_wrapper_for(bin.program()).exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_read_write_server_is_left_running() {
        let (_h, e, bin) = fake(0, None);
        assert!(matches!(prepare(&e, Install::Never), Outcome::Ready(_)));
        assert_eq!(calls(&bin), ["--start-server"]);
    }

    /// A server that was already running (this call's `--start-server` failed with "Address in use") belongs to another agent or
    /// the user: it is never stopped, and a read-only one is used as it is (reads still hit) with one warning.
    #[cfg(unix)]
    #[test]
    fn an_already_running_read_only_server_is_never_stopped_and_is_warned_about() {
        let (h, e, bin) = fake(5, None);
        std::fs::write(bin.dir().join("address-in-use"), "").unwrap();
        std::fs::write(bin.dir().join("starts-1"), "").unwrap(); // the other owner's start: stats say read-only
        assert!(
            matches!(prepare(&e, Install::Never), Outcome::Ready(_)),
            "read-only is still used for reads"
        );
        assert_eq!(calls(&bin), ["--start-server"], "no --stop-server: the server is not ours");
        let warning = already_running_read_only_warning(&e, &h.path().join("sccache-run/sccache.log"));
        assert!(warning.starts_with("::warning::"), "{warning}");
        assert!(
            warning.contains("read-only") && warning.contains("sccache.log") && warning.contains("stop it yourself"),
            "{warning}"
        );
    }

    /// A server of somebody else's (another cache) is neither used nor stopped, read-only or not.
    #[cfg(unix)]
    #[test]
    fn a_foreign_read_only_server_is_never_stopped() {
        let (_h, e, bin) = fake(5, Some("Local disk: \"/somebody/else\""));
        match prepare(&e, Install::Never) {
            Outcome::Off { reason, warn } => assert!(warn && reason.contains("does not use this cache"), "{reason}"),
            Outcome::Ready(_) => panic!("not our cache"),
        }
        assert_eq!(calls(&bin), ["--start-server"]);
    }

    /// S3 without keys fails at once, before any binary is run or any request made (it used to hang about 75 s in the metadata
    /// lookup of a server that a build started).
    #[test]
    fn s3_without_keys_fails_fast_without_touching_the_binary_or_the_network() {
        let home = tempfile::tempdir().unwrap();
        let e = env(&[
            ("PADDOCK_DEV_HOME", home.path().to_str().unwrap()),
            ("PADDOCK_SCCACHE_BACKEND", "s3"),
            ("PADDOCK_SCCACHE_S3_BUCKET", "b"),
            ("PADDOCK_SCCACHE_S3_ENDPOINT", "10.255.255.1:15000"),
            ("PADDOCK_SCCACHE_ENV_FILE", home.path().join("none.env").to_str().unwrap()),
        ]);
        let t = std::time::Instant::now();
        assert!(matches!(prepare(&e, Install::OnDemand), Outcome::Off { warn: true, .. }));
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
        assert!(
            !home.path().join("tools").exists() && !home.path().join("sccache-run").exists(),
            "nothing was made or run"
        );
    }

    #[test]
    fn a_shell_that_exports_the_target_folder_is_warned() {
        assert!(shell_warnings(&env(&[("PATH", "/x")])).is_empty());
        let w = shell_warnings(&env(&[("CARGO_TARGET_DIR", "/t"), ("CARGO_BUILD_TARGET_DIR", "/u")]));
        assert_eq!(w.len(), 2);
        assert!(w[0].starts_with("::warning::") && w[0].contains("unset CARGO_TARGET_DIR") && w[0].contains("--config build.target-dir"));
        assert_eq!(server_port(&env(&[])), "4226");
        assert_eq!(server_port(&env(&[("SCCACHE_SERVER_PORT", "4300")])), "4300");
    }

    #[test]
    fn a_server_that_is_already_running_is_not_a_failed_start() {
        let ran = |success: bool, stderr: &str| crate::ci_tool::Ran {
            success,
            how: String::new(),
            stdout: String::new(),
            stderr: stderr.into(),
        };
        assert!(already_running(&ran(
            false,
            "sccache: error: Server startup failed: Address in use"
        )));
        assert!(!already_running(&ran(true, "Address in use")));
        assert!(!already_running(&ran(false, "permission denied")));
    }

    #[test]
    fn the_backend_is_one_setting_local_by_default() {
        assert!(matches!(
            backend(&env(&[("PADDOCK_DEV_HOME", "/h")])).unwrap(),
            Backend::Local { .. }
        ));
        assert!(
            backend(&env(&[("PADDOCK_SCCACHE_BACKEND", "s3")]))
                .unwrap_err()
                .contains("PADDOCK_SCCACHE_S3_BUCKET")
        );
        assert!(
            backend(&env(&[("PADDOCK_SCCACHE_BACKEND", "redis")]))
                .unwrap_err()
                .contains("local or s3")
        );
        let e = env(&[
            ("PADDOCK_SCCACHE_BACKEND", "s3"),
            ("PADDOCK_SCCACHE_S3_BUCKET", "sccache-paddock"),
            ("PADDOCK_SCCACHE_S3_ENDPOINT", "192.0.2.10:15000"),
        ]);
        let vars = backend(&e).unwrap().vars();
        assert!(vars.contains(&("SCCACHE_BUCKET".into(), "sccache-paddock".into())));
        assert!(vars.contains(&("SCCACHE_S3_USE_SSL".into(), "false".into())));
        assert!(
            vars.iter().all(|(k, _)| !k.starts_with("AWS_")),
            "a credential is never one of the settings"
        );
    }

    #[test]
    fn credentials_are_read_from_the_env_file_and_only_ever_reported_as_configured_or_missing() {
        let text = "# keys\nexport AWS_ACCESS_KEY_ID=\"AKIAEXAMPLE\"\nAWS_SECRET_ACCESS_KEY='s3cr3t'\nSCCACHE_REDIS=redis://x\n";
        let got = parse_env_file(text);
        assert_eq!(got.len(), 2, "no other line reaches the server: {got:?}");
        let c = Creds {
            pairs: got,
            env_ignored: false,
        };
        assert_eq!(c.status(), "configured");
        let shown = format!("{c:?}");
        assert!(!shown.contains("AKIAEXAMPLE") && !shown.contains("s3cr3t"), "{shown}");
        assert_eq!(
            Creds {
                pairs: Vec::new(),
                env_ignored: false
            }
            .status(),
            "missing"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_credentials_file_others_can_read_is_refused_and_a_missing_one_is_missing() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("s3.env");
        let e = env(&[("PADDOCK_SCCACHE_ENV_FILE", f.to_str().unwrap())]);
        assert_eq!(read_creds(&e).unwrap().status(), "missing");
        std::fs::write(&f, "AWS_ACCESS_KEY_ID=a\nAWS_SECRET_ACCESS_KEY=b\n").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        let why = read_creds(&e).unwrap_err();
        assert!(why.contains("chmod 600") && !why.contains("=a"), "{why}");
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_creds(&e).unwrap().status(), "configured");
    }

    #[test]
    fn s3_without_credentials_turns_the_wrapper_off_with_a_warning_and_never_touches_the_network() {
        let home = tempfile::tempdir().unwrap();
        let e = env(&[
            ("PADDOCK_DEV_HOME", home.path().to_str().unwrap()),
            ("PADDOCK_SCCACHE_BACKEND", "s3"),
            ("PADDOCK_SCCACHE_S3_BUCKET", "b"),
            ("PADDOCK_SCCACHE_S3_ENDPOINT", "127.0.0.1:1"),
            ("PADDOCK_SCCACHE_ENV_FILE", home.path().join("none.env").to_str().unwrap()),
        ]);
        match prepare(&e, Install::OnDemand) {
            Outcome::Off { reason, warn } => assert!(warn && reason.contains("credentials are missing"), "{reason}"),
            Outcome::Ready(_) => panic!("no credentials"),
        }
    }

    #[test]
    fn shell_lines_quote_the_values() {
        let r = Ready {
            bin: "/b".into(),
            vars: vec![("RUSTC_WRAPPER".into(), "/a b/it's".into())],
        };
        assert_eq!(shell_lines(&r), [r"export RUSTC_WRAPPER='/a b/it'\''s'"]);
    }

    /// A missing sccache is not a failed build: the wrapper is off with a warning that names the cause and the next step.
    #[cfg(unix)]
    #[test]
    fn a_missing_sccache_turns_the_wrapper_off_with_a_warning() {
        let home = tempfile::tempdir().unwrap();
        let e = env(&[
            ("PADDOCK_DEV_HOME", home.path().to_str().unwrap()),
            ("SCCACHE_DIR", home.path().join("cache").to_str().unwrap()),
            // No network and no release at this base: the download fails.
            ("PADDOCK_SCCACHE_RELEASES", "file:///nonexistent-paddock-test"),
            ("PATH", "/usr/bin:/bin"),
        ]);
        match prepare(&e, Install::OnDemand) {
            Outcome::Off { reason, warn } => {
                assert!(warn, "a failure is said aloud");
                assert!(reason.contains("not available") && reason.contains("Next:"), "{reason}");
            }
            Outcome::Ready(_) => panic!("no sccache was installed, so it cannot be on"),
        }
    }

    /// A build-starting tool never fetches anything: no network and no download in a gate's path.
    #[test]
    fn a_tool_that_starts_builds_never_installs() {
        let home = tempfile::tempdir().unwrap();
        let e = env(&[("PADDOCK_DEV_HOME", home.path().to_str().unwrap())]);
        match prepare(&e, Install::Never) {
            Outcome::Off { reason, warn } => assert!(warn && reason.contains("./dev sccache env"), "{reason}"),
            Outcome::Ready(_) => panic!("nothing is installed"),
        }
        assert!(!home.path().join("tools").exists(), "nothing was made");
    }

    #[cfg(unix)]
    #[test]
    fn a_download_that_does_not_match_its_pin_is_never_installed() {
        let home = tempfile::tempdir().unwrap();
        let base = home.path().join("rel");
        let name = format!("sccache-v{SCCACHE_VERSION}-t");
        let dir = base.join(format!("v{SCCACHE_VERSION}"));
        std::fs::create_dir_all(&dir).unwrap();
        let stage = home.path().join("stage").join(&name);
        std::fs::create_dir_all(&stage).unwrap();
        let packed = paddock_testkit::StandIn::new(&stage, "sccache")
            .stdout("sccache 0.20.1")
            .files()
            .unwrap();
        paddock_testkit::exe::write_program(stage.join(&packed.program_name), packed.bundled()).unwrap();
        let archive = dir.join(format!("{name}.tar.gz"));
        let tar = ToolCall::new("tar")
            .args([
                "-czf",
                &archive.to_string_lossy(),
                "-C",
                &home.path().join("stage").to_string_lossy(),
                &name,
            ])
            .run(&env(&[("PATH", "/usr/bin:/bin")]), &mut std::io::sink())
            .unwrap();
        assert!(tar.success, "{}", tar.stderr);
        let sum = sha256_file(&dir.join(format!("{name}.tar.gz"))).unwrap();
        let e = env(&[("PATH", "/usr/bin:/bin")]);
        let bin = home.path().join("tools").join("sccache");
        std::fs::create_dir_all(&bin).unwrap();
        let bin = bin.join("sccache");
        let url = format!("file://{}", paddock_testkit::path::text(&base));
        let bad = install_from(&bin, &url, "t", "tar.gz", &"0".repeat(64), &e).unwrap_err();
        assert!(bad.contains("checksum mismatch"), "{bad}");
        assert!(!bin.exists(), "nothing is put in place");
        install_from(&bin, &url, "t", "tar.gz", &sum, &e).unwrap();
        assert!(bin.is_file());
        assert_eq!(version_of(&bin, &e).as_deref(), Some(SCCACHE_VERSION));
    }

    /// ER-33 and ER-47: cc-rs wraps `clang-cl` in whatever `RUSTC_WRAPPER` names when its file name is `sccache`, and sccache 0.19 answers
    /// "Compiler not supported" for `clang-cl`. The stand-in `sccache` fails for `clang-cl` the same way; the shim must send that one compile
    /// past it and nothing else.
    #[cfg(unix)]
    mod shim {
        use super::super::*;
        use paddock_testkit::{Actions, Installed, StandIn};

        struct Rig {
            _d: tempfile::TempDir,
            sccache: Installed,
            compilers: Vec<Installed>,
            path: String,
        }

        fn rig() -> Rig {
            let d = tempfile::tempdir().unwrap();
            let dir = |n: &str| {
                let p = d.path().join(n);
                std::fs::create_dir(&p).unwrap();
                p
            };
            let sccache = StandIn::new(&dir("a"), "sccache")
                .when_args(&["clang-cl"], Actions::new().stderr("Compiler not supported").exit(1))
                .records_args()
                .install()
                .unwrap();
            let compilers: Vec<Installed> = ["clang-cl", "clang", "cc"]
                .iter()
                .map(|n| StandIn::new(&dir(n), n).records_args().install().unwrap())
                .collect();
            let path = compilers
                .iter()
                .map(|c| c.dir().display().to_string())
                .collect::<Vec<_>>()
                .join(":");
            write_shim(sccache.program()).unwrap();
            Rig {
                _d: d,
                sccache,
                compilers,
                path,
            }
        }

        impl Rig {
            fn shim(&self) -> PathBuf {
                rustc_wrapper_for(self.sccache.program())
            }

            fn run(&self, args: &[&str]) -> bool {
                let ran = ToolCall::new(self.shim().to_string_lossy())
                    .args(args.iter().copied())
                    .env("PATH", format!("{}:/usr/bin:/bin", self.path))
                    .run(&HashMap::new(), &mut std::io::sink())
                    .unwrap();
                ran.success
            }

            fn seen(&self, which: &str) -> usize {
                let calls = |i: &Installed| i.calls().unwrap().len();
                match which {
                    "sccache" => calls(&self.sccache),
                    other => calls(self.compilers.iter().find(|c| c.dir().ends_with(other)).unwrap()),
                }
            }
        }

        #[test]
        fn the_shim_is_named_sccache_so_native_c_stays_cached_and_runs_the_msvc_compilers_directly() {
            let r = rig();
            assert_eq!(r.shim().file_stem().unwrap(), "sccache");
            // The cross compile cc-rs wraps: run directly, never through sccache.
            for (compiler, args) in [
                ("clang-cl", vec!["clang-cl", "/c", "/Users/x/ring.c"]),
                ("clang", vec!["clang", "--target=x86_64-pc-windows-msvc", "-c", "/Users/x/ring.c"]),
            ] {
                assert!(r.run(&args), "{args:?}");
                assert!(r.seen(compiler) >= 1, "{compiler} did not run");
            }
            assert_eq!(r.seen("sccache"), 0, "a Windows C compile reached sccache");
            // Native C and rustc: sccache gets them, whole.
            assert!(r.run(&["cc", "-c", "/Users/x/sqlite3.c"]));
            assert!(r.run(&["rustc", "--version"]));
            assert!(r.run(&["/some/where/rustc", "--target", "x86_64-pc-windows-msvc"]));
            let calls = r.sccache.calls().unwrap();
            assert_eq!(calls.len(), 3, "{calls:?}");
            assert_eq!(calls[0][0], "cc");
            assert_eq!(calls[2], ["/some/where/rustc", "--target", "x86_64-pc-windows-msvc"]);
            assert_eq!(r.seen("cc"), 0, "native C ran past sccache");
        }

        /// sccache hashes every `CARGO_*` variable of a compile into its key, and CI's jobs have `CARGO_INCREMENTAL=0` and
        /// `CARGO_TERM_COLOR=always` (setup-rust, the workflows). The shim gives sccache those two for a rustc compile, whatever the
        /// caller's environment, so a local key equals CI's; cargo's own environment, and so its output and incremental builds, is not
        /// changed (measured 2026-11-26: with them 166 of 308 keys were CI's stored entries, without 0).
        #[test]
        fn a_rustc_compile_reaches_sccache_with_the_cargo_variables_ci_hashes() {
            let d = tempfile::tempdir().unwrap();
            let dir = d.path().join("a");
            std::fs::create_dir(&dir).unwrap();
            let sccache = StandIn::new(&dir, "sccache")
                .records_env(&["CARGO_INCREMENTAL", "CARGO_TERM_COLOR"])
                .install()
                .unwrap();
            write_shim(sccache.program()).unwrap();
            let shim = rustc_wrapper_for(sccache.program());
            let call = |args: &[&str], envs: &[(&str, &str)]| {
                let mut c = ToolCall::new(shim.to_string_lossy()).args(args.iter().copied());
                for (k, v) in envs {
                    c = c.env(*k, *v);
                }
                assert!(
                    c.env("PATH", "/usr/bin:/bin")
                        .run(&HashMap::new(), &mut std::io::sink())
                        .unwrap()
                        .success
                );
            };
            call(&["rustc", "--version"], &[]);
            call(&["/x/rustc", "-vV"], &[("CARGO_INCREMENTAL", "1"), ("CARGO_TERM_COLOR", "never")]);
            call(&["cc", "-c", "a.c"], &[]);
            let seen = sccache.invocations().unwrap();
            let var = |i: usize, k: &str| seen[i].env.iter().find(|(n, _)| n == k).and_then(|(_, v)| v.clone());
            for i in [0, 1] {
                assert_eq!(var(i, "CARGO_INCREMENTAL").as_deref(), Some("0"), "{seen:?}");
                assert_eq!(var(i, "CARGO_TERM_COLOR").as_deref(), Some("always"), "{seen:?}");
            }
            assert_eq!(var(2, "CARGO_INCREMENTAL"), None, "a C compile keeps the caller's environment");
        }

        /// Positive control: the stand-in sccache itself fails for `clang-cl`, so the test above can see the failure it guards.
        #[test]
        fn naming_sccache_itself_fails_the_clang_cl_compile() {
            let r = rig();
            let ran = ToolCall::new(r.sccache.program().to_string_lossy())
                .args(["clang-cl", "/c", "/Users/x/ring.c"])
                .run(&HashMap::new(), &mut std::io::sink())
                .unwrap();
            assert!(!ran.success && ran.stderr.contains("Compiler not supported"), "{ran:?}");
        }

        #[test]
        fn a_gate_never_writes_the_shim_and_a_missing_one_is_a_reason_with_a_next_step() {
            let d = tempfile::tempdir().unwrap();
            let bin = d.path().join("sccache");
            std::fs::write(&bin, "x").unwrap();
            let why = check_shim(&bin).unwrap_err();
            assert!(why.contains("Next: run `./dev sccache env`"), "{why}");
            assert!(!d.path().join(SHIM_DIR).exists(), "the check wrote it");
            write_shim(&bin).unwrap();
            check_shim(&bin).unwrap();
            std::fs::write(rustc_wrapper_for(&bin), "#!/bin/sh\nexec old\n").unwrap();
            assert!(check_shim(&bin).is_err(), "an out-of-date shim is not accepted");
        }
    }

    #[test]
    fn off_unix_the_wrapper_is_the_binary_itself_and_on_unix_a_shim_beside_it() {
        let bin = Path::new("/t/sccache/0.20.1/sccache");
        assert_eq!(shim_for(bin, false), bin);
        assert_eq!(shim_for(bin, true), Path::new("/t/sccache/0.20.1/shim/sccache"));
    }

    #[test]
    fn an_existing_wrapper_that_is_sccache_itself_is_declined_with_the_cc_rs_cause() {
        let e = env(&[("RUSTC_WRAPPER", "/opt/bin/sccache")]);
        let why = declined(&e).unwrap();
        assert!(
            why.contains("cc-rs") && why.contains("clang-cl") && why.contains("./dev sccache env"),
            "{why}"
        );
        let ours = declined(&env(&[("RUSTC_WRAPPER", "/t/sccache/0.20.1/shim/sccache")])).unwrap();
        assert!(!ours.contains("cc-rs"), "our own shim is not the cause: {ours}");
        // Positive control: another wrapper gets the short message.
        let other = declined(&env(&[("RUSTC_WRAPPER", "/opt/bin/other")])).unwrap();
        assert!(!other.contains("cc-rs"), "{other}");
    }

    /// The shim policy was written for cc-rs 1.8 to 1.11 ([`CC_RANGE`]); a lockfile outside it needs its behaviour re-read first.
    fn cc_outside_range(lock: &str) -> Vec<String> {
        let doc: toml::Table = lock.parse().unwrap();
        let ((lo_major, lo_minor), (hi_major, hi_minor)) = CC_RANGE;
        doc.get("package")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
            .filter(|p| p.get("name").and_then(|n| n.as_str()) == Some("cc"))
            .filter_map(|p| p.get("version").and_then(|v| v.as_str()))
            .filter(|v| {
                let mut it = v.split('.').map(|n| n.parse::<u64>().unwrap_or(u64::MAX));
                let at = (it.next().unwrap_or(0), it.next().unwrap_or(0));
                !((lo_major, lo_minor) <= at && at <= (hi_major, hi_minor))
            })
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn the_range_check_sees_a_cc_outside_it() {
        let lock = |v: &str| format!("[[package]]\nname = \"cc\"\nversion = \"{v}\"\n");
        assert_eq!(cc_outside_range(&lock("1.12.0")), ["1.12.0"]);
        assert_eq!(cc_outside_range(&lock("1.7.9")), ["1.7.9"]);
        assert!(cc_outside_range(&lock("1.8.0")).is_empty() && cc_outside_range(&lock("1.11.1")).is_empty());
    }

    #[test]
    fn every_lockfile_of_the_repository_has_a_cc_inside_the_documented_range() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let listed = crate::plan_repo::git_text(&root, &["ls-files", "*Cargo.lock"]).expect("git lists the lockfiles");
        let locks: Vec<&str> = listed.lines().collect();
        assert!(locks.len() > 5, "the lockfiles were not found: {locks:?}");
        for l in locks {
            let text = std::fs::read_to_string(root.join(l)).unwrap();
            let out = cc_outside_range(&text);
            assert!(
                out.is_empty(),
                "{l} locks cc {out:?}, outside {CC_RANGE:?}: re-read local_sccache::shim_text against that cc-rs, then widen CC_RANGE"
            );
        }
    }

    /// A stand-in S3 health endpoint that answers exactly one connection with `status` (a blocking accept: nothing sleeps and nothing
    /// polls). Returns its port and the thread, which the test joins after the one check it is for.
    fn health_server(status: &'static str) -> (u16, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let thread = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut buf = [0u8; 256];
            let _ = c.read(&mut buf);
            let _ = c.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes());
        });
        (port, thread)
    }

    /// An environment with a credentials file of invented keys (mode 600) and the store at `port`.
    fn s3_env(home: &Path, port: u16) -> HashMap<String, String> {
        let file = home.join("sccache-s3.env");
        std::fs::write(&file, "AWS_ACCESS_KEY_ID=fake-id-1\nAWS_SECRET_ACCESS_KEY=fake-secret-1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        env(&[
            ("PATH", "/usr/bin:/bin"),
            ("PADDOCK_SCCACHE_ENV_FILE", file.to_str().unwrap()),
            ("PADDOCK_SCCACHE_S3_ENDPOINT", format!("127.0.0.1:{port}").as_str()),
            ("SCCACHE_DIR", home.join("cache").to_str().unwrap()),
        ])
    }

    #[test]
    fn the_health_check_wants_a_2xx_answer_and_not_just_a_connect() {
        // curl on Windows cannot open a socket without SystemRoot (Winsock loads its providers from it): the child gets what the
        // caller has (ER-47), so the test's caller has it, as production's does. Without it every connect failed at once, "exit 7
        // after 0 ms", against a listener that was already bound (train 19 trial, run 8560).
        let mut e = env(&[("PATH", "/usr/bin:/bin")]);
        e.extend(
            paddock_proc::KEPT_ENV
                .iter()
                .filter(|k| **k != "PATH")
                .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v))),
        );
        let probe = |port: u16| {
            s3_alive(
                &S3 {
                    bucket: "b".into(),
                    endpoint: format!("127.0.0.1:{port}"),
                    region: "auto".into(),
                    ssl: false,
                    prefix: "p".into(),
                },
                &e,
            )
        };
        let (up, served_up) = health_server("200 OK");
        let (half, served_half) = health_server("503 Service Unavailable");
        assert_eq!(probe(up), Ok(()));
        let why = probe(half).unwrap_err();
        assert!(why.contains("503"), "{why}");
        assert!(probe(free_port()).is_err(), "nothing listens there");
        served_up.join().unwrap();
        served_half.join().unwrap();
    }

    /// The default (user decision 2026-11-21): the local disk first, RustFS behind it, when the file is there and the store answers.
    #[test]
    fn the_default_is_the_local_disk_with_the_shared_store_behind_it() {
        let home = tempfile::tempdir().unwrap();
        let e = s3_env(home.path(), 1);
        let (b, why) = choose(&e, &|_| Ok(())).unwrap();
        assert_eq!(why, None);
        let Backend::Tiered { dir, s3, .. } = &b else {
            panic!("expected the tiered backend, got {b:?}")
        };
        assert_eq!(
            dir,
            &format!("{}-s3", home.path().join("cache").display()),
            "a sibling folder, never the plain one"
        );
        assert_eq!((s3.bucket.as_str(), s3.endpoint.as_str()), (DEFAULT_S3_BUCKET, "127.0.0.1:1"));
        let server: HashMap<_, _> = b.vars().into_iter().collect();
        assert_eq!(server["SCCACHE_MULTILEVEL_CHAIN"], "disk,s3");
        assert_eq!(server["SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY"], "l0");
        // The builds see the disk only: no chain, no bucket, no key (a server a later rustc starts serves from the disk).
        let build: Vec<String> = b.build_vars().into_iter().map(|(k, _)| k).collect();
        assert_eq!(build, ["SCCACHE_DIR", "SCCACHE_CACHE_SIZE"]);
        let all = format!("{:?}{:?}", b.vars(), b.build_vars());
        assert!(!all.contains("fake-"), "a key is never a setting: {all}");
        // The explicit choices are as asked, and never ask the store.
        for (name, want_local) in [("local", true), ("s3", false)] {
            let mut e2 = e.clone();
            e2.insert("PADDOCK_SCCACHE_BACKEND".into(), name.into());
            e2.insert("PADDOCK_SCCACHE_S3_BUCKET".into(), "b".into());
            let (b2, why2) = choose(&e2, &|_| panic!("the store is not asked for `{name}`")).unwrap();
            assert_eq!((matches!(b2, Backend::Local { .. }), why2), (want_local, None));
        }
    }

    #[test]
    fn without_the_credentials_file_or_with_a_store_that_is_down_the_disk_is_used_and_the_cause_is_named() {
        let home = tempfile::tempdir().unwrap();
        // No file: a choice, said as a notice.
        let none = env(&[
            ("PADDOCK_SCCACHE_ENV_FILE", home.path().join("absent.env").to_str().unwrap()),
            ("SCCACHE_DIR", "/c"),
        ]);
        let (b, f) = choose(&none, &|_| panic!("no credentials, so the store is not asked")).unwrap();
        assert!(matches!(b, Backend::Local { .. }));
        let f = f.unwrap();
        assert!(!f.warn && f.why.contains("no S3 credentials"), "{f:?}");
        // A file but a store that does not answer: a warning that carries the probe's cause.
        let e = s3_env(home.path(), 1);
        let (b, f) = choose(&e, &|_| Err("the S3 health check http://x answered `HTTP/1.1 503`, not 2xx".into())).unwrap();
        assert!(matches!(b, Backend::Local { .. }));
        let f = f.unwrap();
        assert!(f.warn && f.why.contains("503"), "{f:?}");
        assert!(!format!("{f:?}").contains("fake-"), "a key is never in a cause");
        // A file others can read is refused with its next step, and the disk is used.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file = e["PADDOCK_SCCACHE_ENV_FILE"].clone();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            let (b, f) = choose(&e, &|_| Ok(())).unwrap();
            assert!(matches!(b, Backend::Local { .. }));
            assert!(f.unwrap().why.contains("chmod 600"));
        }
    }

    /// `eval "$(./dev sccache env)"` with and without the config file, against a stand-in store and a stand-in sccache.
    #[cfg(unix)]
    #[test]
    fn sccache_env_uses_the_shared_store_when_the_file_is_there_and_the_disk_alone_when_not() {
        let chain = format!(
            "{MULTILEVEL_LOCATION}\n  L0 (disk)                     Local disk: \"{{env:SCCACHE_DIR}}\"\n  L1 (s3)                       s3, name: {{env:SCCACHE_BUCKET}}, prefix: /{{env:SCCACHE_S3_KEY_PREFIX}}/"
        );
        let (home, mut e, bin) = fake(0, Some(&chain));
        let (port, served) = health_server("200 OK");
        e.extend(s3_env(home.path(), port));
        let Outcome::Ready(ready) = prepare(&e, Install::Never) else {
            panic!("not ready")
        };
        let dir = |r: &Ready| r.vars.iter().find(|(k, _)| k == "SCCACHE_DIR").map(|(_, v)| v.clone()).unwrap();
        assert!(dir(&ready).ends_with("cache-s3"), "{ready:?}");
        let lines = shell_lines(&ready).join("\n");
        assert!(
            !lines.contains("fake-") && !lines.contains("AWS_ACCESS") && !lines.contains("AWS_SECRET"),
            "the keys go to the server only: {lines}"
        );
        assert!(calls(&bin).contains(&"--start-server".to_string()));
        served.join().unwrap();
        // Without the file: the plain disk folder, no failure.
        let (_h2, mut e2, _bin2) = fake(0, None);
        e2.insert(
            "PADDOCK_SCCACHE_ENV_FILE".into(),
            _h2.path().join("absent.env").display().to_string(),
        );
        let Outcome::Ready(plain) = prepare(&e2, Install::Never) else {
            panic!("not ready")
        };
        assert!(dir(&plain).ends_with("cache"), "{plain:?}");
    }

    /// The part of a two-level server's `--show-adv-stats` that comes before the cache location, as sccache 0.19.0 prints it
    /// (measured on a running server, 2026-11-22): the per-level counter lines, which start with the same labels as the level
    /// description lines further down. A fixture without them cannot tell a reader of the descriptions from a reader of the counters.
    const CHAIN_COUNTERS: &str = "Unsupported compiler calls            0\nAverage cache write               0.038 s\nMulti-level cache levels              2\n  L0 (disk) hits                      0\n  L0 (disk) misses                  179\n  L0 (disk) hit rate               0.00 %\n  L0 (disk) writes                  179\n  L0 (disk) write failures            0\n  L0 (disk) backfills from            0\n  L0 (disk) backfills to              0\n  L0 (disk) avg cache write       0.000 s\n  L0 (disk) avg cache read hit    0.000 s\n  L1 (s3) hits                        0\n  L1 (s3) misses                    179\n  L1 (s3) hit rate                 0.00 %\n  L1 (s3) writes                    179\n  L1 (s3) write failures              0\n  L1 (s3) backfills from              0\n  L1 (s3) backfills to                0\n  L1 (s3) avg cache write         0.052 s\n  L1 (s3) avg cache read hit      0.000 s\n\nNon-cacheable reasons:\nincremental                          17\n\n";

    /// The stats of a server started with the two-level chain, as sccache 0.19.0 prints them (measured): the counters, then the
    /// location line, then one description line per level.
    fn chain_stats(l0_dir: &str, bucket: &str, prefix: &str) -> String {
        format!(
            "{CHAIN_COUNTERS}Cache location                  Multi-level (2 levels)\n  L0 (disk)                     Local disk: \"{l0_dir}\"\n  L1 (s3)                       s3, name: {bucket}, prefix: /{prefix}/\nEffective cache mode            ReadWrite\n"
        )
    }

    /// A multi-level server is ours only when its disk folder, bucket and prefix are ours (ER-33): the location line alone names no setup.
    #[test]
    fn a_two_level_server_is_ours_only_with_this_folder_bucket_and_prefix() {
        let home = tempfile::tempdir().unwrap();
        let e = s3_env(home.path(), 1);
        let (b, _) = choose(&e, &|_| Ok(())).unwrap();
        let Backend::Tiered { dir, s3, .. } = &b else {
            panic!("tiered expected")
        };
        assert!(
            b.matches_stats(&chain_stats(dir, &s3.bucket, &s3.prefix)),
            "the control: all three are ours"
        );
        assert!(
            !b.matches_stats(&chain_stats("/elsewhere", &s3.bucket, &s3.prefix)),
            "another disk folder"
        );
        assert!(!b.matches_stats(&chain_stats(dir, "other-bucket", &s3.prefix)), "another bucket");
        assert!(!b.matches_stats(&chain_stats(dir, &s3.bucket, "other")), "another prefix");
        assert!(
            !b.matches_stats(&format!("{CHAIN_COUNTERS}Cache location                  Multi-level (2 levels)\n")),
            "the location line alone says nothing"
        );
        assert!(
            !b.matches_stats(&format!("{CHAIN_COUNTERS}Cache location                  Multi-level (2 levels)\n  L0 (disk)                     Local disk: \"{dir}\"\n  L1 (redis)                    Redis: x\n")),
            "another second level"
        );
    }

    /// RustFS was down at the first `sccache env`, so a plain-disk server was started; once it is back the next call replaces that server
    /// (it is this tool's own: its folder is the plain one this tool uses) instead of leaving every build uncached for good.
    #[cfg(unix)]
    #[test]
    fn a_plain_disk_server_started_while_the_store_was_down_is_replaced_when_it_is_back() {
        use paddock_testkit::{Actions, Case, StandIn};
        let home = tempfile::tempdir().unwrap();
        let mut e = s3_env(home.path(), free_port());
        e.insert("PADDOCK_DEV_HOME".into(), home.path().to_str().unwrap().into());
        e.insert("PATH".into(), "/usr/bin:/bin".into());
        e.insert("SCCACHE_SERVER_PORT".into(), free_port().to_string());
        let plain = home.path().join("cache").display().to_string();
        let bin = install_path(&e).unwrap();
        let dir = bin.parent().unwrap();
        std::fs::create_dir_all(dir).unwrap();
        let stopped = dir.join("stopped");
        let rw = |a: Actions| a.stdout("Effective cache mode            ReadWrite");
        let installed = StandIn::new(dir, "sccache")
            .records_args()
            .stdout("sccache 0.20.1")
            .when_args(&["--start-server"], Actions::new())
            .when_args(&["--stop-server"], Actions::new().writes(&stopped, ""))
            .when(
                &Case::new().args_start_with(&["--show-adv-stats"]).file_missing(&stopped),
                rw(Actions::new().stdout_with(&format!("Cache location                  Local disk: \"{plain}\""))),
            )
            .when(
                &Case::new().args_start_with(&["--show-adv-stats"]).file_exists(&stopped),
                rw(Actions::new()
                    .stdout_with(&format!("{CHAIN_COUNTERS}Cache location                  Multi-level (2 levels)"))
                    .stdout_with("  L0 (disk)                     Local disk: \"{env:SCCACHE_DIR}\"")
                    .stdout_with("  L1 (s3)                       s3, name: {env:SCCACHE_BUCKET}, prefix: /{env:SCCACHE_S3_KEY_PREFIX}/")),
            )
            .install()
            .unwrap();
        write_shim(installed.program()).unwrap();
        let dir_of = |o: &Outcome| match o {
            Outcome::Ready(r) => r.vars.iter().find(|(k, _)| k == "SCCACHE_DIR").map(|(_, v)| v.clone()).unwrap(),
            other => panic!("not ready: {other:?}"),
        };
        // 1. The store is down: the plain disk, and a server on it.
        assert_eq!(dir_of(&prepare(&e, Install::Never)), plain);
        assert!(!calls(&installed).contains(&"--stop-server".to_string()));
        // 2. The store is back: the plain server is stopped and started again with the chain.
        let (port, served) = health_server("200 OK");
        e.insert("PADDOCK_SCCACHE_S3_ENDPOINT".into(), format!("127.0.0.1:{port}"));
        let up = prepare(&e, Install::Never);
        assert_eq!(dir_of(&up), format!("{plain}-s3"), "now the chain's disk folder");
        let c = calls(&installed);
        assert_eq!(c.iter().filter(|a| *a == "--stop-server").count(), 1, "one stop: {c:?}");
        assert_eq!(
            &c[c.len() - 2..],
            ["--stop-server", "--start-server"],
            "stopped, then started with the chain: {c:?}"
        );
        served.join().unwrap();
    }

    /// The store's endpoint and bucket may come from the credentials file (optional `PADDOCK_SCCACHE_S3_ENDPOINT` and
    /// `PADDOCK_SCCACHE_S3_BUCKET` lines), after the environment and before the defaults.
    #[test]
    fn the_credentials_file_may_name_the_store_and_the_environment_wins() {
        let home = tempfile::tempdir().unwrap();
        let mut e = s3_env(home.path(), 1);
        e.remove("PADDOCK_SCCACHE_S3_ENDPOINT");
        let file = e["PADDOCK_SCCACHE_ENV_FILE"].clone();
        let keys = std::fs::read_to_string(&file).unwrap();
        std::fs::write(
            &file,
            format!("{keys}PADDOCK_SCCACHE_S3_ENDPOINT=http://198.51.100.7:9000\nPADDOCK_SCCACHE_S3_BUCKET=from-file\nOTHER=ignored\n"),
        )
        .unwrap();
        let store = |e: &HashMap<String, String>| match choose(e, &|_| Ok(())).unwrap().0 {
            Backend::Tiered { s3, .. } => (s3.endpoint, s3.bucket),
            other => panic!("expected the tiered backend, got {other:?}"),
        };
        assert_eq!(
            store(&e),
            ("198.51.100.7:9000".to_string(), "from-file".to_string()),
            "the file names the store"
        );
        e.insert("PADDOCK_SCCACHE_S3_BUCKET".into(), "from-env".into());
        assert_eq!(store(&e).1, "from-env", "the environment outranks the file");
        // Neither: the defaults.
        std::fs::write(&file, keys).unwrap();
        e.remove("PADDOCK_SCCACHE_S3_BUCKET");
        assert_eq!(store(&e), (DEFAULT_S3_ENDPOINT.to_string(), DEFAULT_S3_BUCKET.to_string()));
    }

    /// The key file is the machine's declared key: a stray `AWS_*` in the caller's shell (another project's login) never outranks it, and
    /// is said once; with no file the caller's variables are still used.
    #[test]
    fn the_key_files_keys_outrank_the_callers_aws_variables() {
        let home = tempfile::tempdir().unwrap();
        let mut e = s3_env(home.path(), 1);
        e.insert("AWS_ACCESS_KEY_ID".into(), "stray-id".into());
        e.insert("AWS_SECRET_ACCESS_KEY".into(), "stray-secret".into());
        let c = read_creds(&e).unwrap();
        assert_eq!(
            c.pairs(),
            [
                ("AWS_ACCESS_KEY_ID".to_string(), "fake-id-1".to_string()),
                ("AWS_SECRET_ACCESS_KEY".to_string(), "fake-secret-1".to_string())
            ]
        );
        assert!(c.env_ignored(), "the stray variables are said to be ignored");
        assert!(!format!("{c:?}").contains("stray"), "no value in Debug");
        let mut none = e.clone();
        none.insert(
            "PADDOCK_SCCACHE_ENV_FILE".into(),
            home.path().join("absent.env").display().to_string(),
        );
        let c = read_creds(&none).unwrap();
        assert_eq!(
            c.pairs()[0],
            ("AWS_ACCESS_KEY_ID".to_string(), "stray-id".to_string()),
            "no file: the caller's variables"
        );
        assert!(!c.env_ignored());
    }

    /// The keys reach the server's start call and nothing else.
    #[test]
    fn the_keys_reach_the_servers_environment_only() {
        let home = tempfile::tempdir().unwrap();
        let e = s3_env(home.path(), 1);
        let (b, _) = choose(&e, &|_| Ok(())).unwrap();
        let creds = read_creds(&e).unwrap();
        let server = Server {
            bin: Path::new("/h/tools/sccache/0.20.1/sccache"),
            run: Path::new("/h/sccache-run"),
            backend: &b,
            creds: Some(&creds),
            log_level: "warn".into(),
            basedirs: Vec::new(),
        };
        let child = server.call("--start-server").child_env(&env(&[("PATH", "/usr/bin")]));
        assert_eq!(last(&child, "AWS_ACCESS_KEY_ID").as_deref(), Some("fake-id-1"));
        assert_eq!(last(&child, "SCCACHE_MULTILEVEL_CHAIN").as_deref(), Some("disk,s3"));
        assert_eq!(last(&child, "AWS_EC2_METADATA_DISABLED").as_deref(), Some("true"));
        assert!(!format!("{creds:?}").contains("fake-"), "Debug masks the keys");
    }

    #[test]
    fn the_server_and_the_build_get_the_checkout_as_a_base_folder_and_keep_the_manifest_paths_verbatim() {
        let roots = vec![
            "/v/wt/a".to_string(),
            "/v/wt/b/".to_string(),
            "/v/wt/a".to_string(),
            "rel/c".to_string(),
            String::new(),
        ];
        let vars = basedir_vars(&roots, false);
        assert_eq!(
            vars,
            [
                ("SCCACHE_BASEDIRS".to_string(), "/v/wt/a:/v/wt/b".to_string()),
                (
                    "SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS".to_string(),
                    "CARGO_MANIFEST_DIR,CARGO_MANIFEST_PATH".to_string()
                )
            ],
            "absolute, once each, no trailing slash, ':' on a unix host; a relative path would stop the server from starting"
        );
        // The build's variables and the server's own start carry both.
        let local = backend(&env(&[("SCCACHE_DIR", "/c"), ("PADDOCK_SCCACHE_BACKEND", "local")])).unwrap();
        let build = ready_vars(Path::new("/b/sccache"), &local, &vars);
        assert!(
            build.iter().any(|(k, v)| k == "SCCACHE_BASEDIRS" && v == "/v/wt/a:/v/wt/b"),
            "{build:?}"
        );
        assert!(build.iter().any(|(k, _)| k == "SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS"), "{build:?}");
        let server = Server {
            bin: Path::new("/b/sccache"),
            run: Path::new("/h/run"),
            backend: &local,
            creds: None,
            log_level: "warn".into(),
            basedirs: vars,
        };
        let child = server.call("--start-server").child_env(&env(&[("PATH", "/usr/bin")]));
        assert_eq!(last(&child, "SCCACHE_BASEDIRS").as_deref(), Some("/v/wt/a:/v/wt/b"));
        assert_eq!(
            last(&child, "SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS").as_deref(),
            Some("CARGO_MANIFEST_DIR,CARGO_MANIFEST_PATH")
        );
    }

    #[test]
    fn on_windows_the_bases_are_drive_paths_joined_with_a_semicolon_and_unix_paths_are_refused() {
        let vars = basedir_vars(
            &[
                r"C:\ci\src\paddock-s1".to_string(),
                "D:/target/x-s1/".to_string(),
                "/c/git/bash".to_string(),
            ],
            true,
        );
        assert_eq!(vars[0].1, r"C:\ci\src\paddock-s1;D:/target/x-s1");
        assert!(
            basedir_vars(&["/c/only".to_string()], true).is_empty(),
            "a Git-bash path is not absolute to sccache on Windows"
        );
        assert!(basedir_vars(&[], false).is_empty(), "no root, no variables");
    }

    #[test]
    fn the_local_server_lists_the_worktree_and_its_sibling_worktrees() {
        let t = tempfile::tempdir().unwrap();
        for d in ["wt/a/crates", "wt/b", "wt/not-a-worktree"] {
            std::fs::create_dir_all(t.path().join(d)).unwrap();
        }
        std::fs::write(t.path().join("wt/a/.git"), "gitdir: x").unwrap();
        std::fs::write(t.path().join("wt/b/.git"), "gitdir: y").unwrap();
        // One component per join: a joined "wt/a/crates" keeps its `/`, so on Windows the root it finds prints as `...\wt/a`.
        let roots = worktree_roots(&t.path().join("wt").join("a").join("crates"));
        let root = |n: &str| t.path().join("wt").join(n).display().to_string();
        assert_eq!(roots, [root("a"), root("b")]);
    }
}
