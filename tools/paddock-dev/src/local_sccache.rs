//! A local sccache for agents' Rust builds (user decision 2026-11-16; docs/research/sccache-basedirs-2026-11-16.md).
//!
//! `RUSTC_WRAPPER` is set by this tool for the builds it starts (the push gates, `./dev gen check`, heavy's own build) and by
//! `eval "$(./dev sccache env)"` for an agent's own cargo calls; never in the user's shell. One server on sccache's default port,
//! started on demand, with sccache's local disk cache (`SCCACHE_DIR`, `SCCACHE_CACHE_SIZE` from the environment, which the
//! machine's shell profile already sets). No service. The backend is one setting, `PADDOCK_SCCACHE_BACKEND`: `local` (the default)
//! or `s3` (sccache's own S3 support, for a shared cache such as RustFS; [`S3`]). The server is started with a listed environment
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

/// The server's port: the caller's `SCCACHE_SERVER_PORT`, else sccache's default.
pub fn server_port(env: &HashMap<String, String>) -> String {
    env.get("SCCACHE_SERVER_PORT")
        .filter(|p| !p.is_empty())
        .cloned()
        .unwrap_or_else(|| "4226".into())
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
        Some(format!("RUSTC_WRAPPER is already set ({})", env["RUSTC_WRAPPER"]))
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
}

impl Backend {
    /// The settings the server runs with and the builds see (never a credential).
    pub fn vars(&self) -> Vec<(String, String)> {
        match self {
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

    /// A line for a person: no credential in it.
    pub fn describe(&self) -> String {
        match self {
            Backend::Local { dir, size } => format!("local disk {dir} ({size})"),
            Backend::S3(s) => format!("S3 {} bucket {} prefix {}", s.endpoint, s.bucket, s.prefix),
        }
    }

    /// Whether `--show-stats` says the running server uses this very cache (a server with another backend, bucket, prefix or folder is
    /// somebody else's, and is left alone). The comparison is exact: the quoted folder as sccache prints it
    /// (`Local disk: "<dir>"`), and for S3 `s3, name: <bucket>, prefix: /<prefix>/`. A prefix of its own never matches another's, and
    /// the folder `/a/b` never matches `/a/b2` or `/x/a/b`. (sccache's stats do not print the S3 endpoint.)
    pub fn matches_stats(&self, stats: &str) -> bool {
        let want = match self {
            Backend::Local { dir, .. } => format!("Local disk: \"{dir}\""),
            Backend::S3(s) => format!("s3, name: {}, prefix: /{}/", s.bucket, s.prefix.trim_matches('/')),
        };
        stats
            .lines()
            .any(|l| l.strip_prefix("Cache location").is_some_and(|rest| rest.trim() == want))
    }
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
        other => Err(format!("PADDOCK_SCCACHE_BACKEND is `{other}`; it is local or s3")),
    }
}

/// The S3 credentials: where they are read from, and whether they are there. A value never appears in this type's `Debug` output
/// or in any message.
#[derive(Clone, PartialEq, Eq)]
pub struct Creds {
    pairs: Vec<(String, String)>,
}

impl std::fmt::Debug for Creds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Creds({})", self.status())
    }
}

impl Creds {
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

/// Reads the credentials: the caller's `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` (for example exported from the macOS
/// keychain by the user: `security find-generic-password -s paddock-sccache-s3 -w`), else the env file. A file that group or others
/// can read is refused (it holds a secret).
pub fn read_creds(env: &HashMap<String, String>) -> Result<Creds, String> {
    let from_env: Vec<(String, String)> = ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"]
        .iter()
        .filter_map(|k| env.get(*k).filter(|v| !v.is_empty()).map(|v| (k.to_string(), v.clone())))
        .collect();
    let c = Creds { pairs: from_env };
    if c.status() == "configured" {
        return Ok(c);
    }
    let file = env_file(env)?;
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Creds { pairs: Vec::new() }),
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
    Ok(Creds {
        pairs: parse_env_file(&text),
    })
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
}

