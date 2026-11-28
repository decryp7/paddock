//! `dev ci setup-rust`: the compile cache and the Tauri Linux libraries of a CI job, the port of the shell steps "Install
//! sccache", "sccache" and "Linux dependencies for Tauri" of `.gitea/actions/setup-rust/action.yml` (docs/plans/ci-in-rust.md,
//! step 1). The action keeps two steps in YAML, on purpose: CARGO_BUILD_JOBS (the build-jobs script) and the Rust toolchain.
//! `./dev` needs `rustc` to compute its input hash and to build itself, and every later `./dev` of the job must see the same
//! compiler, so the toolchain is installed and made the default before the first `./dev` of the action runs.
//!
//! The behaviour is the shell's, case for case (tests/it/setup_rust.rs runs both on the same inputs and compares what they
//! write to `$GITHUB_ENV` and how they call sccache):
//!
//! 1. **Install** the pinned sccache ([`SCCACHE_VERSION`], one per OS and architecture, checksum-verified) into
//!    `${RUNNER_TOOL_CACHE:-$HOME/.cache}/sccache/<version>` and add that folder to `$GITHUB_PATH`, unless the `sccache` on
//!    `PATH` is exactly that version. A failed download is a warning (the job builds without the cache); a checksum mismatch
//!    fails the step.
//! 2. **Configure** it: the backend (`local`, `auto` or `redis`), the server port (4227 in a Linux container, else the job
//!    slot's own port), the local folder and size (the slot's kept folder on a host; in a container the job's lane folder
//!    `<mount>/sccache-<lane>`, never the mount itself: ADR 0111; the host's budget divided between the slots or lanes), a fresh server for this job, and `RUSTC_WRAPPER=sccache` with the settings in
//!    `$GITHUB_ENV`. A server that does not start leaves the wrapper unset: the cache may slow a build, never fail it.
//! 3. **Tauri libraries** (`--tauri-linux-deps true`, Linux only): `apt-get update` and `apt-get install` of the libraries.
//!
//! Messages and `::warning::`/`::notice::` lines go to standard output, as the shell's `echo` did; a failure is one
//! `::error::` line with the next step, and exit 1. The Redis endpoint is read from `PADDOCK_SCCACHE_REDIS_ENDPOINT` (never
//! the command line: it may carry a password, which every printed line masks).
//!
//! Test hooks: `PADDOCK_SCCACHE_MOUNT` (the container mount, as in the shell) and `PADDOCK_SCCACHE_RELEASES` (the download
//! base, so a test never reaches the network; the pinned checksum still decides).

use crate::ci_isolation::{self, Ctx, Sinks, home};
use crate::ci_tool::{NETWORK_ENV, Output, TAIL_LINES, ToolCall, error_line, tail};
use crate::local_sccache::server_env;
use clap::Args;
use paddock_ci_config::settings::sccache::{S3Ci, S3Off};
use paddock_env::Vars;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The pinned sccache release, the same on every OS (one version, one cache format). The CI image pins the same
/// (`ci-image/Dockerfile`, checked by scripts/ci-image.test.mjs).
pub const SCCACHE_VERSION: &str = "0.20.1";
const RELEASES: &str = "https://github.com/mozilla/sccache/releases/download";
/// The compile-cache budget of a Linux host in GB, divided between its lanes like the other classes' (`PADDOCK_CI_SCCACHE_GB`
/// overrides it): 6 lanes get 10 GB each, the share Mac and Windows slots have. Before ADR 0111 the whole `/sccache` mount was
/// one 20 GB cache. Not measured against d-gtr9's disk: that needs a run there.
pub const LINUX_SCCACHE_GB: u64 = 60;
/// The folder of lane `lane` on the kept `mount`: the one place that names it (`<mount>/sccache-<lane>`). Never the mount itself.
/// Another tier of the compile cache that needs a folder of its own beside it (a local tier of an S3 cache) derives its folder from
/// this one (`<mount>/sccache-<lane>-s3`), so no two writers ever share a folder.
pub fn lane_sccache_dir(mount: &str, lane: &str) -> String {
    format!("{mount}/sccache-{lane}")
}

/// The children of the kept mount that older jobs used and this design does not: `paddock-dev/` (the tool), `npm/`, `angular/` and
/// sccache's own hex folders `0`..`f` (its cache used to be the mount's root). Never `lanes/` (the slot locks) or `sccache-*`.
pub fn legacy_mount_entries() -> Vec<String> {
    let mut v: Vec<String> = ["paddock-dev", "npm", "angular"].iter().map(|s| (*s).to_string()).collect();
    v.extend("0123456789abcdef".chars().map(|c| c.to_string()));
    v
}

/// How long a legacy folder must have been untouched (all folders below it, to a bounded depth) before it is removed: an older-style
/// job still running on this host keeps using them and touches them.
const LEGACY_IDLE: Duration = Duration::from_secs(24 * 3600);

/// The newest modification time of `dir` and of the folders below it, to `depth` levels and at most `budget` folders.
fn newest_folder_mtime(dir: &Path, depth: u32, budget: &mut u32) -> std::time::SystemTime {
    let mut newest = std::fs::symlink_metadata(dir)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    if depth == 0 || *budget == 0 {
        return newest;
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                *budget = budget.saturating_sub(1);
                newest = newest.max(newest_folder_mtime(&e.path(), depth - 1, budget));
            }
        }
    }
    newest
}

/// How long the sccache client waits for a server to start, in milliseconds (`server_startup_timeout_ms` of the config file named by
/// `SCCACHE_CONF`; sccache has no environment variable for it, its default is 10 s). Cause, measured: at its start the server runs a
/// storage check (a GET and a PUT of `.sccache_check`, sccache 0.20.1 src/server.rs `start_server`, src/cache/storage.rs `check`),
/// and a hung S3 store is cut by the stall guard only after its own 10 s (`paddock_net::stall_guard::Limits`), so the client gave up
/// at the same moment and the tolerant l0 policy never ran: in run 9274 (RustFS hung, 14:41 to 14:44Z) four starts took exactly
/// 10.0 s and those jobs built uncached. 30 s covers the check's GET and PUT each cut by the guard (ER-12: a limit raised with its
/// measured cause beside it). Twin: the shell step of `.gitea/actions/setup-rust/action.yml`; a test keeps both above the guard.
pub const SERVER_STARTUP_TIMEOUT_MS: u64 = 30_000;

/// The text of the job's sccache config file: only the client's start wait; every other setting stays in the environment, which
/// outranks the file.
pub fn server_conf_text() -> String {
    format!("server_startup_timeout_ms = {SERVER_STARTUP_TIMEOUT_MS}\n")
}

/// The `$GITHUB_ENV` variable that says the job's server did not start (`off:<cause>`); `dev ci sccache-stats` reads it.
pub const STATE_VAR: &str = "PADDOCK_SCCACHE_STATE";

/// The server port in a Linux container (one job per host there); a host job takes its slot's from ports.txt.
const CONTAINER_PORT: &str = "4227";
/// What an sccache child gets from the job's environment besides the base list (paddock_proc::KEPT_ENV): its own settings,
/// cargo's and rustup's. Never a token.
const SCCACHE_PASS: [&str; 3] = ["SCCACHE_*", "CARGO_*", "RUSTUP_*"];
/// The packages Tauri needs on Linux.
const TAURI_LINUX_PACKAGES: [&str; 6] = [
    "libwebkit2gtk-4.1-dev",
    "libgtk-3-dev",
    "librsvg2-dev",
    "libayatana-appindicator3-dev",
    "patchelf",
    "cmake",
];

/// `dev ci setup-rust`.
#[derive(Args, Debug)]
pub struct SetupRustArgs {
    /// s3 (the default: the local disk first and the shared S3 store behind it, falling back to the local disk alone, with a notice
    /// naming the cause, when S3 cannot be used or the run is a release or tag run), local (sccache's disk cache), auto (Redis when
    /// PADDOCK_SCCACHE_REDIS_ENDPOINT is set, else local) or redis.
    #[arg(long, default_value = "s3")]
    pub sccache_backend: String,
    /// The Redis database number for sccache (empty: 5, the shared server's database for sccache).
    #[arg(long, default_value = "")]
    pub sccache_redis_db: String,
    /// "true": install the Linux libraries Tauri needs (ignored on other systems).
    #[arg(long, default_value = "false")]
    pub tauri_linux_deps: String,
}

/// A release asset of the pinned version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pin {
    /// The release's target triple.
    pub target: &'static str,
    /// `tar.gz` or `zip`.
    pub ext: &'static str,
    /// The SHA-256 of the archive.
    pub sha256: &'static str,
}

/// The pinned asset for a runner: `os` is `RUNNER_OS`, `arch` is what `uname -m` or Rust's `ARCH` says.
pub fn pin_for(os: &str, arch: &str) -> Option<Pin> {
    let pin = |target, ext, sha256| Some(Pin { target, ext, sha256 });
    match (os, arch) {
        ("Linux", "x86_64") => pin(
            "x86_64-unknown-linux-musl",
            "tar.gz",
            "c9f6c5a3ae84d9da0956b28c357a365827bda33f3383647fc0b67484c74a5ea9",
        ),
        ("Linux", "aarch64" | "arm64") => pin(
            "aarch64-unknown-linux-musl",
            "tar.gz",
            "505324daad5fcbb1a3cab5d57d26ef42baf4d10b236df7de93a008196f1f2230",
        ),
        ("macOS", "arm64" | "aarch64") => pin(
            "aarch64-apple-darwin",
            "tar.gz",
            "d2658c9a24bab25a24fdc1917cb5b614b7c20a4ce8f363e5303d37b7e699d5ec",
        ),
        ("Windows", "x86_64") => pin(
            "x86_64-pc-windows-msvc",
            "zip",
            "ca500a01139e9af8d5f5a5e69b6a0dd941ed79ed3399a91cc35bba484fe732fa",
        ),
        _ => None,
    }
}

