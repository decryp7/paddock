//! `dev fetch` (ADR 0022, R3): the pinned downloads of the repository, on the shared crates. Each command is one job on a
//! small pool (`paddock-jobs`), so the command thread only waits, progress is phase-labelled on standard error from the first
//! moment, Ctrl+C cancels in every phase and a cancelled or failed fetch leaves nothing half-done in place.
//!
//! - `chrome`: the Chrome for Testing build a `paddock-runtime` catalogue pins ([`chrome`]);
//! - `libduckdb`: Debrief's prebuilt libduckdb, against its pinned SHA-256 ([`libduckdb`]);
//! - `pin`: re-pins a catalogue (version, URLs, sizes, checksums) ([`pin`]);
//! - `llama`, `ffmpeg`: re-pin the runtime manifests Debrief embeds ([`llama`], [`ffmpeg`]).
//!
//! **Online access** is limited to what each command names: Chrome for Testing's storage and channel list, the DuckDB release
//! on GitHub, the URLs of the catalogue being pinned, and the GitHub release API of the one repository a manifest comes from.
//! Nothing else is contacted, and nothing is downloaded unless a command is run.

pub mod chrome;
pub mod error;
pub mod ffmpeg;
pub mod github;
pub mod libduckdb;
pub mod llama;
pub mod pin;
pub mod runner;
#[cfg(test)]
mod testing;

use crate::fetch::error::FetchError;
use crate::fetch::runner::{DEFAULT_JOBS, MAX_JOBS, Runner};
use clap::{Args, Subcommand};
use paddock_cli::{CliError, Report};
use paddock_runtime::{Catalogue, Platform};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The `dev fetch` commands.
#[derive(Subcommand)]
pub enum Command {
    /// Install the pinned Chrome for Testing build (tests never launch an installed Chrome). Prints CHROMIUM_TEST_PATH=<program>.
    Chrome(ChromeArgs),
    /// Fetch Debrief's pinned libduckdb and print the lines that point a build at it (`eval "$(./dev fetch libduckdb)"`).
    Libduckdb(LibduckdbArgs),
    /// Re-pin a paddock-runtime catalogue: new version and URLs, and the size and SHA-256 of every archive.
    Pin(PinArgs),
    /// Re-pin the llama.cpp runtime manifest (llama-runtimes.json).
    Llama(LlamaArgs),
    /// Re-pin the FFmpeg runtime manifest (ffmpeg-runtimes.json).
    Ffmpeg(FfmpegArgs),
}

/// Options every command takes.
#[derive(Args)]
pub struct Common {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    root: Option<PathBuf>,
    /// How many archives to fetch at once, 1 to 8 (default 3: the archives sit on one or two hosts, each download already uses
    /// up to four connections, and the unpack is disk-bound).
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=8))]
    jobs: Option<u8>,
}

/// `dev fetch chrome`.
#[derive(Args)]
pub struct ChromeArgs {
    #[command(flatten)]
    common: Common,
    /// The catalogue to install from (default: crates/paddock-chromium/src/chromium.toml).
    #[arg(long)]
    catalogue: Option<PathBuf>,
    /// Where to install (default: $PADDOCK_TEST_BROWSER_DIR, else ~/.cache/paddock-test-browser).
    #[arg(long)]
    dir: Option<PathBuf>,
    /// The catalogue key to install, such as linux-x86_64 (default: this machine's).
    #[arg(long)]
    platform: Option<String>,
    /// Also print <PREFIX>_REQUIRE_TEST_BROWSER=1, so a missing browser fails the tests instead of skipping them.
    #[arg(long)]
    env_prefix: Option<String>,
}

/// `dev fetch libduckdb`.
#[derive(Args)]
pub struct LibduckdbArgs {
    #[command(flatten)]
    common: Common,
    /// Where to unpack (default: $CARGO_TARGET_DIR or apps/debrief/target, then libduckdb/).
    #[arg(long)]
    cache: Option<PathBuf>,
    /// Print lines for this shell: sh or powershell (default: powershell on Windows, sh elsewhere).
    #[arg(long)]
    shell: Option<String>,
    /// Also write apps/debrief/.cargo/libduckdb.toml, so plain cargo build and cargo test link this library.
    #[arg(long)]
    cargo_config: bool,
}