impl Server<'_> {
    fn call(&self, arg: &str) -> ToolCall {
        let run = self.run.to_string_lossy();
        let mut c = ToolCall::new(self.bin.to_string_lossy())
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
            // The server never exits on its own: a build that finds none would start one with the build's environment (and, for S3,
            // without the keys), and that one would replace ours.
            .env("SCCACHE_IDLE_TIMEOUT", "0")
            .env("SCCACHE_IGNORE_SERVER_IO_ERROR", "1")
            .timeout(Duration::from_secs(60));
        for (k, v) in self.backend.vars() {
            c = c.env(k, v);
        }
        if matches!(self.backend, Backend::S3(_)) {
            // No instance-metadata lookup: with no keys it hangs for about 75 s before it fails.
            c = c.env("AWS_EC2_METADATA_DISABLED", "true");
        }
        // Only the server needs the keys, and the logged command masks them.
        for (k, v) in self.creds.map(Creds::pairs).unwrap_or_default() {
            c = c.env(k.as_str(), v.as_str()).secret(v.as_str());
        }
        c
    }
}

/// The variables a build gets: the wrapper, the settings the server runs with and the two that keep a server a build starts itself
/// from replacing ours or hanging (never a credential).
fn ready_vars(bin: &Path, backend: &Backend) -> Vec<(String, String)> {
    let mut vars = vec![("RUSTC_WRAPPER".to_string(), bin.display().to_string())];
    vars.extend(backend.vars());
    vars.push(("SCCACHE_IGNORE_SERVER_IO_ERROR".into(), "1".into()));
    vars.push(("SCCACHE_IDLE_TIMEOUT".into(), "0".into()));
    if matches!(backend, Backend::S3(_)) {
        vars.push(("AWS_EC2_METADATA_DISABLED".into(), "true".into()));
    }
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
    let backend = match backend(env) {
        Ok(b) => b,
        Err(e) => {
            return off(
                format!("{e}. Next: fix the PADDOCK_SCCACHE_* settings (docs/dev-setup.md, \"Local sccache\")"),
                true,
            );
        }
    };
    let creds = match &backend {
        Backend::S3(_) => match read_creds(env) {
            Ok(c) if c.status() == "configured" => Some(c),
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
    if let Backend::Local { dir, .. } = &backend
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return off(format!("cannot make the cache folder {dir}: {e}. Next: fix SCCACHE_DIR"), true);
    }
    let log = error_log(&run);
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
    loop {
        let start_log = run.join(format!("start-{}.log", std::process::id()));
        // The server outlives this process and would hold a pipe open, so its starter writes to a file.
        let start = server
            .call("--start-server")
            .output(Output::File(start_log.clone()))
            .run(env, &mut std::io::sink())
            .map(|mut s| {
                s.success |= already_running(&s);
                s
            });
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
        if !backend.matches_stats(&stats.stdout) {
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
        if restarted {
            return off(
                format!(
                    "the sccache server came up read-only twice: its startup write check against the cache failed, and sccache 0.19.0 keeps that result for the server's whole life, so no compile would be stored. {where_log}. Next: read the log for the failed write (the bucket, the key's rights, the folder's permissions), fix it, then `sccache --stop-server`"
                ),
                true,
            );
        }
        restarted = true;
        eprintln!("local-sccache: the server came up read-only; stopping it (it is this cache's own) and starting it once more");
        let _ = server.call("--stop-server").run(env, &mut std::io::sink());
    }
    eprintln!(
        "local-sccache: sccache {SCCACHE_VERSION} on, {}{}, ready in {:.1} s; server log {}",
        backend.describe(),
        creds.as_ref().map_or(String::new(), |c| format!(", credentials {}", c.status())),
        started.elapsed().as_secs_f64(),
        log.display()
    );
    Outcome::Ready(Ready {
        vars: ready_vars(&bin, &backend),
        bin,
    })
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
        assert_eq!(cache_dir(&e).unwrap(), "/h/sccache");
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
            bin: Path::new("/h/tools/sccache/0.19.0/sccache"),
            run: Path::new("/h/sccache-run"),
            backend,
            creds: None,
            log_level: "warn".into(),
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
            last(&child, "SCCACHE_ERROR_LOG").as_deref(),
            Some("/h/sccache-run/sccache.log"),
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
            let vars = ready_vars(Path::new("/b/sccache"), b);
            assert!(vars.contains(&("SCCACHE_IDLE_TIMEOUT".into(), "0".into())), "{vars:?}");
            assert!(
                vars.iter()
                    .all(|(k, _)| !k.starts_with("AWS_ACCESS") && !k.starts_with("AWS_SECRET")),
                "never a key"
            );
        }
        assert!(ready_vars(Path::new("/b/sccache"), &s3).contains(&("AWS_EC2_METADATA_DISABLED".into(), "true".into())));
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

    /// A stand-in sccache: `--version`, start and stop count in files beside it, and the stats say read-only for the first
    /// `readonly-starts` starts (sccache 0.19.0 keeps a failed startup write check for the server's whole life).
    #[cfg(unix)]
    fn fake(readonly_starts: u32, foreign_location: Option<&str>) -> (tempfile::TempDir, HashMap<String, String>, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let e = env(&[
            ("PADDOCK_DEV_HOME", home.path().to_str().unwrap()),
            ("SCCACHE_DIR", home.path().join("cache").to_str().unwrap()),
            ("PATH", "/usr/bin:/bin"),
        ]);
        let bin = install_path(&e).unwrap();
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        let script = r#"#!/bin/sh
d=$(dirname "$0")
echo "$1" >> "$d/calls"
case "$1" in
  --version) echo "sccache 0.19.0";;
  --start-server) n=$(cat "$d/starts" 2>/dev/null || echo 0); echo $((n+1)) > "$d/starts";;
  --stop-server) ;;
  --show-adv-stats)
    n=$(cat "$d/starts" 2>/dev/null || echo 0); ro=$(cat "$d/readonly-starts" 2>/dev/null || echo 0)
    mode=ReadWrite; [ "$n" -le "$ro" ] && mode=ReadOnly
    if [ -f "$d/location" ]; then cat "$d/location"; else echo "Cache location                  Local disk: \"$SCCACHE_DIR\""; fi
    echo "Effective cache mode            $mode";;