/// Where the compile cache lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// The shared Redis server.
    Redis {
        /// `redis://host:port`.
        endpoint: String,
        /// The database number.
        db: String,
    },
    /// sccache's disk cache.
    Local,
    /// The job's local disk cache first, the shared S3 store behind it ([`s3_server_settings`]); resolved when the step runs, and
    /// the local disk alone when S3 cannot be used ([`S3Ci::from_vars`]).
    S3,
}

/// This host's `RUNNER_OS` spelling (`macOS`, `Linux`, `Windows`), for the prefix of a local server.
pub(crate) fn host_runner_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    }
}

/// The suffix of the S3 level's local disk: a SIBLING of the job's plain sccache folder (`<folder>-s3`), never a folder inside it
/// (sccache's LRU evicts any file below its folder) and never one a second lane can reach (ADR 0111). Jobs without the S3 level and
/// release builds use the plain folder and never see what S3 backfilled.
const S3_TIER_SUFFIX: &str = "-s3";

/// The S3 level's local disk of lane `lane` on the kept `mount` (`<mount>/sccache-<lane>-s3`), derived from [`lane_sccache_dir`], the
/// one function that names lane folders.
pub fn lane_s3_dir(mount: &str, lane: &str) -> String {
    s3_tier_of(&lane_sccache_dir(mount, lane))
}

/// The S3 tier beside any job-private sccache folder.
pub(crate) fn s3_tier_of(plain: &str) -> String {
    format!("{plain}{S3_TIER_SUFFIX}")
}

/// The longest the timeout guard may take to report its port.
const GUARD_START: Duration = Duration::from_secs(5);

/// The timeout guard that was started.
struct GuardStarted {
    port: u16,
    log: String,
}

/// The health check of the S3 store before a job uses it: an HTTP GET of [`S3_HEALTH_PATH`] that must answer 2xx. A TCP connect proved
/// only that something listens (a store that accepts connections and then hangs passed it). curl's own limits end a dead or slow
/// store at [`S3_HEALTH_CONNECT`] / [`S3_HEALTH_MAX`] seconds, the DNS lookup included. On the LAN the health check took 1 to 119 ms
/// (measured 2026-11-19 from the Mac, Linux and Windows runners, docs/benchmarks.md).
pub const S3_HEALTH_PATH: &str = "/minio/health/live";
/// curl `--connect-timeout`, in seconds.
pub const S3_HEALTH_CONNECT: u64 = 3;
/// curl `--max-time`, in seconds: the whole check.
pub const S3_HEALTH_MAX: u64 = 5;

/// The one health check of the S3 store, CI's step and the developer's `./dev sccache env` alike (ER-22): curl with a connect limit of
/// [`S3_HEALTH_CONNECT`] seconds (the DNS lookup included) and a total limit of [`S3_HEALTH_MAX`], proxies off, a non-2xx answer a failure.
pub fn s3_health_call(url: &str) -> ToolCall {
    ToolCall::new("curl")
        .args(["-fsS", "--noproxy", "*", "--connect-timeout"])
        .args([
            S3_HEALTH_CONNECT.to_string(),
            "--max-time".to_string(),
            S3_HEALTH_MAX.to_string(),
            url.to_string(),
        ])
        .timeout(Duration::from_secs(S3_HEALTH_MAX + 5))
}

/// What [`s3_health_call`] says: `Ok` for a 2xx answer, else one sentence with the cause (never a key).
pub fn s3_health_verdict(url: &str, ran: Result<crate::ci_tool::Ran, String>) -> Result<(), String> {
    match ran {
        Ok(r) if r.success => Ok(()),
        Ok(r) => Err(format!(
            "the S3 health check {url} failed ({}): {}",
            r.how,
            r.stderr.trim().lines().last().unwrap_or("no message")
        )),
        Err(e) => Err(format!("the S3 health check could not run curl: {e}")),
    }
}

/// The size of the object the startup write probe puts: sccache's own startup check puts 13 bytes, which a store over its quota still takes
/// (runs 9381 and 9388), so a refusal of real writes showed only as "writes 0" at the end of the job.
pub const S3_WRITE_PROBE_BYTES: usize = 256 * 1024;

/// The curl config (read from curl's standard input, never an argument: it holds the key) of the startup write probe: a signed PUT of
/// `body` to `object_url`, or a DELETE of it when `body` is `None`. The answer's body, then its status on a last line.
pub fn s3_write_probe_config(object_url: &str, key: (&str, &str), body: Option<&Path>) -> String {
    use crate::curl_config::line;
    let mut c = line("url", object_url);
    c.push_str("aws-sigv4 = \"aws:amz:auto:s3\"\n");
    c.push_str(&line("user", &format!("{}:{}", key.0, key.1)));
    c.push_str("silent\nshow-error\nconnect-timeout = 3\nmax-time = 20\nwrite-out = \"\\n%{http_code}\"\n");
    match body {
        Some(f) => c.push_str(&line("upload-file", &f.display().to_string())),
        None => c.push_str("request = \"DELETE\"\n"),
    }
    c
}

/// What the probe's PUT answered: `Ok` for a 2xx, else one sentence with the store's own reason (its S3 `Code` and `Message`, never the key).
pub fn s3_write_probe_verdict(curl_stdout: &str) -> Result<(), String> {
    let (body, status) = curl_stdout.trim_end().rsplit_once('\n').unwrap_or(("", curl_stdout.trim()));
    match status.trim().parse::<u16>() {
        Ok(200..=299) => Ok(()),
        Ok(code) => Err(format!(
            "the store answered {code} to a write of {} KiB{}",
            S3_WRITE_PROBE_BYTES / 1024,
            crate::test_cache::s3::s3_error(body)
        )),
        Err(_) => Err("the store gave no status to a write probe".to_string()),
    }
}

/// The store's health URL for `host:port` and the scheme.
pub fn s3_health_url(ssl: bool, endpoint: &str) -> String {
    format!("{}://{endpoint}{S3_HEALTH_PATH}", if ssl { "https" } else { "http" })
}

/// The settings the sccache SERVER starts with (never a build's environment): the multi-level chain `disk,s3` behind the local disk
/// (`dir`, `size`) and the S3 store. Only the start call gets these; `$GITHUB_ENV` gets [`s3_build_settings`].
pub fn s3_server_settings(s3: &S3Ci, dir: &str, size: &str) -> Vec<(String, String)> {
    paddock_ci_config::settings::sccache::multilevel_settings(dir, size, &s3.bucket, &s3.endpoint, s3.ssl, &s3.prefix)
}

/// The settings every later step of the job gets in `$GITHUB_ENV` when the S3 level is on: the local disk only, the two that keep
/// a server a build starts itself alive and off the metadata lookup, and where the server writes its errors. No chain, no bucket, no
/// key: a server started by a later rustc (ours ended) then serves from the local disk instead of failing 10 s into the build for
/// lack of keys (measured in review of 534918839: exit 2 in 2 of 2 runs).
pub fn s3_build_settings(dir: &str, size: &str, error_log: &str) -> Vec<(String, String)> {
    let mut v = vec![
        ("SCCACHE_DIR".to_string(), dir.to_string()),
        ("SCCACHE_CACHE_SIZE".to_string(), size.to_string()),
        ("SCCACHE_ERROR_LOG".to_string(), error_log.to_string()),
    ];
    v.extend(paddock_ci_config::settings::sccache::survival_vars(true));
    v
}

/// The backend the inputs ask for, or the `::error::` text when they are wrong.
pub fn choose_backend(backend: &str, endpoint: &str, db: &str) -> Result<Backend, String> {
    if backend == "s3" {
        return Ok(Backend::S3);
    }
    if !matches!(backend, "auto" | "redis" | "local") {
        return Err(error_line(
            &format!("sccache-backend must be auto, redis, local or s3 (got '{backend}')."),
            "fix the input of the setup-rust action.",
        ));
    }
    if backend == "redis" && endpoint.is_empty() {
        return Err(error_line(
            "sccache-backend is redis but sccache-redis-endpoint is empty (set the Gitea variable SCCACHE_REDIS_ENDPOINT).",
            "set the variable, or use sccache-backend: auto or local.",
        ));
    }
    if backend == "redis" || (backend == "auto" && !endpoint.is_empty()) {
        let db = if db.is_empty() { "5" } else { db };
        return Ok(Backend::Redis {
            endpoint: endpoint.to_string(),
            db: db.to_string(),
        });
    }
    Ok(Backend::Local)
}

/// A host slot's share of the host's sccache budget: `total_gb` (`PADDOCK_CI_SCCACHE_GB`; 40 when unset or not digits)
/// divided by the slots, at least 2 GB.
pub fn slot_cache_size(total_gb: Option<&str>, slots: u64) -> String {
    slot_cache_size_or(total_gb, slots, 40)
}

/// [`slot_cache_size`] with the class's own default budget (Linux: [`LINUX_SCCACHE_GB`]).
pub fn slot_cache_size_or(total_gb: Option<&str>, slots: u64, default_gb: u64) -> String {
    let total = total_gb
        .filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or(default_gb);
    let per = (total / slots.max(1)).max(2);
    format!("{per}G")
}

/// Cargo's switch back to its legacy build-dir layout (one `-L dependency=<deps>` instead of one per indirect dependency), read
/// in cargo 1.100's `is_new_build_dir_layout_opt_out` (src/workspace/features.rs). Windows needs it under sccache: cargo falls
/// back to an `@argfile` for a command line over CreateProcessW's 32,767 characters, and sccache 0.19 expands that file and starts
/// rustc with the whole command line again, which fails with os error 206 (run 8027, job 37099: 313 `-L` for spotter_app_lib;
/// mozilla/sccache#2787, open fixes #2789 and #3026). sccache hashes `CARGO_*` variables only, so this one leaves its keys alone.
/// Remove it once the pinned sccache passes a response file to rustc; cargo may drop the variable in a later release.
pub const LEGACY_BUILD_DIR_LAYOUT: &str = "__CARGO_TEMPORARY_BUILD_DIR_NEW_LAYOUT_OPT_OUT=1";