/// `dev fetch pin`.
#[derive(Args)]
pub struct PinArgs {
    #[command(flatten)]
    common: Common,
    /// The catalogue to re-pin (rewritten in place; comments and order stay).
    catalogue: PathBuf,
    /// Move every URL from the pinned version to this one.
    #[arg(long, conflicts_with = "chrome_for_testing")]
    version: Option<String>,
    /// Take the version and URLs of a Chrome for Testing channel (Stable, Beta, Dev, Canary).
    #[arg(long, num_args = 0..=1, default_missing_value = "Stable", value_name = "CHANNEL")]
    chrome_for_testing: Option<String>,
    /// The Chrome for Testing channel list (default: Google's).
    #[arg(long, default_value = pin::CFT_ENDPOINT)]
    endpoint: String,
    /// Where the archives are streamed while they are measured (each is removed at once; default: a private folder beside the catalogue).
    #[arg(long)]
    work: Option<PathBuf>,
}

/// `dev fetch llama`.
#[derive(Args)]
pub struct LlamaArgs {
    #[command(flatten)]
    common: Common,
    /// A build tag such as b11669 (default: the release GitHub marks as latest).
    #[arg(long)]
    tag: Option<String>,
    /// Also download every chosen asset and check its SHA-256.
    #[arg(long)]
    verify: bool,
    /// Write the manifest here (repeatable; default: apps/debrief/crates/debrief-ai/src/runtimes/llama-runtimes.json).
    #[arg(long)]
    out: Vec<PathBuf>,
    /// Where --verify streams the assets (each is removed at once; default: a private folder beside the output).
    #[arg(long)]
    work: Option<PathBuf>,
}

/// `dev fetch ffmpeg`.
#[derive(Args)]
pub struct FfmpegArgs {
    #[command(flatten)]
    common: Common,
    /// An autobuild tag such as autobuild-2026-09-30-13-08 (default: the newest month-end autobuild).
    #[arg(long)]
    tag: Option<String>,
    /// The FFmpeg release branch the builds follow.
    #[arg(long, default_value = ffmpeg::DEFAULT_BRANCH)]
    branch: String,
    /// Also download every chosen asset and check its SHA-256.
    #[arg(long)]
    verify: bool,
    /// Write the manifest here (repeatable; default: apps/debrief/crates/debrief-video/src/runtime/ffmpeg-runtimes.json).
    #[arg(long)]
    out: Vec<PathBuf>,
    /// Where --verify streams the assets (each is removed at once; default: a private folder beside the output).
    #[arg(long)]
    work: Option<PathBuf>,
}

/// What the commands read from the machine, passed in so tests need no environment.
#[derive(Debug, Clone)]
pub struct Env {
    /// The repository root.
    pub root: PathBuf,
    /// The user's home folder.
    pub home: Option<PathBuf>,
    /// `$PADDOCK_TEST_BROWSER_DIR`.
    pub test_browser_dir: Option<PathBuf>,
    /// `$GITHUB_ENV`: the file a CI step appends exports to.
    pub github_env: Option<PathBuf>,
    /// `$GITHUB_TOKEN`.
    pub github_token: Option<String>,
    /// `$CARGO_TARGET_DIR`.
    pub cargo_target_dir: Option<String>,
    /// `std::env::consts::OS`.
    pub os: String,
    /// `std::env::consts::ARCH`.
    pub arch: String,
}

impl Env {
    /// The environment of this process, for the repository at `root`.
    pub fn process(root: PathBuf) -> Env {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        Env {
            root,
            home: var("HOME").or_else(|| var("USERPROFILE")).map(PathBuf::from),
            test_browser_dir: var("PADDOCK_TEST_BROWSER_DIR").map(PathBuf::from),
            github_env: var("GITHUB_ENV").map(PathBuf::from),
            github_token: var("GITHUB_TOKEN").and_then(|v| v.into_string().ok()),
            cargo_target_dir: var("CARGO_TARGET_DIR").and_then(|v| v.into_string().ok()),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        }
    }

