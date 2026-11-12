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
//!    slot's own port), the local folder and size (the `/sccache` mount in a container, else the slot's kept folder with the
//!    host's budget divided between the slots), a fresh server for this job, and `RUSTC_WRAPPER=sccache` with the settings in
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
use clap::Args;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The pinned sccache release, the same on every OS (one version, one cache format). The CI image pins the same
/// (`ci-image/Dockerfile`, checked by scripts/ci-image.test.mjs).
pub const SCCACHE_VERSION: &str = "0.19.0";
const RELEASES: &str = "https://github.com/mozilla/sccache/releases/download";
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
    /// local (sccache's disk cache), auto (Redis when PADDOCK_SCCACHE_REDIS_ENDPOINT is set, else local) or redis.
    #[arg(long, default_value = "local")]
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
            "aa73c7270c997894038c0d8f0d297865f57569e1687883322bc431713d4bb9dd",
        ),
        ("Linux", "aarch64" | "arm64") => pin(
            "aarch64-unknown-linux-musl",
            "tar.gz",
            "3e47fc4a71b1ddd1b4294a0de9d34498922fdf41e3cd0bda5df3954e3d6deb9d",
        ),
        ("macOS", "arm64" | "aarch64") => pin(
            "aarch64-apple-darwin",
            "tar.gz",
            "a35a0b7d622618774bc1bb8102d72562dfb21e619f88b153b370d7f89bc286af",
        ),
        ("Windows", "x86_64") => pin(
            "x86_64-pc-windows-msvc",
            "zip",
            "28be8c61f6b2c89b9eb2869f6af7f78e6f992e78a8abae0ce855fcc129cbeac1",
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
}