/// The lines for `$GITHUB_ENV`, in the shell's order: on Windows [`LEGACY_BUILD_DIR_LAYOUT`] follows the wrapper settings.
pub fn env_lines(port: &str, settings: &[(String, String)], windows: bool) -> Vec<String> {
    let mut lines = vec![
        "RUSTC_WRAPPER=sccache".to_string(),
        "SCCACHE_IGNORE_SERVER_IO_ERROR=1".to_string(),
        format!("SCCACHE_SERVER_PORT={port}"),
    ];
    // After the wrapper: the `CARGO_*` variables sccache hashes into every key, one definition with the agents' shim.
    lines.splice(
        1..1,
        paddock_ci_config::settings::sccache::CI_KEY_ENV
            .iter()
            .map(|(k, v)| format!("{k}={v}")),
    );
    if windows {
        lines.push(LEGACY_BUILD_DIR_LAYOUT.to_string());
    }
    lines.extend(settings.iter().map(|(k, v)| format!("{k}={v}")));
    lines
}

/// An endpoint for the log: a password in it (`redis://:pw@host`) is masked.
pub fn shown_endpoint(endpoint: &str) -> String {
    match (endpoint.find("://"), endpoint.rfind('@')) {
        (Some(s), Some(at)) if at > s + 3 => format!("{}***{}", &endpoint[..s + 3], &endpoint[at..]),
        _ => endpoint.to_string(),
    }
}

/// The password of a `redis://[user]:password@host` endpoint (or the user part when there is no colon), for masking.
pub fn endpoint_password(endpoint: &str) -> Option<String> {
    let rest = &endpoint[endpoint.find("://").map_or(0, |s| s + 3)..];
    let (userinfo, _) = rest.rsplit_once('@')?;
    let password = userinfo.split_once(':').map_or(userinfo, |(_, p)| p);
    (!password.is_empty()).then(|| password.to_string())
}

/// The step's environment as [`Vars`], so the settings read it like any other.
struct CtxVars<'a>(&'a Ctx);

impl Vars for CtxVars<'_> {
    fn get(&self, key: &str) -> Option<std::ffi::OsString> {
        self.0.get(key).map(std::ffi::OsString::from)
    }
}

/// The step's environment and streams.
struct Step<'a> {
    ctx: Ctx,
    io: Sinks<'a>,
}

/// A phase's result: go on, or stop the step with this exit code.
type Phase = Result<(), i32>;