    /// The folder for streamed downloads: the one given, else a private folder beside `beside` (the file about to be rewritten),
    /// so the disk is the one the result lands on and no system temporary folder is used.
    fn work(given: &Option<PathBuf>, beside: &Path) -> Work {
        match given {
            Some(p) => Work {
                dir: p.clone(),
                owned: false,
            },
            None => {
                let parent = beside.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
                Work {
                    dir: parent.join(format!(".paddock-dev-fetch-{}", std::process::id())),
                    owned: true,
                }
            }
        }
    }
}

/// A folder for streamed downloads: removed afterwards when this command made the choice.
struct Work {
    dir: PathBuf,
    owned: bool,
}

impl Drop for Work {
    fn drop(&mut self) {
        if self.owned {
            let _ = std::fs::remove_dir_all(&self.dir);
        } else {
            let _ = std::fs::remove_dir(&self.dir);
        }
    }
}

impl Command {
    fn common(&self) -> &Common {
        match self {
            Command::Chrome(a) => &a.common,
            Command::Libduckdb(a) => &a.common,
            Command::Pin(a) => &a.common,
            Command::Llama(a) => &a.common,
            Command::Ffmpeg(a) => &a.common,
        }
    }
}

/// The repository root: `--root`, else the folder containing `.git` found from the current folder.
fn find_root(given: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(r) = given {
        return Ok(r.to_path_buf());
    }
    let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
    let out = crate::input_hash::Exec::run(&crate::input_hash::Real, "git", &["rev-parse", "--show-toplevel"], &cwd, None)?;
    Ok(PathBuf::from(out.trim()))
}

/// `dev fetch ...` on the real network and machine.
pub fn command(c: Command) -> Result<Report, CliError> {
    let jobs = c.common().jobs.map_or(DEFAULT_JOBS, usize::from).clamp(1, MAX_JOBS);
    let env = Env::process(find_root(c.common().root.as_deref())?);
    let runner = Runner::http(jobs)?;
    runner::cancel_on_ctrl_c(&runner);
    Ok(run(&runner, &env, c)?)
}

/// Runs one command over `runner` and `env`.
pub fn run(runner: &Runner, env: &Env, c: Command) -> Result<Report, FetchError> {
    match c {
        Command::Chrome(a) => run_chrome(runner, env, &a),
        Command::Libduckdb(a) => run_libduckdb(runner, env, &a),
        Command::Pin(a) => run_pin(runner, &a),
        Command::Llama(a) => run_llama(runner, env, &a),
        Command::Ffmpeg(a) => run_ffmpeg(runner, env, &a),
    }
}

fn read(path: &Path) -> Result<String, FetchError> {
    std::fs::read_to_string(path).map_err(|e| FetchError::io(path.display(), &e))
}

fn run_chrome(runner: &Runner, env: &Env, a: &ChromeArgs) -> Result<Report, FetchError> {
    if let Some(p) = a.env_prefix.as_deref().filter(|p| !chrome::prefix_ok(p)) {
        return Err(FetchError::Usage(format!(
            "--env-prefix '{p}' must be upper-case letters, digits and underscores, starting with a letter."
        )));
    }
    let platform = match &a.platform {
        Some(k) => Platform::parse(k).ok_or_else(|| FetchError::Usage(format!("'{k}' is not a platform key such as linux-x86_64.")))?,
        None => Platform::from_consts(&env.os, &env.arch).ok_or_else(|| {
            FetchError::Usage(format!(
                "No browser is pinned for {}-{}; name a platform with --platform.",
                env.os, env.arch
            ))
        })?,
    };
    let path = a
        .catalogue
        .clone()
        .unwrap_or_else(|| env.root.join("crates/paddock-chromium/src/chromium.toml"));
    let catalogue = Catalogue::parse(&read(&path)?).map_err(|e| FetchError::failed(format!("{}: {e}.", path.display())))?;
    let dir = a
        .dir
        .clone()
        .or_else(|| env.test_browser_dir.clone())
        .or_else(|| env.home.as_ref().map(|h| h.join(".cache").join("paddock-test-browser")))
        .ok_or_else(|| FetchError::Usage("No install folder: pass --dir or set PADDOCK_TEST_BROWSER_DIR.".into()))?;
    let installed = chrome::install(runner, &catalogue, platform, &dir)?;
    let lines = chrome::env_lines(&installed.executable, a.env_prefix.as_deref());
    if let Some(file) = &env.github_env {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)
            .map_err(|e| FetchError::io(file.display(), &e))?;
        writeln!(f, "{}", lines.join("\n")).map_err(|e| FetchError::io(file.display(), &e))?;
    }
    let data = json!({"executable": installed.executable.display().to_string(), "version": installed.version, "cached": installed.cached,
        "platform": platform.key(), "env": lines});
    Ok(Report::new(data, lines.join("\n")))
}