esac
exit 0
"#;
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(bin.with_file_name("readonly-starts"), readonly_starts.to_string()).unwrap();
        if let Some(l) = foreign_location {
            std::fs::write(bin.with_file_name("location"), format!("Cache location                  {l}\n")).unwrap();
        }
        (home, e, bin)
    }

    #[cfg(unix)]
    fn calls(bin: &Path) -> Vec<String> {
        std::fs::read_to_string(bin.with_file_name("calls"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("--start") || l.starts_with("--stop"))
            .map(str::to_string)
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

    #[cfg(unix)]
    #[test]
    fn a_read_write_server_is_left_running() {
        let (_h, e, bin) = fake(0, None);
        assert!(matches!(prepare(&e, Install::Never), Outcome::Ready(_)));
        assert_eq!(calls(&bin), ["--start-server"]);
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
            ("PADDOCK_SCCACHE_S3_ENDPOINT", "172.16.1.220:15000"),
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
        let c = Creds { pairs: got };
        assert_eq!(c.status(), "configured");
        let shown = format!("{c:?}");
        assert!(!shown.contains("AKIAEXAMPLE") && !shown.contains("s3cr3t"), "{shown}");
        assert_eq!(Creds { pairs: Vec::new() }.status(), "missing");
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
        std::fs::write(stage.join("sccache"), "#!/bin/sh\necho sccache 0.19.0\n").unwrap();
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
        assert_eq!(version_of(&bin, &e).as_deref(), Some("0.19.0"));
    }
}