impl Step<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.ctx.get(key)
    }

    fn os(&self) -> &str {
        self.get("RUNNER_OS").unwrap_or("")
    }

    fn say(&mut self, text: impl AsRef<str>) {
        let _ = writeln!(self.io.out, "{}", text.as_ref());
    }

    fn fail(&mut self, line: String) -> i32 {
        self.say(line);
        1
    }

    /// A path from the environment as this process takes it (Git bash hands `/c/x` on Windows).
    fn path_var(&self, key: &str) -> Option<String> {
        self.get(key)
            .map(|v| paddock_env::winpath::from_git_bash(v, home::os_of(self.ctx.windows)))
    }

    /// `${RUNNER_TOOL_CACHE:-$HOME/.cache}/sccache/<version>`.
    fn pinned_dir(&self) -> PathBuf {
        let base = self
            .path_var("RUNNER_TOOL_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(self.path_var("HOME").unwrap_or_default()).join(".cache"));
        base.join("sccache").join(SCCACHE_VERSION)
    }

    fn exe_name(&self, name: &str) -> String {
        if self.os() == "Windows" {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }

    /// The first `name` in `dirs`, then on `PATH` (`command -v`).
    fn find(&self, name: &str, first: &[PathBuf]) -> Option<PathBuf> {
        let file = self.exe_name(name);
        let sep = if cfg!(windows) { ';' } else { ':' };
        let path = self.get("PATH").unwrap_or("").to_string();
        first
            .iter()
            .cloned()
            .chain(path.split(sep).filter(|d| !d.is_empty()).map(PathBuf::from))
            .map(|d| d.join(&file))
            .find(|p| is_executable(p))
    }

    /// The job's temp folder for a download (mktemp's choice: TMPDIR, else the runner's, else the system's).
    fn temp_base(&self) -> PathBuf {
        ["TMPDIR", "RUNNER_TEMP", "TEMP"]
            .iter()
            .find_map(|k| self.path_var(k))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"))
    }

    /// `dev ci isolation <args>`, in this process: its value, or its message and exit code.
    fn isolation(&mut self, args: &[&str]) -> Result<String, i32> {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let argv: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let code = ci_isolation::dispatch(
            &self.ctx,
            &argv,
            &mut Sinks {
                out: &mut out,
                err: &mut err,
            },
        );
        let _ = self.io.err.write_all(&err);
        if code != 0 {
            return Err(code);
        }
        Ok(String::from_utf8_lossy(&out).trim_end().to_string())
    }

    fn append(&mut self, key: &str, lines: &[String]) -> Phase {
        let Some(file) = self.path_var(key) else {
            let code = self.fail(error_line(
                &format!("{key} is not set."),
                "run this command in a CI step (or set it for a test).",
            ));
            return Err(code);
        };
        let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .and_then(|mut f| f.write_all(body.as_bytes()));
        written.map_err(|e| {
            self.fail(error_line(
                &format!("cannot append to {key} ({file}): {e}."),
                "check the runner's job folder.",
            ))
        })
    }

    /// Runs a call, logging it to standard output.
    fn run(&mut self, call: &ToolCall) -> Result<crate::ci_tool::Ran, String> {
        let parent = self.ctx.env_snapshot();
        call.run(&parent, &mut *self.io.out)
    }

    fn phase(&mut self, name: &str, f: impl FnOnce(&mut Self) -> Phase) -> Phase {
        let started = Instant::now();
        let r = f(self);
        let took = started.elapsed().as_secs_f64();
        match r {
            Ok(()) => self.say(format!("setup-rust: {name}: done in {took:.1} s")),
            Err(_) => self.say(format!("setup-rust: {name}: failed after {took:.1} s")),
        }
        r
    }

    // ---- 1. install --------------------------------------------------------------------------------------------------

    fn install(&mut self) -> Phase {
        // A runner that already has exactly the pinned version keeps it.
        if let Some(found) = self.find("sccache", &[])
            && self.sccache_version(&found).as_deref() == Some(SCCACHE_VERSION)
        {
            return Ok(());
        }
        let arch = std::env::consts::ARCH;
        let Some(pin) = pin_for(self.os(), arch) else {
            let os = self.os().to_string();
            self.say(format!(
                "::warning::no pinned sccache release for {os} {arch}; building without the shared compile cache."
            ));
            return Ok(());
        };
        let dest = self.pinned_dir();
        let bin = dest.join(self.exe_name("sccache"));
        // A copy in the tool cache is used only when it answers with the pinned version (a half-written or foreign file is
        // replaced). A failed download ends the phase without touching PATH, as the shell's `exit 0` did.
        let cached = is_executable(&bin) && self.sccache_version(&bin).as_deref() == Some(SCCACHE_VERSION);
        if !cached && !self.download(pin, &dest, &bin)? {
            return Ok(());
        }
        let line = home::native(&dest.to_string_lossy());
        self.append("GITHUB_PATH", &[line])
    }

    fn sccache_version(&mut self, exe: &Path) -> Option<String> {
        let call = ToolCall::new(exe.to_string_lossy())
            .args(["--version"])
            .timeout(Duration::from_secs(30));
        let ran = self.run(&call.pass(SCCACHE_PASS)).ok()?;
        // `sccache 0.19.0`: the second field, as `awk '{print $2}'`.
        ran.stdout.split_whitespace().nth(1).map(str::to_string)
    }

    /// Downloads, checks and places the pinned release: `Ok(false)` when the download failed (a warning, the job goes on).
    fn download(&mut self, pin: Pin, dest: &Path, bin: &Path) -> Result<bool, i32> {
        let dl = self.temp_base().join(format!("paddock-sccache-{}-{}", std::process::id(), nanos()));
        if let Err(e) = std::fs::create_dir_all(&dl) {
            return Err(self.fail(error_line(
                &format!("cannot make the download folder {}: {e}.", dl.display()),
                "check the job's temp folder.",
            )));
        }
        let r = self.fetch_and_place(pin, &dl, dest, bin);
        let _ = std::fs::remove_dir_all(&dl);
        r
    }

    fn fetch_and_place(&mut self, pin: Pin, dl: &Path, dest: &Path, bin: &Path) -> Result<bool, i32> {
        let name = format!("sccache-v{SCCACHE_VERSION}-{}", pin.target);
        let archive = dl.join(format!("{name}.{}", pin.ext));
        let base = self.get("PADDOCK_SCCACHE_RELEASES").unwrap_or(RELEASES).to_string();
        let url = format!("{base}/v{SCCACHE_VERSION}/{name}.{}", pin.ext);
        let curl = ToolCall::new("curl")
            .args(["-fsSL", "--retry", "3", "-o"])
            .args([archive.to_string_lossy().into_owned(), url])
            .pass(NETWORK_ENV.iter().copied())
            .timeout(Duration::from_secs(300));
        match self.run(&curl) {
            Ok(r) if r.success => {}
            other => {
                // A failed download must not fail the job: the build runs without the cache.
                let why = match other {
                    Ok(r) => format!("curl {}; {}", r.how, tail(&r.stderr, 3).trim()),
                    Err(e) => e,
                };
                self.say(format!(
                    "::warning::could not download sccache {SCCACHE_VERSION}; building without the compile cache ({why})."
                ));
                return Ok(false);
            }
        }
        let got = match sha256_file(&archive) {
            Ok(h) => h,
            Err(e) => return Err(self.fail(error_line(&format!("cannot read the sccache download: {e}."), "rerun the job."))),
        };
        if got != pin.sha256 {
            let target = pin.target;
            return Err(self.fail(error_line(
                &format!("sccache {SCCACHE_VERSION} checksum mismatch for {target} (got {got})."),
                "do not use this download; check the release on GitHub and the pin in tools/paddock-dev/src/ci_setup_rust.rs.",
            )));
        }
        self.extract(pin, &archive, dl)?;
        let exe = self.exe_name("sccache");
        let from = dl.join(&name).join(&exe);
        // Renamed into place whole: a runner with a shared tool cache may run two jobs that install it at once. The part file's
        // name is this attempt's own (pid and time).
        let part = dest.join(format!("{exe}.part.{}.{}", std::process::id(), nanos()));
        let placed = std::fs::create_dir_all(dest)
            .and_then(|()| std::fs::copy(&from, &part).map(|_| ()))
            .and_then(|()| make_executable(&part))
            .and_then(|()| std::fs::rename(&part, bin));
        let Err(e) = placed else { return Ok(true) };
        let _ = std::fs::remove_file(&part);
        // Windows cannot replace a program that a concurrent job is running: that job put the same release there, which is
        // as good when it answers with the pinned version.
        if is_executable(bin) && self.sccache_version(bin).as_deref() == Some(SCCACHE_VERSION) {
            self.say(format!(
                "::notice::sccache {SCCACHE_VERSION} was put in place by another job meanwhile ({e}); using it."
            ));
            return Ok(true);
        }
        Err(self.fail(error_line(
            &format!("cannot put sccache into {}: {e}.", dest.display()),
            "check the runner's tool cache folder (RUNNER_TOOL_CACHE) is writable.",
        )))
    }

    fn extract(&mut self, pin: Pin, archive: &Path, into: &Path) -> Phase {
        let a = archive.to_string_lossy().into_owned();
        let d = into.to_string_lossy().into_owned();
        let call = if pin.ext == "zip" {
            if self.find("unzip", &[]).is_some() {
                ToolCall::new("unzip").args(["-q", a.as_str(), "-d", d.as_str()])
            } else {
                // Paths go to PowerShell as data (an environment variable), never spliced into its script.
                ToolCall::new("powershell")
                    .args([
                        "-NoProfile",
                        "-Command",
                        "Expand-Archive -Path $env:PADDOCK_ARCHIVE -DestinationPath $env:PADDOCK_ARCHIVE_TO",
                    ])
                    .env("PADDOCK_ARCHIVE", home::native(&a))
                    .env("PADDOCK_ARCHIVE_TO", home::native(&d))
            }
        } else {
            ToolCall::new("tar").args(["-xzf", a.as_str(), "-C", d.as_str()])
        };
        let call = call.timeout(Duration::from_secs(120));
        match self.run(&call) {
            Ok(r) if r.success => Ok(()),
            Ok(r) => {
                let t = tail(&r.stderr, TAIL_LINES);
                self.say(t);
                Err(self.fail(error_line(
                    &format!("cannot unpack the sccache download ({}).", r.how),
                    "rerun the job; if it repeats, check the archive tool on the runner.",
                )))
            }
            Err(e) => Err(self.fail(error_line(
                &format!("cannot unpack the sccache download: {e}"),
                "install the archive tool on the runner.",
            ))),
        }
    }

    // ---- 2. configure ------------------------------------------------------------------------------------------------

    fn configure(&mut self, args: &SetupRustArgs) -> Phase {
        // The pinned copy first (the install added it to PATH for later steps only), then PATH.
        let pinned = self.pinned_dir();
        let first: Vec<PathBuf> = if pinned.is_dir() { vec![pinned] } else { Vec::new() };
        let Some(sccache) = self.find("sccache", &first) else {
            self.say("::warning::sccache is not available on this runner; building without the compile cache.");
            return Ok(());
        };
        let endpoint = self.get("PADDOCK_SCCACHE_REDIS_ENDPOINT").unwrap_or("").to_string();
        // The runner masks it in every later line of the job too (the variable reaches $GITHUB_ENV).
        if let Some(pw) = endpoint_password(&endpoint)
            && self.get("GITHUB_ACTIONS") == Some("true")
        {
            self.say(format!("::add-mask::{pw}"));
        }
        let backend = match choose_backend(&args.sccache_backend, &endpoint, &args.sccache_redis_db) {
            Ok(b) => b,
            Err(line) => return Err(self.fail(line)),
        };
        // CI's own server: a host may run a developer's sccache on the default port 4226 with its own backend. On a host each
        // job slot has its own port (ports.txt), so a concurrent job's start or stats never takes this job's server away.
        let port = if self.os() == "Linux" {
            CONTAINER_PORT.to_string()
        } else {
            self.isolation(&["port", "sccache"])?
        };
        // `settings` go to $GITHUB_ENV (every later step); `server_settings` only to the server's own start. They differ for S3: the
        // chain, the bucket and the keys belong to the server alone.
        let mut settings: Vec<(String, String)> = Vec::new();
        let mut server_settings: Option<Vec<(String, String)>> = None;
        // The timeout guard's port and log (S3 only; `ci sccache-stats` stops it and prints its log).
        let mut guard_port: Option<u16> = None;
        // The S3 keys, for the server's start only: never in `settings`, which go to $GITHUB_ENV.
        let mut s3_keys: Vec<(String, String)> = Vec::new();
        match &backend {
            Backend::Redis { endpoint, db } => {
                settings.push(("SCCACHE_REDIS_ENDPOINT".into(), endpoint.clone()));
                settings.push(("SCCACHE_REDIS_DB".into(), db.clone()));
                self.say(format!("sccache: shared backend {} db {db}", shown_endpoint(endpoint)));
            }
            Backend::Local => {
                let (dir, size, _) = self.local_cache()?;
                settings.push(("SCCACHE_DIR".into(), dir.clone()));
                settings.push(("SCCACHE_CACHE_SIZE".into(), size));
                self.say(format!("sccache: local disk {dir}"));
            }
            Backend::S3 => {
                let (plain_dir, size, tier) = self.local_cache()?;
                // The S3 level's local disk is a sibling folder of the job's own: entries backfilled from S3 never reach the
                // plain folder that release builds and jobs without S3 read (review of 534918839, P2: cache poisoning), and no
                // other lane can reach it (ADR 0111).
                let dir = tier;
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    return Err(self.fail(error_line(
                        &format!("cannot make the sccache folder {dir}: {e}."),
                        "check the disk of the runner.",
                    )));
                }
                let level = S3Ci::from_vars(&CtxVars(&self.ctx), self.os(), std::env::consts::ARCH)
                    .and_then(|s3| self.health(&s3).map(|()| s3))
                    .and_then(|s3| self.start_guard(&s3).map(|g| (s3, g)));
                match level {
                    Ok((real, guard)) => {
                        // sccache talks to the guard on 127.0.0.1, which cuts a request the store does not answer (ci_s3_guard.rs).
                        let s3 = S3Ci {
                            endpoint: format!("127.0.0.1:{}", guard.port),
                            ssl: false,
                            ..real.clone()
                        };
                        guard_port = Some(guard.port);
                        let guard_env = [
                            ("PADDOCK_S3_GUARD_PORT".to_string(), guard.port.to_string()),
                            ("PADDOCK_S3_GUARD_LOG".to_string(), guard.log),
                        ];
                        // Gitea masks secrets it passes itself; masking them again here also covers a value reaching the step another way.
                        if self.get("GITHUB_ACTIONS") == Some("true") {
                            self.say(format!("::add-mask::{}", s3.key.0.expose()));
                            self.say(format!("::add-mask::{}", s3.key.1.expose()));
                        }
                        s3_keys.push(("AWS_ACCESS_KEY_ID".into(), s3.key.0.expose().to_string()));
                        s3_keys.push(("AWS_SECRET_ACCESS_KEY".into(), s3.key.1.expose().to_string()));
                        self.say(format!(
                            "sccache: local disk {dir}, then S3 bucket {} at {} (through the timeout guard on {}) under {} (credentials configured)",
                            s3.bucket, real.endpoint, s3.endpoint, s3.prefix
                        ));
                        let error_log = self.temp_base().join("sccache-error.log").display().to_string();
                        let mut full = s3_server_settings(&s3, &dir, &size);
                        // The server writes its errors into the job's temp folder, so `sccache-stats` can show them (ER-27).
                        full.push(("SCCACHE_ERROR_LOG".into(), error_log.clone()));
                        server_settings = Some(full);
                        settings.extend(s3_build_settings(&dir, &size, &error_log));
                        settings.extend(guard_env.iter().cloned());
                    }
                    Err(off) => {
                        self.say(format!(
                            "::{}::sccache: {}; this job uses its local disk cache only. Next: docs/research/2026-11-19-sccache.md, section 8.",
                            if off.warn { "warning" } else { "notice" },
                            off.why
                        ));
                        settings.push(("SCCACHE_DIR".into(), plain_dir.clone()));
                        settings.push(("SCCACHE_CACHE_SIZE".into(), size));
                    }
                }
            }
        }
        // The checkout and the build folder are per slot or per job (a random work folder in a container), so a workspace crate's key
        // changed with every slot flip and the hit rate was 0%: sccache 0.20 strips these bases from every path in a Rust key
        // (`SCCACHE_BASEDIRS`), and keeps the manifest paths verbatim for the crates that bake them in (local_sccache::basedir_vars).
        // The server reads them at its start and each compile reads the verbatim list, so they go to both.
        let roots: Vec<String> = ["PADDOCK_WORKSPACE", "GITHUB_WORKSPACE", "PADDOCK_BUILD_FOLDER"]
            .iter()
            .filter_map(|k| self.path_var(k))
            .collect();
        let basedirs = paddock_ci_config::settings::sccache::basedir_vars(&roots, self.ctx.windows);
        if let Some((k, v)) = basedirs.first() {
            self.say(format!(
                "sccache: base directories {v} ({k}; the same source at another slot or work folder hits the same entry)"
            ));
        }
        settings.extend(basedirs.iter().cloned());
        if let Some(full) = server_settings.as_mut() {
            full.extend(basedirs);
        }
        // The client's start wait (SERVER_STARTUP_TIMEOUT_MS): the server's own start and every later client of the job read it.
        let conf = self.temp_base().join("paddock-sccache.toml");
        match std::fs::write(&conf, server_conf_text()) {
            Ok(()) => {
                let kv = ("SCCACHE_CONF".to_string(), conf.display().to_string());
                settings.push(kv.clone());
                if let Some(full) = server_settings.as_mut() {
                    full.push(kv);
                }
            }
            Err(e) => self.say(format!(
                "::warning::sccache: cannot write its config {} ({e}); the client keeps its 10 s start wait.",
                conf.display()
            )),
        }
        let secret = endpoint
            .rsplit_once('@')
            .map(|(userinfo, _)| userinfo.to_string())
            .unwrap_or_default();
        let password = endpoint_password(&endpoint).unwrap_or_default();
        let call = |a: &str| {
            let c = ToolCall::new(sccache.to_string_lossy())
                .args([a])
                .env("SCCACHE_SERVER_PORT", port.as_str())
                .secret(secret.clone())
                .secret(password.clone())
                .pass(SCCACHE_PASS)
                .timeout(Duration::from_secs(60));
            // One shared way to start a server that may serve S3 (local_sccache.rs, also the agents' server).
            let s3 = !s3_keys.is_empty();
            server_env(c, server_settings.as_deref().unwrap_or(&settings), s3, s3, &s3_keys)
        };
        if let Ok(r) = self.run(&call("--version")) {
            let v = r.stdout.trim().to_string();
            self.say(v);
        }
        // A server left on this port by an earlier job would keep that job's backend: start it fresh with this job's settings.
        let _ = self.run(&call("--stop-server"));
        // Two tries: a slow storage check at the start (run 9274) is not a reason to build uncached. The server outlives this step and
        // would hold a pipe open, so its starter writes to a file.
        let mut cause = "start-failed-twice";
        let mut answers = false;
        for attempt in 0..2 {
            if attempt == 1 {
                self.say("sccache: the server did not start; stopping it and starting it once more.");
                let _ = self.run(&call("--stop-server"));
            }
            let log = self
                .temp_base()
                .join(format!("paddock-sccache-start-{}-{}.log", std::process::id(), nanos()));
            let started = self.run(&call("--start-server").output(Output::File(log.clone())));
            let _ = std::fs::remove_file(&log);
            let up = matches!(&started, Ok(r) if r.success);
            answers = up && matches!(self.run(&call("--show-stats")), Ok(r) if r.success);
            if answers {
                break;
            }
            cause = if up { "no-answer-after-start" } else { "start-failed-twice" };
            if let Ok(r) = &started
                && !r.success
            {
                let t = tail(&r.stderr, TAIL_LINES);
                self.say(t);
            }
        }
        if !answers {
            // The cache must slow a build down, never fail it: RUSTC_WRAPPER and the port stay unset (plainly uncached, never half
            // wired) and the state says why, so the stats step reports "not measured" instead of asking some other server.
            let _ = self.run(&call("--stop-server"));
            if let Some(port) = guard_port {
                crate::ci_s3_guard::stop(port);
            }
            self.say(format!(
                "::warning::the sccache server did not start ({cause}); the build runs without the compile cache."
            ));
            self.summary_line(&format!("- sccache: OFF ({cause}); this job built without the compile cache"));
            return self.append("GITHUB_ENV", &[format!("{STATE_VAR}=off:{cause}")]);
        }
        // sccache itself is the wrapper, never a script around it (Windows starts a .cmd through cmd.exe, whose 8191-character
        // command line rustc's arguments for a large crate exceed). Cached builds are not incremental.
        let windows = self.os() == "Windows";
        self.append("GITHUB_ENV", &env_lines(&port, &settings, windows))
    }

    /// The S3 store's health check: an HTTP GET that answers 2xx within curl's own limits ([`S3_HEALTH_CONNECT`], [`S3_HEALTH_MAX`]).
    /// A store that accepts a connection and then hangs (or answers an error) is not used: this job runs on its local disk alone.
    fn health(&mut self, s3: &S3Ci) -> Result<(), S3Off> {
        let url = s3_health_url(s3.ssl, &s3.endpoint);
        let ran = self.run(&s3_health_call(&url));
        s3_health_verdict(&url, ran).map_err(|why| S3Off { warn: true, why })?;
        self.write_probe(s3);
        Ok(())
    }

    /// A signed PUT of [`S3_WRITE_PROBE_BYTES`] under the job's prefix, then its DELETE: sccache's own startup check puts 13 bytes, which a
    /// store over its quota still accepts, so this is where a refusal of real writes shows at the start (ER-27). A refusal is a warning,
    /// never a stop: reads still help, and `sccache-stats` says the same at the end. The key goes to curl's standard input only.
    fn write_probe(&mut self, s3: &S3Ci) {
        use paddock_proc::{Limits, run_bounded_with_input};
        let scheme = if s3.ssl { "https" } else { "http" };
        let url = format!(
            "{scheme}://{}/{}/{}/.paddock-write-probe-{}",
            s3.endpoint,
            s3.bucket,
            s3.prefix,
            std::process::id()
        );
        let body = self.temp_base().join(format!("s3-write-probe-{}", std::process::id()));
        if std::fs::write(&body, vec![0u8; S3_WRITE_PROBE_BYTES]).is_err() {
            return;
        }
        let key = (s3.key.0.expose(), s3.key.1.expose());
        let curl = |config: String| {
            let mut cmd = paddock_proc::command("curl");
            cmd.args(["-q", "-K", "-"]);
            run_bounded_with_input(cmd, config.into_bytes(), Limits::new(Duration::from_secs(30)))
        };
        let verdict = match curl(s3_write_probe_config(&url, key, Some(&body))) {
            Ok(out) => s3_write_probe_verdict(&out.stdout_text()),
            Err(e) => Err(format!("the write probe could not run curl: {e}")),
        };
        let _ = std::fs::remove_file(&body);
        match verdict {
            Ok(()) => {
                let _ = curl(s3_write_probe_config(&url, key, None));
                self.say(format!("sccache: S3 write probe ok ({} KiB put and removed)", S3_WRITE_PROBE_BYTES / 1024));
            }
            Err(why) => self.say(format!(
                "::warning::sccache: S3 writes refused: {why}; this job reads from S3 but its writes will fail. Free space or raise the quota on the bucket {}.",
                s3.bucket
            )),
        }
    }

    /// Starts the timeout guard in front of the store ([`crate::ci_s3_guard`]): detached, so it outlives this step; its port comes on
    /// its standard output. No guard, no S3: the job runs on its local disk alone (a store that hangs would hold its compiles).
    fn start_guard(&mut self, s3: &S3Ci) -> Result<GuardStarted, S3Off> {
        let off = |why: String| S3Off { warn: true, why };
        if s3.ssl {
            return Err(off(
                "the timeout guard in front of the S3 store speaks http only, and the endpoint is https".to_string(),
            ));
        }
        let exe = crate::tool_cache::self_exe().map_err(|e| off(format!("the timeout guard cannot find this program to start it: {e}")))?;
        let log = self.temp_base().join("s3-guard.log").display().to_string();
        let mut cmd = paddock_proc::command(&exe);
        cmd.args(["ci", "s3-guard", "serve", "--upstream", &s3.endpoint, "--log", &log])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        // Detached and without this step's handles: the runner ends every process that keeps a pipe of the step (run 8019).
        let mut child = paddock_proc::spawn_detached_clearing_handles(&mut cmd)
            .map_err(|e| off(format!("the timeout guard could not be started: {e}")))?;
        let line = crate::slots::first_line(&mut child, GUARD_START);
        let port = line
            .as_deref()
            .and_then(|l| l.trim().strip_prefix("PORT "))
            .and_then(|p| p.trim().parse::<u16>().ok());
        match port {
            Some(port) => {
                // The guard lives on by itself; reap nothing, wait for nothing.
                drop(child);
                Ok(GuardStarted { port, log })
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                Err(off(format!(
                    "the timeout guard did not report its port within {} s",
                    GUARD_START.as_secs()
                )))
            }
        }
    }

    /// The local disk cache: a job slot's own folder with its share of the budget, on every OS (ADR 0111). On Linux the slot is
    /// a lane of the host's kept mount: the folder is `<mount>/sccache-<lane>`, never the mount itself, because sccache's LRU
    /// evicts any file below its folder, whoever put it there, and two sccache servers on one folder race ("The local storage only
    /// supports a single sccache server at a time", sccache docs/Local.md). A container without the mount keeps the cache in the
    /// job (with a notice), never in a folder another job can see.
    fn local_cache(&mut self) -> Result<(String, String, String), i32> {
        let (dir, size, tier) = if self.os() == "Linux" {
            let mount = self.path_var("PADDOCK_SCCACHE_MOUNT").unwrap_or_else(|| "/sccache".to_string());
            let own = |s: &mut Self, why: &str| {
                let temp = s.path_var("RUNNER_TEMP").unwrap_or_else(|| "/tmp".to_string());
                s.say(format!("::notice::{why}; the compile cache lives and dies with the job."));
                let plain = format!("{temp}/sccache");
                let tier = s3_tier_of(&plain);
                (plain, "20G".to_string(), tier)
            };
            if Path::new(&mount).is_dir() && dir_writable(Path::new(&mount)) {
                let slots = self.lane_count();
                match self.lane(&mount, slots) {
                    Ok(lane) => (
                        lane_sccache_dir(&mount, &lane),
                        slot_cache_size_or(self.get("PADDOCK_CI_SCCACHE_GB"), slots, LINUX_SCCACHE_GB),
                        lane_s3_dir(&mount, &lane),
                    ),
                    Err(why) => {
                        self.summary_line(&format!("- sccache lane: NOT taken ({why}); job-private folder"));
                        own(self, &format!("no lane of {mount} could be taken ({why})"))
                    }
                }
            } else {
                own(self, "no /sccache mount in this container")
            }
        } else {
            // sccache's local storage "only supports a single sccache server at a time" (docs/Local.md): one folder per slot.
            let dir = self.isolation(&["path", "sccache"])?;
            let slots = self.isolation(&["slots"])?;
            let slots: u64 = slots.parse().unwrap_or(1);
            let size = slot_cache_size(self.get("PADDOCK_CI_SCCACHE_GB"), slots);
            let dir = paddock_env::winpath::from_git_bash(&dir, home::os_of(self.ctx.windows));
            let tier = s3_tier_of(&dir);
            (dir, size, tier)
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return Err(self.fail(error_line(
                &format!("cannot make the sccache folder {dir}: {e}."),
                "check the disk of the runner.",
            )));
        }
        let (dir, tier) = if self.os() == "Windows" {
            (home::native(&dir), home::native(&tier))
        } else {
            (dir, tier)
        };
        Ok((dir, size, tier))
    }

    /// One line for the job summary (`$GITHUB_STEP_SUMMARY`), where a notice alone would scroll away; a missing file is not an error.
    fn summary_line(&mut self, text: &str) {
        if self.get("GITHUB_STEP_SUMMARY").is_some() {
            let _ = self.append("GITHUB_STEP_SUMMARY", &[text.to_string()]);
        }
    }

    /// Removes what older-style jobs left at the mount's root (see [`legacy_mount_entries`]), once a lane is held, and says what it
    /// removed. Only a folder untouched for [`LEGACY_IDLE`] goes (a job of the old design still running keeps its folders young), and
    /// never `lanes/` or `sccache-*`, which are not in the list. A refusal or failure is a warning: the cache must slow a build
    /// down, never fail it.
    fn clean_legacy_mount(&mut self, mount: &str) {
        let root = Path::new(mount);
        let home_dir = self.path_var("HOME").map(PathBuf::from);
        let mut removed = Vec::new();
        for name in legacy_mount_entries() {
            let path = root.join(&name);
            if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
                continue;
            }
            let mut budget = 20_000;
            let newest = newest_folder_mtime(&path, 4, &mut budget);
            if newest.elapsed().is_ok_and(|age| age < LEGACY_IDLE) {
                continue;
            }
            match crate::safe_remove::remove_in_mount(root, &path, home_dir.as_deref()) {
                Ok(true) => removed.push(name),
                Ok(false) => {}
                Err(e) => self.say(format!(
                    "::warning::sccache: could not remove the old {} ({e}); it stays.",
                    path.display()
                )),
            }
        }
        if !removed.is_empty() {
            self.say(format!(
                "sccache: removed what older jobs left at the root of {mount} (idle for a day or more): {}",
                removed.join(", ")
            ));
        }
    }

    /// How many lanes a Linux host has: `PADDOCK_CI_SLOTS`, else the runner's jobs per host, else the class's layout (6 on d-gtr9).
    fn lane_count(&self) -> u64 {
        let n = |k: &str| {
            self.get(k)
                .filter(|v| crate::ci_build_jobs::is_count(v))
                .and_then(|v| v.parse::<u64>().ok())
        };
        n("PADDOCK_CI_SLOTS").or_else(|| n("PADDOCK_CI_JOBS_PER_HOST")).unwrap_or_else(|| {
            crate::ci_build_jobs::class_default("Linux", crate::ci_build_jobs::Setting::Share)
                .parse()
                .unwrap_or(1)
        })
    }

    /// The job's lane on a Linux host: the slot an earlier step took (`PADDOCK_CI_SLOT`), else one taken now from the mount's
    /// `lanes/` folder with the same slot holder the host runners use (`dev ci slot`), and recorded in `$GITHUB_ENV` so every later
    /// step of the job keeps it. The holder lives until the job's folder (`RUNNER_TEMP`) is gone.
    fn lane(&mut self, mount: &str, slots: u64) -> Result<String, String> {
        if let Some(s) = self.get("PADDOCK_CI_SLOT").filter(|s| home::valid_slot(s)) {
            return Ok(s.to_string());
        }
        let owner = self.path_var("RUNNER_TEMP").ok_or("RUNNER_TEMP is not set")?;
        let args = crate::slots::AcquireArgs::new(
            u32::try_from(slots).unwrap_or(1),
            PathBuf::from(&owner),
            Path::new(mount).join("lanes"),
        );
        let report = crate::slots::acquire(&args).map_err(|e| format!("{e}"))?;
        let slot = report.json()["slot"]
            .as_str()
            .map(str::to_string)
            .ok_or("the slot tool named no slot")?;
        // A failed write is not dropped: the later steps would not know the lane, so this job does not use it.
        let mut env_lines = vec![format!("PADDOCK_CI_SLOT={slot}"), format!("PADDOCK_CI_LANE_ROOT={mount}/lanes")];
        if let Some(lock) = report.json()["lock"].as_str() {
            env_lines.push(format!("PADDOCK_CI_LANE_LOCK={lock}"));
        }
        self.append("GITHUB_ENV", &env_lines)
            .map_err(|_| format!("the lane {slot} could not be recorded in GITHUB_ENV (the error is above)"))?;
        self.say(format!("sccache: this job holds lane {slot} of {slots} on {mount}"));
        self.clean_legacy_mount(mount);
        Ok(slot)
    }

    // ---- 3. Tauri libraries ------------------------------------------------------------------------------------------

    fn tauri_linux_deps(&mut self) -> Phase {
        let sudo = self.find("sudo", &[]);
        let apt = |args: &[&str]| {
            let c = match &sudo {
                Some(s) => ToolCall::new(s.to_string_lossy()).args(["apt-get"]),
                None => ToolCall::new("apt-get"),
            };
            c.args(args.iter().copied())
                .pass(NETWORK_ENV.iter().copied())
                .pass(["DEBIAN_FRONTEND"])
                .output(Output::Stream)
                .timeout(Duration::from_secs(900))
        };
        let mut install = vec!["install", "-y"];
        install.extend(TAURI_LINUX_PACKAGES);
        for call in [apt(&["update"]), apt(&install)] {
            match self.run(&call) {
                Ok(r) if r.success => {}
                Ok(r) => {
                    return Err(self.fail(error_line(
                        &format!("`{}` failed ({}); its output is above.", call.display(), r.how),
                        "rerun the job; if it repeats, check the container image's package sources.",
                    )));
                }
                Err(e) => return Err(self.fail(error_line(&e, "use the Paddock CI image, which has apt-get."))),
            }
        }
        Ok(())
    }
}