fn run_libduckdb(runner: &Runner, env: &Env, a: &LibduckdbArgs) -> Result<Report, FetchError> {
    let shell = match a.shell.as_deref() {
        None => libduckdb::Shell::host(),
        Some(s) => libduckdb::Shell::parse(s).ok_or_else(|| FetchError::Usage(format!("--shell '{s}' is not sh or powershell.")))?,
    };
    let pins = libduckdb::read_pins(&env.root)?;
    let app = env.root.join(libduckdb::APP);
    let cache = a
        .cache
        .clone()
        .unwrap_or_else(|| libduckdb::default_cache(&app, env.cargo_target_dir.as_deref()));
    let fetched = libduckdb::fetch(runner, &pins, &env.os, &env.arch, &cache)?;
    let mut lines = libduckdb::export_lines(shell, &env.os, &fetched.dir);
    let mut wrote = Value::Null;
    if a.cargo_config {
        let path = libduckdb::write_cargo_config(&app, &fetched)?;
        // A comment in both shells, so the output still evaluates.
        lines.push(format!(
            "# wrote {}; cargo build and cargo test now link this libduckdb",
            path.display()
        ));
        wrote = json!(path.display().to_string());
    }
    let data = json!({"version": fetched.version, "asset": fetched.asset, "dir": fetched.dir.display().to_string(), "cached": fetched.cached,
        "exports": lines, "cargoConfig": wrote});
    Ok(Report::new(data, lines.join("\n")))
}

fn run_pin(runner: &Runner, a: &PinArgs) -> Result<Report, FetchError> {
    let target = match (&a.version, &a.chrome_for_testing) {
        (Some(v), _) => pin::Target::Version(v.clone()),
        (None, Some(c)) => pin::Target::ChromeForTesting {
            channel: c.clone(),
            endpoint: a.endpoint.clone(),
        },
        (None, None) => pin::Target::Same,
    };
    let text = read(&a.catalogue)?;
    let work = Env::work(&a.work, &a.catalogue);
    let out = pin::repin(runner, &text, &target, &work.dir)?;
    pin::write_atomic(&a.catalogue, &out.text)?;
    let platforms: Vec<Value> = out
        .platforms
        .iter()
        .map(|p| json!({"key": p.key, "url": p.url, "size": p.size, "sha256": p.sha256}))
        .collect();
    let summary = format!(
        "{}: pinned {} ({} platforms); review the diff before committing.",
        a.catalogue.display(),
        out.version,
        out.platforms.len()
    );
    Ok(Report::new(
        json!({"catalogue": a.catalogue.display().to_string(), "version": out.version, "platforms": platforms}),
        summary,
    ))
}