/// The backend the inputs ask for, or the `::error::` text when they are wrong.
pub fn choose_backend(backend: &str, endpoint: &str, db: &str) -> Result<Backend, String> {
    if !matches!(backend, "auto" | "redis" | "local") {
        return Err(error_line(
            &format!("sccache-backend must be auto, redis or local (got '{backend}')."),
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
    let total = total_gb
        .filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or(40);
    let per = (total / slots.max(1)).max(2);
    format!("{per}G")
}

/// The lines for `$GITHUB_ENV`, in the shell's order.
pub fn env_lines(port: &str, settings: &[(String, String)]) -> Vec<String> {
    let mut lines = vec![
        "RUSTC_WRAPPER=sccache".to_string(),
        "CARGO_INCREMENTAL=0".to_string(),
        "SCCACHE_IGNORE_SERVER_IO_ERROR=1".to_string(),
        format!("SCCACHE_SERVER_PORT={port}"),
    ];
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
        self.get(key).map(|v| home::from_git_bash(v, self.ctx.windows))
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
        let mut settings: Vec<(String, String)> = Vec::new();
        match &backend {
            Backend::Redis { endpoint, db } => {
                settings.push(("SCCACHE_REDIS_ENDPOINT".into(), endpoint.clone()));
                settings.push(("SCCACHE_REDIS_DB".into(), db.clone()));
                self.say(format!("sccache: shared backend {} db {db}", shown_endpoint(endpoint)));
            }
            Backend::Local => {
                let (dir, size) = self.local_cache()?;
                settings.push(("SCCACHE_DIR".into(), dir.clone()));
                settings.push(("SCCACHE_CACHE_SIZE".into(), size));
                self.say(format!("sccache: local disk {dir}"));
            }
        }
        let secret = endpoint
            .rsplit_once('@')
            .map(|(userinfo, _)| userinfo.to_string())
            .unwrap_or_default();
        let password = endpoint_password(&endpoint).unwrap_or_default();
        let call = |a: &str| {
            let mut c = ToolCall::new(sccache.to_string_lossy())
                .args([a])
                .env("SCCACHE_SERVER_PORT", port.as_str())
                .env("SCCACHE_IGNORE_SERVER_IO_ERROR", "1")
                .secret(secret.clone())
                .secret(password.clone())
                .pass(SCCACHE_PASS)
                .timeout(Duration::from_secs(60));
            for (k, v) in &settings {
                c = c.env(k.as_str(), v.as_str());
            }
            c
        };
        if let Ok(r) = self.run(&call("--version")) {
            let v = r.stdout.trim().to_string();
            self.say(v);
        }
        // A server left on this port by an earlier job would keep that job's backend: start it fresh with this job's settings.
        let _ = self.run(&call("--stop-server"));
        // The server outlives this step and would hold a pipe open, so its starter writes to a file.
        let log = self
            .temp_base()
            .join(format!("paddock-sccache-start-{}-{}.log", std::process::id(), nanos()));
        let started = self.run(&call("--start-server").output(Output::File(log.clone())));
        let _ = std::fs::remove_file(&log);
        let answers = matches!(&started, Ok(r) if r.success) && matches!(self.run(&call("--show-stats")), Ok(r) if r.success);
        if !answers {
            // The cache must slow a build down, never fail it: RUSTC_WRAPPER stays unset and the job builds without sccache.
            if let Ok(r) = &started
                && !r.success
            {
                let t = tail(&r.stderr, TAIL_LINES);
                self.say(t);
            }
            let _ = self.run(&call("--stop-server"));
            self.say("::warning::the sccache server did not start; the build runs without the compile cache.");
            return Ok(());
        }
        // sccache itself is the wrapper, never a script around it (Windows starts a .cmd through cmd.exe, whose 8191-character
        // command line rustc's arguments for a large crate exceed). Cached builds are not incremental.
        self.append("GITHUB_ENV", &env_lines(&port, &settings))
    }

    /// The local disk cache: a host slot's kept folder with its share of the budget, else the container's mount (or a folder
    /// that dies with the container, with a notice).
    fn local_cache(&mut self) -> Result<(String, String), i32> {
        let (dir, size) = if self.os() == "Linux" {
            let mount = self.path_var("PADDOCK_SCCACHE_MOUNT").unwrap_or_else(|| "/sccache".to_string());
            if Path::new(&mount).is_dir() && dir_writable(Path::new(&mount)) {
                (mount, "20G".to_string())
            } else {
                let temp = self.path_var("RUNNER_TEMP").unwrap_or_else(|| "/tmp".to_string());
                self.say("::notice::no /sccache mount in this container; the compile cache lives and dies with the job.");
                (format!("{temp}/sccache"), "20G".to_string())
            }
        } else {
            // sccache's local storage "only supports a single sccache server at a time" (docs/Local.md): one folder per slot.
            let dir = self.isolation(&["path", "sccache"])?;
            let slots = self.isolation(&["slots"])?;
            let slots: u64 = slots.parse().unwrap_or(1);
            let size = slot_cache_size(self.get("PADDOCK_CI_SCCACHE_GB"), slots);
            (home::from_git_bash(&dir, self.ctx.windows), size)
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return Err(self.fail(error_line(
                &format!("cannot make the sccache folder {dir}: {e}."),
                "check the disk of the runner.",
            )));
        }
        let dir = if self.os() == "Windows" { home::native(&dir) } else { dir };
        Ok((dir, size))
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
    let env: HashMap<String, String> = std::env::vars().collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let tables = ci_isolation::find_tables(None, &env, &cwd);
    let exe = std::env::current_exe().unwrap_or_default();
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

fn sha256_file(path: &Path) -> std::io::Result<String> {
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
            bad.starts_with("::error::sccache-backend must be auto, redis or local (got 'disk')"),
            "{bad}"
        );
        assert!(
            choose_backend("redis", "", "")
                .unwrap_err()
                .contains("sccache-backend is redis but sccache-redis-endpoint is empty")
        );
        assert!(choose_backend("", "", "").is_err(), "an empty input is refused, as in the shell");
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
    fn the_environment_lines_are_the_wrapper_then_the_settings() {
        let lines = env_lines(
            "4228",
            &[("SCCACHE_DIR".into(), "/d".into()), ("SCCACHE_CACHE_SIZE".into(), "10G".into())],
        );
        assert_eq!(
            lines,
            [
                "RUSTC_WRAPPER=sccache",
                "CARGO_INCREMENTAL=0",
                "SCCACHE_IGNORE_SERVER_IO_ERROR=1",
                "SCCACHE_SERVER_PORT=4228",
                "SCCACHE_DIR=/d",
                "SCCACHE_CACHE_SIZE=10G"
            ]
        );
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
            let root = dir.path().canonicalize().unwrap();
            let exe = if os == "Windows" { "sccache.exe" } else { "sccache" };
            let name = format!("sccache-v{SCCACHE_VERSION}-test-target");
            let stage = root.join("stage").join(&name);
            std::fs::create_dir_all(&stage).unwrap();
            std::fs::write(stage.join(exe), "#!/bin/sh\necho sccache 0.19.0\n").unwrap();
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
            let releases = format!("file://{}", root.join("rel").display());
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
            assert_eq!(std::fs::read_to_string(&bin).unwrap(), "#!/bin/sh\necho sccache 0.19.0\n");
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