/// `dev ci setup-rust` for the real process. Returns the exit code.
pub fn run(args: &SetupRustArgs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let env: HashMap<String, String> = crate::process_env::vars().collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let tables = ci_isolation::find_tables(None, &env, &cwd);
    let exe = crate::tool_cache::self_exe().unwrap_or_default();
    let mut ctx = Ctx::new(env, tables, exe);
    ctx.windows |= ctx.get("PADDOCK_CI_ASSUME_WINDOWS") == Some("1");
    run_with(ctx, args, out, err)
}

/// The command over an explicit context (tests).
pub fn run_with(ctx: Ctx, args: &SetupRustArgs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut step = Step {
        ctx,
        io: Sinks { out, err },
    };
    let result = step
        .phase("install sccache", Step::install)
        .and_then(|()| step.phase("configure sccache", |s| s.configure(args)))
        .and_then(|()| {
            if step.os() == "Linux" && args.tauri_linux_deps == "true" {
                step.phase("Linux libraries for Tauri", Step::tauri_linux_deps)
            } else {
                Ok(())
            }
        });
    match result {
        Ok(()) => 0,
        Err(code) => code,
    }
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

pub(crate) fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// `[ -x path ]` for a file: it exists and (Unix) has an execute bit.
fn is_executable(p: &Path) -> bool {
    let Ok(m) = std::fs::metadata(p) else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        m.is_file() && m.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        m.is_file()
    }
}