fn manifest_report(outs: &[PathBuf], manifest: &Value, notes: &[String], verified: usize, tag_line: &str) -> Report {
    let mut text = notes.iter().map(|n| format!("note: {n}")).collect::<Vec<_>>();
    if verified > 0 {
        text.push(format!("Verified {verified} assets against their SHA-256."));
    }
    for o in outs {
        text.push(format!("Wrote {} ({tag_line}); review the diff before committing.", o.display()));
    }
    let files: Vec<String> = outs.iter().map(|o| o.display().to_string()).collect();
    Report::new(
        json!({"written": files, "tag": manifest["tag"], "release": manifest["release"], "notes": notes, "verified": verified}),
        text.join("\n"),
    )
}

fn run_llama(runner: &Runner, env: &Env, a: &LlamaArgs) -> Result<Report, FetchError> {
    let out = if a.out.is_empty() {
        vec![llama::default_out(&env.root)]
    } else {
        a.out.clone()
    };
    let work = Env::work(&a.work, &out[0]);
    let opts = llama::Options {
        tag: a.tag.clone(),
        verify: a.verify,
        out: out.clone(),
        token: env.github_token.clone(),
        work: work.dir.clone(),
    };
    let pinned = llama::pin(runner, &opts)?;
    let line = format!("llama.cpp {}", pinned.manifest["tag"].as_str().unwrap_or(""));
    Ok(manifest_report(&out, &pinned.manifest, &pinned.notes, pinned.verified, &line))
}

fn run_ffmpeg(runner: &Runner, env: &Env, a: &FfmpegArgs) -> Result<Report, FetchError> {
    let out = if a.out.is_empty() {
        vec![ffmpeg::default_out(&env.root)]
    } else {
        a.out.clone()
    };
    let work = Env::work(&a.work, &out[0]);
    let opts = ffmpeg::Options {
        tag: a.tag.clone(),
        branch: a.branch.clone(),
        verify: a.verify,
        out: out.clone(),
        token: env.github_token.clone(),
        work: work.dir.clone(),
    };
    let pinned = ffmpeg::pin(runner, &opts)?;
    let line = format!(
        "FFmpeg {} from {}",
        pinned.manifest["release"].as_str().unwrap_or(""),
        pinned.manifest["tag"].as_str().unwrap_or("")
    );
    Ok(manifest_report(&out, &pinned.manifest, &pinned.notes, pinned.verified, &line))
}

#[cfg(test)]
mod tests {
    use super::testing::{FakeNet, runner, sha256_hex, zip_of};
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Wrap {
        #[command(subcommand)]
        c: Command,
    }

    fn parse(args: &[&str]) -> Command {
        Wrap::try_parse_from(std::iter::once("dev").chain(args.iter().copied())).unwrap().c
    }

    fn env(root: &Path) -> Env {
        Env {
            root: root.to_path_buf(),
            home: Some(root.join("home")),
            test_browser_dir: None,
            github_env: None,
            github_token: None,
            cargo_target_dir: None,
            os: "linux".into(),
            arch: "x86_64".into(),
        }
    }

    fn chromium_toml(zip: &[u8]) -> String {
        format!(
            "format = 1\nname = \"chromium\"\nversion = \"9.9.9\"\n[[platform]]\nkey = \"linux-x86_64\"\nurl = \"https://dl.test/9.9.9/linux64/chrome-linux64.zip\"\nsize = {}\nsha256 = \"{}\"\n",
            zip.len(),
            sha256_hex(zip)
        )
    }

    #[test]
    fn chrome_installs_prints_the_exports_and_appends_them_to_github_env() {
        let root = tempfile::tempdir().unwrap();
        let zip = zip_of(&[("chrome-linux64/chrome", b"x", 0o755)]);
        let net = FakeNet::new();
        net.serve("https://dl.test/9.9.9/linux64/chrome-linux64.zip", zip.clone());
        let cat = root.path().join("c.toml");
        std::fs::write(&cat, chromium_toml(&zip)).unwrap();
        let gh = root.path().join("gh_env");
        let mut e = env(root.path());
        e.github_env = Some(gh.clone());
        e.test_browser_dir = Some(root.path().join("browser"));
        let c = parse(&["chrome", "--catalogue", cat.to_str().unwrap(), "--env-prefix", "DEBRIEF"]);
        let report = run(&runner(&net, 1), &e, c).unwrap();
        let exe = root.path().join("browser/chrome-linux64/chrome");
        assert_eq!(
            report.text,
            format!("CHROMIUM_TEST_PATH={}\nDEBRIEF_REQUIRE_TEST_BROWSER=1", exe.display())
        );
        assert_eq!(report.data["cached"], false);
        assert_eq!(std::fs::read_to_string(gh).unwrap(), format!("{}\n", report.text));
    }