fn make_executable(p: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        Ok(())
    }
}

/// `[ -w dir ]`: this process can create a file in it (also false on a read-only mount). The probe file is removed at once.
fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".paddock-write-probe-{}-{}", std::process::id(), nanos()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_runner_class_has_its_pin_and_others_have_none() {
        assert_eq!(pin_for("Linux", "x86_64").unwrap().target, "x86_64-unknown-linux-musl");
        assert_eq!(pin_for("Linux", "arm64"), pin_for("Linux", "aarch64"));
        assert_eq!(pin_for("macOS", "aarch64").unwrap().target, "aarch64-apple-darwin");
        assert_eq!(pin_for("macOS", "arm64"), pin_for("macOS", "aarch64"));
        assert_eq!(pin_for("Windows", "x86_64").unwrap().ext, "zip");
        for (os, arch) in [("macOS", "x86_64"), ("Windows", "aarch64"), ("FreeBSD", "x86_64"), ("", "")] {
            assert_eq!(pin_for(os, arch), None, "{os} {arch}");
        }
        for (os, arch) in [("Linux", "x86_64"), ("Linux", "aarch64"), ("macOS", "arm64"), ("Windows", "x86_64")] {
            let p = pin_for(os, arch).unwrap();
            assert!(p.sha256.len() == 64 && p.sha256.bytes().all(|b| b.is_ascii_hexdigit()), "{os}");
        }
    }

    #[test]
    fn the_backend_is_local_unless_redis_is_asked_for_or_auto_has_an_endpoint() {
        assert_eq!(choose_backend("local", "redis://h:6379", ""), Ok(Backend::Local));
        assert_eq!(choose_backend("auto", "", ""), Ok(Backend::Local));
        let redis = |db: &str| Backend::Redis {
            endpoint: "redis://h:6379".into(),
            db: db.into(),
        };
        assert_eq!(choose_backend("auto", "redis://h:6379", ""), Ok(redis("5")));
        assert_eq!(choose_backend("redis", "redis://h:6379", "7"), Ok(redis("7")));
        let bad = choose_backend("disk", "", "").unwrap_err();
        assert!(
            bad.starts_with("::error::sccache-backend must be auto, redis, local or s3 (got 'disk')"),
            "{bad}"
        );
        assert!(
            choose_backend("redis", "", "")
                .unwrap_err()
                .contains("sccache-backend is redis but sccache-redis-endpoint is empty")
        );
        assert!(choose_backend("", "", "").is_err(), "an empty input is refused, as in the shell");
    }

    fn vars(pairs: &[(&str, &str)]) -> paddock_env::MapVars {
        pairs.iter().fold(paddock_env::MapVars::new(), |m, (k, v)| m.with(k, *v))
    }

    #[test]
    fn the_write_probe_names_the_stores_reason_and_never_the_key() {
        let quota = "<?xml version=\"1.0\"?><Error><Code>InvalidRequest</Code><Message>Bucket quota exceeded. Current usage 5, limit 5</Message></Error>\n400";
        let why = s3_write_probe_verdict(quota).unwrap_err();
        assert!(
            why.contains("answered 400") && why.contains("256 KiB") && why.contains("Bucket quota exceeded. Current usage 5, limit 5"),
            "{why}"
        );
        assert_eq!(s3_write_probe_verdict("\n200"), Ok(()));
        assert!(s3_write_probe_verdict("").unwrap_err().contains("no status"));
        let put = s3_write_probe_config("http://192.0.2.10:15000/b/p/o", ("kid", "ksecret"), Some(Path::new("probe-body")));
        assert!(
            put.contains("upload-file = \"probe-body\"") && put.contains("user = \"kid:ksecret\"") && !put.contains("DELETE"),
            "{put}"
        );
        assert!(s3_write_probe_config("http://h/b/o", ("k", "s"), None).contains("request = \"DELETE\""));
    }

    fn s3_health_url_of(s3: &S3Ci) -> String {
        s3_health_url(s3.ssl, &s3.endpoint)
    }

    const S3_VARS: [(&str, &str); 4] = [
        ("PADDOCK_SCCACHE_S3_BUCKET", "sccache-paddock"),
        ("PADDOCK_SCCACHE_S3_ENDPOINT", "http://192.0.2.10:15000"),
        ("PADDOCK_SCCACHE_S3_ACCESS_KEY_ID", "test-key-id"),
        ("PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY", "test-secret"),
    ];

    /// User decision 2026-11-21: s3 is the default of the command and of the action (no Gitea variable switches it).
    #[test]
    fn the_command_defaults_to_the_s3_backend() {
        #[derive(clap::Parser)]
        struct Cli {
            #[command(flatten)]
            args: SetupRustArgs,
        }
        use clap::Parser;
        assert_eq!(Cli::parse_from(["x"]).args.sccache_backend, "s3");
        assert_eq!(Cli::parse_from(["x", "--sccache-backend", "local"]).args.sccache_backend, "local");
    }

    #[test]
    fn s3_is_its_own_backend_and_has_one_key_prefix_per_runner_class() {
        assert_eq!(choose_backend("s3", "", ""), Ok(Backend::S3));
        let s3 = S3Ci::from_vars(&vars(&S3_VARS), "Windows", "x86_64").unwrap();
        assert_eq!(s3.prefix, "paddock-ci/Windows-x86_64");
        let linux = S3Ci::from_vars(&vars(&S3_VARS), "Linux", "aarch64").unwrap();
        assert_ne!(s3.prefix, linux.prefix);
        assert_eq!((s3.endpoint.as_str(), s3.ssl), ("192.0.2.10:15000", false));
    }

    #[test]
    fn the_endpoint_is_a_url_or_host_and_port_with_no_path() {
        let with = |e: &'static str| {
            let mut v = S3_VARS.to_vec();
            v[1] = ("PADDOCK_SCCACHE_S3_ENDPOINT", e);
            S3Ci::from_vars(&vars(&v), "Linux", "x86_64")
        };
        let tls = with("https://s3.example.test:443/").unwrap();
        assert_eq!((tls.endpoint.as_str(), tls.ssl), ("s3.example.test:443", true));
        assert_eq!(with("192.0.2.10:15000").unwrap().endpoint, "192.0.2.10:15000");
        for bad in ["http://192.0.2.10:15000/bucket", "192.0.2.10", "ftp://192.0.2.10:21"] {
            let off = with(bad).unwrap_err();
            assert!(off.warn && off.why.contains("http://host:port"), "{bad}: {off:?}");
        }
    }

    #[test]
    fn without_its_key_or_variables_s3_is_not_used_and_the_reason_names_no_value() {
        let without =
            |ks: &[&str]| -> Vec<(&'static str, &'static str)> { S3_VARS.iter().copied().filter(|(n, _)| !ks.contains(n)).collect() };
        // No key passed at all: a workflow not wired yet, a notice only.
        let off = S3Ci::from_vars(
            &vars(&without(&[
                "PADDOCK_SCCACHE_S3_ACCESS_KEY_ID",
                "PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY",
            ])),
            "macOS",
            "aarch64",
        )
        .unwrap_err();
        assert!(!off.warn && off.why.contains("passes no S3 key"), "{off:?}");
        for missing in [
            "PADDOCK_SCCACHE_S3_ACCESS_KEY_ID",
            "PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY",
            "PADDOCK_SCCACHE_S3_BUCKET",
            "PADDOCK_SCCACHE_S3_ENDPOINT",
        ] {
            let off = S3Ci::from_vars(&vars(&without(&[missing])), "macOS", "aarch64").unwrap_err();
            assert!(off.warn && off.why.contains(missing), "{off:?}");
            assert!(!off.why.contains("test-secret") && !off.why.contains("test-key-id"), "{off:?}");
        }
        let s3 = S3Ci::from_vars(&vars(&S3_VARS), "macOS", "aarch64").unwrap();
        assert_eq!((s3.key.0.expose(), s3.key.1.expose()), ("test-key-id", "test-secret"));
        let shown = format!("{s3:?}");
        assert!(!shown.contains("test-secret") && !shown.contains("test-key-id"), "{shown}");
    }

    #[test]
    fn the_server_gets_the_chain_and_the_job_only_the_disk() {
        let s3 = S3Ci::from_vars(&vars(&S3_VARS), "Linux", "x86_64").unwrap();
        let server = s3_server_settings(&s3, "/sccache", "20G");
        let get = |s: &[(String, String)], k: &str| s.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(
            get(&server, "SCCACHE_MULTILEVEL_CHAIN").as_deref(),
            Some("disk,s3"),
            "the local disk is level 0"
        );
        assert_eq!(
            get(&server, "SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY").as_deref(),
            Some("l0"),
            "an S3 failure never fails a compile"
        );
        assert_eq!(get(&server, "SCCACHE_S3_KEY_PREFIX").as_deref(), Some("paddock-ci/Linux-x86_64"));
        // What every later step gets: no chain, no bucket, no key; the survival settings and the error log.
        let build = s3_build_settings("/sccache", "20G", "/tmp/sccache-error.log");
        let lines = env_lines("4227", &build, false).join("\n");
        for gone in [
            "MULTILEVEL",
            "SCCACHE_BUCKET",
            "SCCACHE_ENDPOINT",
            "SCCACHE_S3",
            "AWS_ACCESS",
            "AWS_SECRET",
            "test-secret",
            "test-key-id",
        ] {
            assert!(!lines.contains(gone), "`{gone}` in {lines}");
        }
        for want in [
            "SCCACHE_DIR=/sccache",
            "SCCACHE_IDLE_TIMEOUT=0",
            "AWS_EC2_METADATA_DISABLED=true",
            "SCCACHE_ERROR_LOG=/tmp/sccache-error.log",
        ] {
            assert!(lines.contains(want), "`{want}` missing in {lines}");
        }
    }

    #[test]
    fn a_release_or_tag_run_never_reads_the_shared_cache_even_with_a_key() {
        let with = |extra: &[(&'static str, &'static str)]| {
            let mut v = S3_VARS.to_vec();
            v.extend_from_slice(extra);
            S3Ci::from_vars(&vars(&v), "macOS", "aarch64")
        };
        assert!(with(&[]).is_ok(), "the control: the same inputs on a branch run are accepted");
        // The log names the real trigger (ER-27).
        for (extra, named) in [
            (("GITHUB_REF", "refs/tags/dyno-v1.2.0"), "GITHUB_REF is refs/tags/dyno-v1.2.0"),
            (("PADDOCK_TAG_REHEARSAL", "1"), "PADDOCK_TAG_REHEARSAL=1"),
            (("PADDOCK_CI_RELEASE", "true"), "PADDOCK_CI_RELEASE=true"),
        ] {
            let off = with(&[extra]).unwrap_err();
            assert!(
                !off.warn && off.why.contains("release or tag") && off.why.contains(named),
                "{extra:?}: {off:?}"
            );
        }
        // PADDOCK_DEV_DOWNLOAD=0 is not a release signal: setup-rust sets it for any job without a read token (ER-33: one flag, one meaning).
        assert!(
            with(&[("PADDOCK_DEV_DOWNLOAD", "0")]).is_ok(),
            "a job with no read token still uses the store"
        );
        assert!(
            with(&[("PADDOCK_CI_RELEASE", "false")]).is_ok(),
            "the release flag false is a branch run"
        );
        assert!(with(&[("GITHUB_REF", "refs/heads/main")]).is_ok(), "main is not a tag");
    }

    #[test]
    fn the_health_check_is_an_http_get_of_the_stores_own_endpoint() {
        let s3 = S3Ci::from_vars(&vars(&S3_VARS), "Linux", "x86_64").unwrap();
        assert_eq!(s3_health_url(s3.ssl, &s3.endpoint), "http://192.0.2.10:15000/minio/health/live");
        let mut v = S3_VARS.to_vec();
        v[1] = ("PADDOCK_SCCACHE_S3_ENDPOINT", "https://s3.example.test:443");
        assert_eq!(
            s3_health_url_of(&S3Ci::from_vars(&vars(&v), "Linux", "x86_64").unwrap()),
            "https://s3.example.test:443/minio/health/live"
        );
    }

    #[test]
    fn an_endpoint_with_a_user_or_password_is_refused() {
        let mut v = S3_VARS.to_vec();
        v[1] = ("PADDOCK_SCCACHE_S3_ENDPOINT", "http://user:pw@192.0.2.10:15000");
        let off = S3Ci::from_vars(&vars(&v), "Linux", "x86_64").unwrap_err();
        assert!(off.warn && off.why.contains('@') && !off.why.contains("pw"), "{off:?}");
    }

    #[test]
    fn a_slot_gets_its_share_of_the_budget_and_at_least_two_gigabytes() {
        assert_eq!(slot_cache_size(None, 4), "10G");
        assert_eq!(slot_cache_size(Some("60"), 3), "20G");
        assert_eq!(slot_cache_size(Some("x1"), 4), "10G", "not a number: the default 40");
        assert_eq!(slot_cache_size(Some(""), 4), "10G");
        assert_eq!(slot_cache_size(Some("3"), 4), "2G");
    }

    #[test]
    fn the_clients_start_wait_is_above_the_s3_guards_stall_so_the_servers_storage_check_can_finish() {
        // The server's start runs a storage check (a GET and a PUT of `.sccache_check`); a store that hangs is cut by the guard only
        // after its stall. The client's 10 s default gave up at the same moment (run 9274), so the tolerant l0 policy never ran.
        let stall = paddock_net::stall_guard::Limits::default().stall;
        assert!(
            Duration::from_millis(SERVER_STARTUP_TIMEOUT_MS) > stall,
            "{SERVER_STARTUP_TIMEOUT_MS} ms must exceed the guard's {stall:?}"
        );
        assert_eq!(server_conf_text(), "server_startup_timeout_ms = 30000\n");
        let parsed: toml::Table = server_conf_text().parse().expect("sccache reads its config file as TOML");
        assert_eq!(
            parsed["server_startup_timeout_ms"].as_integer(),
            Some(SERVER_STARTUP_TIMEOUT_MS as i64)
        );
    }

    #[test]
    fn the_environment_lines_are_the_wrapper_then_the_settings() {
        let lines = env_lines(
            "4228",
            &[("SCCACHE_DIR".into(), "/d".into()), ("SCCACHE_CACHE_SIZE".into(), "10G".into())],
            false,
        );
        assert_eq!(
            lines,
            [
                "RUSTC_WRAPPER=sccache",
                "CARGO_INCREMENTAL=0",
                "CARGO_TERM_COLOR=always",
                "SCCACHE_IGNORE_SERVER_IO_ERROR=1",
                "SCCACHE_SERVER_PORT=4228",
                "SCCACHE_DIR=/d",
                "SCCACHE_CACHE_SIZE=10G"
            ]
        );
    }

    // Run 8027 (job 37099): under cargo 1.100's build-dir layout, rustc for spotter_app_lib had 313 `-L dependency=` arguments, about
    // 38,000 characters; cargo fell back to an @argfile, sccache 0.19 expanded it and could not start rustc (os error 206).
    #[test]
    fn on_windows_the_wrapper_comes_with_cargos_legacy_build_dir_layout() {
        let windows = env_lines("4227", &[], true);
        assert!(
            windows.contains(&"__CARGO_TEMPORARY_BUILD_DIR_NEW_LAYOUT_OPT_OUT=1".to_string()),
            "{windows:?}"
        );
        assert_eq!(windows[0], "RUSTC_WRAPPER=sccache");
        let unix = env_lines("4227", &[], false);
        assert!(!unix.iter().any(|l| l.starts_with("__CARGO")), "{unix:?}");
    }

    #[test]
    fn the_password_of_an_endpoint_is_found_for_masking() {
        assert_eq!(endpoint_password("redis://:pw@h:6379").as_deref(), Some("pw"));
        assert_eq!(endpoint_password("redis://user:p:w@h").as_deref(), Some("p:w"));
        assert_eq!(endpoint_password("redis://token@h").as_deref(), Some("token"));
        assert_eq!(endpoint_password("redis://h:6379"), None);
        assert_eq!(endpoint_password("redis://:@h"), None);
        assert_eq!(endpoint_password(""), None);
    }

    #[test]
    fn a_password_in_the_endpoint_is_never_shown() {
        assert_eq!(shown_endpoint("redis://h:6379"), "redis://h:6379");
        assert_eq!(shown_endpoint("redis://:pw@h:6379"), "redis://***@h:6379");
        assert_eq!(shown_endpoint("redis://u:p@w@h"), "redis://***@h");
    }

    /// The success path of the download, which the shell's step cannot be pointed at: a release folder (`file://`) with an
    /// archive whose checksum is the test's own pin. On a Unix runner and (by its program name) on a Windows one.
    #[cfg(unix)]
    #[test]
    fn a_download_that_matches_its_pin_is_unpacked_and_put_in_place_whole() {
        use std::os::unix::fs::PermissionsExt;
        for os in ["macOS", "Windows"] {
            let dir = tempfile::tempdir().unwrap();
            let root = paddock_env::winpath::real(dir.path()).unwrap();
            let exe = if os == "Windows" { "sccache.exe" } else { "sccache" };
            let name = format!("sccache-v{SCCACHE_VERSION}-test-target");
            let stage = root.join("stage").join(&name);
            std::fs::create_dir_all(&stage).unwrap();
            std::fs::write(stage.join(exe), "sccache 0.20.1 stand-in bytes\n").unwrap();
            let rel = root.join("rel").join(format!("v{SCCACHE_VERSION}"));
            std::fs::create_dir_all(&rel).unwrap();
            let archive = rel.join(format!("{name}.tar.gz"));
            let packed = paddock_proc::command("tar")
                .arg("-czf")
                .arg(&archive)
                .arg("-C")
                .arg(root.join("stage"))
                .arg(&name)
                .status()
                .unwrap();
            assert!(packed.success());
            let pin = Pin {
                target: "test-target",
                ext: "tar.gz",
                sha256: Box::leak(sha256_file(&archive).unwrap().into_boxed_str()),
            };
            let temp = root.join("temp");
            std::fs::create_dir_all(&temp).unwrap();
            let releases = format!("file://{}", paddock_testkit::path::text(root.join("rel")));
            let ctx = crate::ci_isolation::testkit::ctx(&[
                ("PATH", "/usr/bin:/bin"),
                ("RUNNER_OS", os),
                ("TMPDIR", temp.to_str().unwrap()),
                ("PADDOCK_SCCACHE_RELEASES", releases.as_str()),
            ]);
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let mut step = Step {
                ctx,
                io: Sinks {
                    out: &mut out,
                    err: &mut err,
                },
            };
            let dest = root.join("tools/sccache").join(SCCACHE_VERSION);
            let bin = dest.join(exe);
            assert_eq!(step.download(pin, &dest, &bin), Ok(true), "{os}: {}", String::from_utf8_lossy(&out));
            assert_eq!(std::fs::metadata(&bin).unwrap().permissions().mode() & 0o777, 0o755, "{os}");
            assert_eq!(std::fs::read_to_string(&bin).unwrap(), "sccache 0.20.1 stand-in bytes\n");
            let left: Vec<_> = std::fs::read_dir(&dest).unwrap().map(|e| e.unwrap().file_name()).collect();
            assert_eq!(left.len(), 1, "{os}: no part file is left: {left:?}");
            assert_eq!(std::fs::read_dir(&temp).unwrap().count(), 0, "{os}: the download folder is removed");
            // A wrong pin for the same archive is a mismatch, and nothing is replaced.
            let wrong = Pin {
                sha256: "0".repeat(64).leak(),
                ..pin
            };
            assert_eq!(step.download(wrong, &dest, &bin), Err(1));
            assert!(String::from_utf8_lossy(&out).contains("checksum mismatch for test-target"));
        }
    }

    #[test]
    fn the_pins_are_the_ci_images() {
        // The image installs the same release (ci-image/Dockerfile); scripts/ci-image.test.mjs checks the action's copy.
        let dockerfile = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ci-image/Dockerfile")).unwrap();
        assert!(dockerfile.contains(&format!("SCCACHE_VERSION={SCCACHE_VERSION}")));
        assert!(dockerfile.contains(pin_for("Linux", "x86_64").unwrap().sha256));
        assert!(dockerfile.contains(pin_for("Linux", "aarch64").unwrap().sha256));
    }
}