    #[test]
    fn chrome_reads_the_default_catalogue_and_folder_and_refuses_bad_options() {
        let root = tempfile::tempdir().unwrap();
        let net = FakeNet::new();
        let r = runner(&net, 1);
        let e = env(root.path());
        let usage = |args: &[&str], e: &Env| run(&r, e, parse(args)).unwrap_err();
        assert!(matches!(usage(&["chrome", "--env-prefix", "bad"], &e), FetchError::Usage(_)));
        assert!(matches!(usage(&["chrome", "--platform", "nope"], &e), FetchError::Usage(_)));
        let mut odd = e.clone();
        odd.os = "plan9".into();
        assert!(matches!(usage(&["chrome"], &odd), FetchError::Usage(_)));
        assert!(
            usage(&["chrome"], &e).to_string().contains("chromium.toml"),
            "the default catalogue is looked for under the root"
        );
        let cat = root.path().join("bad.toml");
        std::fs::write(&cat, "nonsense").unwrap();
        let bad = usage(&["chrome", "--catalogue", cat.to_str().unwrap()], &e);
        assert!(bad.to_string().contains("bad.toml"), "{bad}");
        let mut homeless = e.clone();
        homeless.home = None;
        let zip = zip_of(&[("chrome-linux64/chrome", b"x", 0o755)]);
        std::fs::write(&cat, chromium_toml(&zip)).unwrap();
        assert!(matches!(
            usage(&["chrome", "--catalogue", cat.to_str().unwrap()], &homeless),
            FetchError::Usage(_)
        ));
        // The home-based default folder and an unwritable GITHUB_ENV.
        net.serve("https://dl.test/9.9.9/linux64/chrome-linux64.zip", zip);
        let mut gh = e.clone();
        gh.github_env = Some(root.path().join("no/such/dir/env"));
        assert!(matches!(
            usage(&["chrome", "--catalogue", cat.to_str().unwrap()], &gh),
            FetchError::Failed { .. }
        ));
        assert!(root.path().join("home/.cache/paddock-test-browser/chrome-linux64/chrome").is_file());
    }

    fn debrief_tree(root: &Path, zip: &[u8], with_include: bool) {
        let script = root.join(libduckdb::APP).join(libduckdb::PIN_SCRIPT);
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(
            &script,
            format!("VERSION=1.5.6\n    ASSET=libduckdb-linux-amd64.zip; SHA={} ;;\n", sha256_hex(zip)),
        )
        .unwrap();
        let cargo = root.join(libduckdb::APP).join(".cargo");
        std::fs::create_dir_all(&cargo).unwrap();
        let config = if with_include {
            "include = [{ path = \"libduckdb.toml\", optional = true }]\n"
        } else {
            ""
        };
        std::fs::write(cargo.join("config.toml"), config).unwrap();
    }

    #[test]
    fn libduckdb_prints_exports_and_can_write_the_cargo_config() {
        let root = tempfile::tempdir().unwrap();
        let zip = zip_of(&[("duckdb.h", b"h", 0o644)]);
        debrief_tree(root.path(), &zip, true);
        let net = FakeNet::new();
        net.serve(
            "https://github.com/duckdb/duckdb/releases/download/v1.5.6/libduckdb-linux-amd64.zip",
            zip,
        );
        let r = runner(&net, 1);
        let e = env(root.path());
        let plain = run(&r, &e, parse(&["libduckdb", "--shell", "sh"])).unwrap();
        let dir = root.path().join("apps/debrief/target/libduckdb/1.5.6/libduckdb-linux-amd64");
        assert!(
            plain.text.starts_with(&format!("export DUCKDB_LIB_DIR='{}'", dir.display())),
            "{}",
            plain.text
        );
        assert_eq!(plain.data["cached"], false);
        let with = run(&r, &e, parse(&["libduckdb", "--cargo-config", "--shell", "powershell"])).unwrap();
        assert!(
            with.text.contains("$env:DUCKDB_LIB_DIR = ") && with.text.contains("# wrote "),
            "{}",
            with.text
        );
        assert_eq!(with.data["cached"], true);
        let written = std::fs::read_to_string(root.path().join("apps/debrief/.cargo/libduckdb.toml")).unwrap();
        assert!(written.contains("relative = true"));
        let mut target = e.clone();
        target.cargo_target_dir = Some("elsewhere".into());
        let cache = root.path().join("cache");
        let custom = run(
            &r,
            &target,
            parse(&["libduckdb", "--shell", "sh", "--cache", cache.to_str().unwrap()]),
        )
        .unwrap();
        assert!(custom.text.contains("cache"), "{}", custom.text);
        assert!(matches!(
            run(&r, &e, parse(&["libduckdb", "--shell", "fish"])).unwrap_err(),
            FetchError::Usage(_)
        ));
        let mut mac = e.clone();
        mac.os = "plan9".into();
        assert!(run(&r, &mac, parse(&["libduckdb"])).is_err());
        assert!(run(&r, &env(&root.path().join("empty")), parse(&["libduckdb"])).is_err());
        debrief_tree(root.path(), &[], false);
        assert!(
            run(&r, &e, parse(&["libduckdb", "--cargo-config"])).is_err(),
            "a cargo config that does not include the local file"
        );
    }

    const CATALOGUE: &str = "format = 1\nname = \"chromium\"\nversion = \"1.0.0\"\n# keep\n[[platform]]\nkey = \"linux-x86_64\"\nurl = \"https://dl.test/1.0.0/linux64/chrome-linux64.zip\"\nsize = 1\nsha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\n";

    #[test]
    fn pin_rewrites_the_catalogue_in_place() {
        let root = tempfile::tempdir().unwrap();
        let cat = root.path().join("c.toml");
        std::fs::write(&cat, CATALOGUE).unwrap();
        let net = FakeNet::new();
        net.serve("https://dl.test/1.0.0/linux64/chrome-linux64.zip", b"new bytes".to_vec());
        net.serve("https://dl.test/2.0.0/linux64/chrome-linux64.zip", b"v2".to_vec());
        let r = runner(&net, 1);
        let e = env(root.path());
        let report = run(&r, &e, parse(&["pin", cat.to_str().unwrap()])).unwrap();
        assert!(report.text.contains("pinned 1.0.0 (1 platforms)"));
        assert!(std::fs::read_to_string(&cat).unwrap().contains(&sha256_hex(b"new bytes")));
        let moved = run(
            &r,
            &e,
            parse(&[
                "pin",
                cat.to_str().unwrap(),
                "--version",
                "2.0.0",
                "--work",
                root.path().join("w").to_str().unwrap(),
            ]),
        )
        .unwrap();
        assert_eq!(moved.data["version"], "2.0.0");
        let text = std::fs::read_to_string(&cat).unwrap();
        assert!(text.contains("# keep") && text.contains("version = \"2.0.0\""));
        assert!(!root.path().join("w").exists(), "an empty work folder is removed");
        let leftovers = std::fs::read_dir(root.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".paddock-dev-fetch-"))
            .count();
        assert_eq!(leftovers, 0, "the private work folder is removed");
        assert!(run(&r, &e, parse(&["pin", root.path().join("missing.toml").to_str().unwrap()])).is_err());
        assert!(Wrap::try_parse_from(["dev", "pin", "c.toml", "--version", "1", "--chrome-for-testing"]).is_err());
        let Command::Pin(a) = parse(&["pin", "c.toml", "--chrome-for-testing"]) else {
            unreachable!()
        };
        assert_eq!(a.chrome_for_testing.as_deref(), Some("Stable"));
        let cft = |net: &FakeNet| {
            net.serve("https://dl.test/ch.json", br#"{"channels":{"Beta":{"version":"3.0.0","downloads":{"chrome":[{"platform":"linux64","url":"https://dl.test/3.0.0/linux64/chrome-linux64.zip"}]}}}}"#.to_vec());
            net.serve("https://dl.test/3.0.0/linux64/chrome-linux64.zip", b"v3".to_vec());
        };
        cft(&net);
        let beta = run(
            &r,
            &e,
            parse(&[
                "pin",
                cat.to_str().unwrap(),
                "--chrome-for-testing",
                "Beta",
                "--endpoint",
                "https://dl.test/ch.json",
            ]),
        )
        .unwrap();
        assert_eq!(beta.data["version"], "3.0.0");
    }

    #[test]
    fn llama_and_ffmpeg_write_their_manifests_and_report_notes() {
        let root = tempfile::tempdir().unwrap();
        let net = FakeNet::new();
        let a = |n: &str, body: &[u8]| serde_json::json!({"name": n, "browser_download_url": format!("https://dl.test/{n}"), "size": body.len(), "digest": format!("sha256:{}", sha256_hex(body))});
        let rel = serde_json::json!({"assets": [a("llama-b5-bin-macos-x64.tar.gz", b"m")], "published_at": "2026-01-02T00:00:00Z"});
        net.serve(
            "https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b5",
            serde_json::to_vec(&rel).unwrap(),
        );
        net.serve("https://dl.test/llama-b5-bin-macos-x64.tar.gz", b"m".to_vec());
        let r = runner(&net, 1);
        let mut e = env(root.path());
        e.github_token = Some("t".into());
        let out = root.path().join("l.json");
        let report = run(&r, &e, parse(&["llama", "--tag", "b5", "--verify", "--out", out.to_str().unwrap()])).unwrap();
        assert!(
            report.text.contains("note: ") && report.text.contains("Verified 1 assets") && report.text.contains("llama.cpp b5"),
            "{}",
            report.text
        );
        assert_eq!(report.data["verified"], 1);
        assert!(out.is_file());
        let fa = |n: &str| serde_json::json!({"name": n, "browser_download_url": format!("https://dl.test/{n}"), "size": 1, "digest": format!("sha256:{}", sha256_hex(b"w"))});
        let frel = serde_json::json!({"assets": [fa("ffmpeg-n9.0.2-1-gabc-win64-lgpl-shared-9.0.zip")]});
        net.serve(
            "https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/tags/autobuild-2026-09-30-13-08",
            serde_json::to_vec(&frel).unwrap(),
        );
        let fout = root.path().join("f.json");
        let report = run(
            &r,
            &e,
            parse(&["ffmpeg", "--tag", "autobuild-2026-09-30-13-08", "--out", fout.to_str().unwrap()]),
        )
        .unwrap();
        assert!(
            report.text.contains("FFmpeg n9.0.2-1-gabc from autobuild-2026-09-30-13-08"),
            "{}",
            report.text
        );
        assert!(run(&r, &e, parse(&["llama", "--tag", "b404", "--out", out.to_str().unwrap()])).is_err());
        assert!(run(&r, &e, parse(&["ffmpeg", "--tag", "nope"])).is_err());
    }

    #[test]
    fn the_defaults_and_limits_of_the_options() {
        let Command::Chrome(a) = parse(&["chrome"]) else { unreachable!() };
        assert!(a.common.jobs.is_none());
        assert!(Wrap::try_parse_from(["dev", "chrome", "--jobs", "9"]).is_err());
        assert!(Wrap::try_parse_from(["dev", "chrome", "--jobs", "0"]).is_err());
        let c = parse(&["pin", "c.toml", "--jobs", "5"]);
        assert_eq!(c.common().jobs, Some(5));
        let e = Env::process(PathBuf::from("/x"));
        assert_eq!(e.root, Path::new("/x"));
        assert_eq!(e.os, std::env::consts::OS);
        assert!(find_root(Some(Path::new("/y"))).unwrap() == Path::new("/y"));
    }
}
