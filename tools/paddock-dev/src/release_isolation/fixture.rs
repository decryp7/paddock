//! A synthetic release job for tests: a clone with two lock files, a `CARGO_HOME` whose archives match them, the folders
//! `setup` makes, every action recorded, a clean environment, and one **plantable leak per probe** (ER-33: a check counts
//! only once it has been shown to see the thing). ADR 0025's build-folder check and any other release-path tool can reuse it.
//!
//! Everything lives under the folder the caller gives (use the shared test scratch): the fixture never reads or writes the
//! user's home, git configuration or npmrc (ER-14); git runs with `HOME` pointing into the fixture.

// A test helper: a failed set-up step panics with its message.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::identity::identity_of;
use super::ledger::{Kind, Mode, append};
use super::setup::{now_ns, setup};
use super::verify::{Env, Probe, Verdict, VerifyOptions, verify};
use paddock_proc::{Limits, command, run_bounded};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The Cargo workspaces of the fixture, relative to the clone.
pub const WORKSPACES: [&str; 2] = ["app/src-tauri", "tools/paddock-dev"];

/// One planted leak. Each one must make `verify` fail, naming [`Leak::probe`] and [`Leak::needle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leak {
    /// A kind the job expects was never recorded.
    LedgerMissingKind,
    /// A line carries the mode `shared`.
    LedgerSharedMode,
    /// A folder was replaced after it was recorded (same path, another inode).
    FolderReplaced,
    /// The folders existed before the run started (the ledger says the run started an hour later).
    KeptFolder,
    /// A `job` folder lies outside the job's folder.
    OutsideJobRoot,
    /// `CARGO_HOME` points at another folder than the ledger's.
    CargoHomeElsewhere,
    /// `SCCACHE_DIR` points outside the job.
    SccacheDirShared,
    /// `RUSTC_WRAPPER` is set.
    RustcWrapper,
    /// sccache served requests.
    SccacheRequests,
    /// No sccache statistics were given: the probe could not look.
    SccacheStatsMissing,
    /// A `.crate` archive with other bytes than `Cargo.lock` says.
    CrateHashMismatch,
    /// A `.crate` archive that no tagged lock file lists.
    CrateNotInLock,
    /// `CARGO_HOME` holds no archive: nothing was compared.
    NoCrates,
    /// A `.cargo/config.toml` in a parent folder of the clone.
    ConfigInParent,
    /// A `config.toml` in `CARGO_HOME`.
    ConfigInCargoHome,
    /// An untracked `.cargo/config.toml` inside the clone.
    ConfigUntrackedInClone,
    /// `.cargo/ci.toml` holds more than the target-dir.
    CiTomlExtra,
    /// `core.hooksPath` in the user's git configuration.
    GitHooksPath,
    /// `url.*.insteadOf` in the user's git configuration.
    GitInsteadOf,
    /// A user `~/.npmrc`.
    UserNpmrc,
    /// The Angular cache is a link (Unix).
    AngularLink,
    /// A global npmrc where `npm config get globalconfig` says.
    NpmGlobalNpmrc,
    /// Homebrew: `node` on PATH is a link into the Cellar and the global npmrc is under the link's prefix.
    NpmHomebrewLink,
    /// `npm` cannot be run (check broken).
    NpmMissing,
    /// `git` cannot be run (check broken).
    GitMissing,
    /// An archive in another registry's index folder, with a name and version the lock file has.
    OtherRegistryIndex,
    /// `CARGO_HOME/git` exists, but no lock file has a git source.
    GitCheckoutsInCargoHome,
    /// The `dev-tool` folder is not named `release-<run id>-<job>`.
    DevToolWrongName,
    /// `dev-tool` recorded as `off`.
    DevToolOff,
    /// `npm` recorded as `run`.
    FolderKindRun,
    /// `registry/src` is a link to a kept folder (Unix).
    LinkRegistrySrc,
    /// `<clone>/node_modules` is a link to a kept folder (Unix).
    LinkNodeModules,
    /// `<target>/release` is a link to a kept folder (Unix).
    LinkTargetRelease,
    /// `<target>/release` is a junction to a kept folder (Windows, made with `mklink /J`).
    LinkJunction,
    /// A tracked `.cargo/config.toml` edited after checkout.
    ConfigTrackedModified,
    /// The clone is not at `GITHUB_SHA`.
    CloneNotAtTag,
    /// `--allow-unpinned-tauri` in release mode in CI.
    AllowUnpinnedInCiReleaseMode,
    /// No Tauri pin file was given.
    TauriPinsMissing,
    /// `--allow-unpinned-tauri` on a tag run.
    AllowUnpinnedOnTag,
    /// A file of the Tauri tool folder with other bytes than its pin.
    TauriFileTampered,
}

impl Leak {
    /// Every leak that can be planted on this OS.
    pub fn all() -> Vec<Leak> {
        use Leak::*;
        let mut v = vec![
            LedgerMissingKind,
            LedgerSharedMode,
            FolderReplaced,
            KeptFolder,
            OutsideJobRoot,
            CargoHomeElsewhere,
            SccacheDirShared,
            RustcWrapper,
            SccacheRequests,
            SccacheStatsMissing,
            CrateHashMismatch,
            CrateNotInLock,
            NoCrates,
            ConfigInParent,
            ConfigInCargoHome,
            ConfigUntrackedInClone,
            CiTomlExtra,
            GitHooksPath,
            GitInsteadOf,
            UserNpmrc,
            TauriFileTampered,
            NpmGlobalNpmrc,
            NpmMissing,
            TauriPinsMissing,
            AllowUnpinnedOnTag,
            OtherRegistryIndex,
            GitCheckoutsInCargoHome,
            DevToolWrongName,
            DevToolOff,
            FolderKindRun,
            ConfigTrackedModified,
            CloneNotAtTag,
            AllowUnpinnedInCiReleaseMode,
        ];
        if cfg!(windows) {
            v.push(LinkJunction);
        }
        if cfg!(unix) {
            v.extend([
                AngularLink,
                LinkRegistrySrc,
                LinkNodeModules,
                LinkTargetRelease,
                GitMissing,
                NpmHomebrewLink,
            ]);
        }
        v
    }

    /// The probe that must see it.
    pub fn probe(self) -> Probe {
        use Leak::*;
        match self {
            LedgerMissingKind | LedgerSharedMode => Probe::Ledger,
            FolderReplaced | KeptFolder | OutsideJobRoot => Probe::Identity,
            DevToolWrongName | DevToolOff | FolderKindRun => Probe::Ledger,
            LinkRegistrySrc | LinkNodeModules | LinkTargetRelease | LinkJunction => Probe::Links,
            OtherRegistryIndex | GitCheckoutsInCargoHome => Probe::Crates,
            GitMissing => Probe::Git,
            CargoHomeElsewhere | SccacheDirShared => Probe::Pointers,
            RustcWrapper | SccacheRequests | SccacheStatsMissing => Probe::CompileCache,
            CrateHashMismatch | CrateNotInLock | NoCrates => Probe::Crates,
            ConfigInParent | ConfigInCargoHome | ConfigUntrackedInClone | CiTomlExtra | ConfigTrackedModified | CloneNotAtTag => {
                Probe::CargoConfig
            }
            GitHooksPath | GitInsteadOf => Probe::Git,
            UserNpmrc | NpmGlobalNpmrc | NpmHomebrewLink | NpmMissing => Probe::Npm,
            AngularLink => Probe::Angular,
            TauriFileTampered | TauriPinsMissing | AllowUnpinnedOnTag | AllowUnpinnedInCiReleaseMode => Probe::TauriTools,
        }
    }

    /// Text that the finding must contain, so that the finding names the planted thing and not another one.
    pub fn needle(self) -> &'static str {
        use Leak::*;
        match self {
            LedgerMissingKind => "sccache",
            LedgerSharedMode => "shared",
            FolderReplaced => "`npm`",
            KeptFolder => "born before this job started",
            OutsideJobRoot => "outside the job folder",
            CargoHomeElsewhere => "CARGO_HOME is",
            SccacheDirShared => "SCCACHE_DIR",
            RustcWrapper => "RUSTC_WRAPPER",
            SccacheRequests => "served 7",
            SccacheStatsMissing => "no statistics",
            CrateHashMismatch => "serde-1.0.0",
            CrateNotInLock => "evil-9.9.9",
            NoCrates => "0 .crate archives",
            ConfigInParent => "not a file of the tagged tree",
            ConfigInCargoHome => "config.toml",
            ConfigUntrackedInClone => "not a file of the tagged tree",
            CiTomlExtra => "more than",
            GitHooksPath => "core.hooksPath",
            GitInsteadOf => "insteadOf",
            UserNpmrc => ".npmrc",
            AngularLink => "build would read a kept Angular cache",
            TauriFileTampered => "makensis",
            NpmGlobalNpmrc => "globalconfig",
            NpmHomebrewLink => "node's prefix",
            NpmMissing => "`npm config get",
            GitMissing => "the probe did not show",
            OtherRegistryIndex => "match no registry source",
            GitCheckoutsInCargoHome => "no tagged Cargo.lock has a git source",
            DevToolWrongName => "is not named",
            DevToolOff => "only run is allowed",
            FolderKindRun => "only job is allowed",
            LinkRegistrySrc | LinkNodeModules | LinkTargetRelease | LinkJunction => "lead outside the job",
            ConfigTrackedModified => "differs from the commit",
            CloneNotAtTag => "is not GITHUB_SHA",
            AllowUnpinnedInCiReleaseMode => "refused on a tag run",
            TauriPinsMissing => "not measured (check broken)",
            AllowUnpinnedOnTag => "refused on a tag run",
        }
    }
}

/// A synthetic job.
pub struct Fixture {
    /// The folder everything is under.
    pub root: PathBuf,
    /// The stand-in for the user's home.
    pub home: PathBuf,
    /// The job's folder (`$RUNNER_TEMP`).
    pub temp: PathBuf,
    /// The ledger file.
    pub ledger: PathBuf,
    /// The clone (a git repository with one commit).
    pub clone: PathBuf,
    /// `CARGO_HOME`.
    pub cargo_home: PathBuf,
    /// The build folder.
    pub target: PathBuf,
    /// The npm cache.
    pub npm: PathBuf,
    /// The Tauri tool folder.
    pub tauri_tools: PathBuf,
    /// The fake node install (`<prefix>/bin/node`) that is first on PATH.
    pub node_prefix: PathBuf,
    /// The job's environment.
    pub env: Env,
    /// What `verify` is asked.
    pub opts: VerifyOptions,
}

const NODE_NAME: &str = if cfg!(windows) { "node.exe" } else { "node" };

/// The bytes of a fake archive.
fn crate_bytes(name: &str) -> Vec<u8> {
    format!("fake .crate archive of {name}").into_bytes()
}

fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn run(env: &Env, dir: &Path, program: &str, args: &[&str]) {
    let mut cmd = command(program);
    cmd.args(args).current_dir(dir).env_clear();
    for (k, v) in &env.0 {
        cmd.env(k, v);
    }
    let out = run_bounded(cmd, Limits::new(Duration::from_secs(60))).expect("the program starts");
    assert!(out.success(), "{program} {args:?}: {}", out.stderr_text());
}

/// A stand-in for `npm` that answers `npm config get userconfig|globalconfig` the way npm would for this fake install.
fn write_fake_npm(bin: &Path, prefix: &Path, home: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = format!(
            "#!/bin/sh\ncase \"$3\" in\n  globalconfig) echo '{}/etc/npmrc';;\n  userconfig) echo '{}/.npmrc';;\nesac\n",
            prefix.display(),
            home.display()
        );
        let path = bin.join("npm");
        fs::write(&path, script).expect("fake npm");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    #[cfg(windows)]
    {
        let script = format!(
            "@echo off\r\nif \"%3\"==\"globalconfig\" echo {}\\bin\\etc\\npmrc\r\nif \"%3\"==\"userconfig\" echo {}\\.npmrc\r\n",
            prefix.display(),
            home.display()
        );
        fs::write(bin.join("npm.cmd"), script).expect("fake npm");
    }
}

/// `PATH` without the folders that hold `name`.
fn path_without(env: &mut Env, names: &[&str]) {
    let kept: Vec<_> = std::env::split_paths(&env.0["PATH"])
        .filter(|d| !names.iter().any(|n| d.join(n).exists()))
        .collect();
    env.0.insert(
        "PATH".into(),
        std::env::join_paths(kept).expect("PATH").to_string_lossy().into_owned(),
    );
}

impl Fixture {
    /// Builds a clean job under `root` (an existing, empty folder).
    pub fn build(root: &Path) -> Fixture {
        // Plain, not `\\?\C:\...`: the fixture hands these paths to git (HOME) and writes them into files, and git on Windows cannot open
        // a config file through a verbatim path ("unknown error occurred while reading the configuration files").
        assert!(root.exists(), "root exists");
        let root = paddock_repo_graph::real_path(root);
        let home = root.join("home");
        let temp = root.join("job");
        let dev = root.join("tools-home").join("release-1234-bundle");
        for d in [&home, &temp, &dev] {
            fs::create_dir_all(d).expect("fixture folder");
        }
        // `./dev` is built into `dev` before `setup`, which runs through it: born after the job started, before `setup`.
        fs::write(dev.join(".rustc_info.json"), "{}").expect("a built tool");
        let made = setup(&temp, &temp.join("release-isolation").join("ledger.tsv"), Some(&dev)).expect("setup");
        let folder = |k: Kind| made.folders.iter().find(|(kk, _)| *kk == k).map(|(_, p)| p.clone()).expect("made");
        let mut env = Env::default();
        for key in ["PATH", "SYSTEMROOT", "TEMP", "TMP", "LANG"] {
            if let Ok(v) = std::env::var(key) {
                env.0.insert(key.to_string(), v);
            }
        }
        let node_prefix = root.join("node-prefix");
        let node_bin = node_prefix.join("bin");
        fs::create_dir_all(&node_bin).expect("node bin");
        fs::write(node_bin.join(NODE_NAME), b"not a real node").expect("fake node");
        write_fake_npm(&node_bin, &node_prefix, &home);
        let path =
            std::env::join_paths(std::iter::once(node_bin).chain(std::env::split_paths(&env.0.get("PATH").cloned().unwrap_or_default())))
                .expect("PATH");
        env.0.insert("PATH".into(), path.to_string_lossy().into_owned());
        env.0.insert("HOME".into(), home.display().to_string());
        env.0.insert("USERPROFILE".into(), home.display().to_string());
        env.0.insert("XDG_CONFIG_HOME".into(), home.join(".config").display().to_string());
        env.0.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
        env.0.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
        env.0.insert("RUNNER_TEMP".into(), temp.display().to_string());
        env.0.insert("GITHUB_RUN_ID".into(), "1234".into());
        for (k, v) in &made.env {
            env.0.insert(k.clone(), v.clone());
        }
        let clone = folder(Kind::Workspace);
        let cargo_home = folder(Kind::CargoHome);
        let tauri_tools = folder(Kind::TauriTools);
        // The tagged tree: two lock files whose checksums are those of the fake archives.
        for (ws, name) in [(WORKSPACES[0], "serde-1.0.0"), (WORKSPACES[1], "regex-1.2.3")] {
            fs::create_dir_all(clone.join(ws)).expect("workspace folder");
            let (n, v) = name.rsplit_once('-').expect("name-version");
            fs::write(
                clone.join(ws).join("Cargo.lock"),
                format!(
                    "version = 4\n\n[[package]]\nname = \"{n}\"\nversion = \"{v}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{}\"\n\n[[package]]\nname = \"local\"\nversion = \"0.1.0\"\n",
                    sha(&crate_bytes(name))
                ),
            )
            .expect("lock file");
        }
        run(&env, &clone, "git", &["init", "-q", "-b", "main"]);
        run(&env, &clone, "git", &["add", "-A"]);
        run(
            &env,
            &clone,
            "git",
            &[
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                "tag",
            ],
        );
        let head = {
            let mut cmd = command("git");
            cmd.args(["rev-parse", "HEAD"]).current_dir(&clone).env_clear();
            for (k, v) in &env.0 {
                cmd.env(k, v);
            }
            run_bounded(cmd, Limits::new(Duration::from_secs(60)))
                .expect("git rev-parse")
                .stdout_text()
                .trim()
                .to_string()
        };
        env.0.insert("GITHUB_SHA".into(), head);
        // The archives and an extracted source.
        let cache = cargo_home.join("registry").join("cache").join("index.crates.io-0000000000000000");
        let src = cargo_home
            .join("registry")
            .join("src")
            .join("index.crates.io-0000000000000000")
            .join("serde-1.0.0");
        fs::create_dir_all(&cache).expect("cache");
        fs::create_dir_all(&src).expect("src");
        for name in ["serde-1.0.0", "regex-1.2.3"] {
            fs::write(cache.join(format!("{name}.crate")), crate_bytes(name)).expect("archive");
        }
        fs::write(src.join("lib.rs"), "pub fn f() {}\n").expect("source");
        // Tauri tools with their pins.
        fs::create_dir_all(tauri_tools.join("NSIS")).expect("tauri");
        fs::write(tauri_tools.join("NSIS").join("makensis.exe"), b"nsis bytes").expect("tool");
        let pins = root.join("tauri-pins.txt");
        fs::write(&pins, format!("{}  NSIS/makensis.exe\n", sha(b"nsis bytes"))).expect("pins");
        // The actions that record their own folder or cache.
        let angular = clone.join("app").join(".angular").join("cache");
        fs::create_dir_all(&angular).expect("angular");
        let ledger = temp.join("release-isolation").join("ledger.tsv");
        append(&ledger, Kind::Sccache, Mode::Off, None).expect("record sccache");
        append(&ledger, Kind::Angular, Mode::Job, Some(&angular)).expect("record angular");
        let stats = root.join("sccache-stats.json");
        fs::write(&stats, r#"{"stats":{"compile_requests":0}}"#).expect("stats");
        let mut opts = VerifyOptions::new(ledger.clone());
        opts.cargo_workspaces = WORKSPACES.iter().map(ToString::to_string).collect();
        opts.app_dirs = vec!["app".into()];
        opts.sccache_stats = Some(stats);
        opts.tauri_pins = Some(pins);
        opts.walk_stop = Some(root.clone());
        Fixture {
            target: folder(Kind::Target),
            npm: folder(Kind::Npm),
            root,
            home,
            temp,
            ledger,
            clone,
            cargo_home,
            tauri_tools,
            node_prefix,
            env,
            opts,
        }
    }

    /// The folder `setup` made for `node_modules`.
    pub fn node_modules(&self) -> PathBuf {
        PathBuf::from(
            super::ledger::Ledger::read(&self.ledger)
                .expect("ledger")
                .get(Kind::NodeModules)
                .expect("node-modules")
                .path
                .clone(),
        )
    }

    /// Runs `verify` on this job.
    pub fn verify(&self) -> Verdict {
        verify(&self.opts, &self.env)
    }

    /// Gives every archive and extracted source a modification time of 2006, as cargo's unpacked sources and copied files have.
    /// Freshness is by identity, so this must not matter.
    pub fn age_registry(&self) {
        let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_150_000_000);
        let mut stack = vec![self.cargo_home.join("registry")];
        while let Some(dir) = stack.pop() {
            for e in fs::read_dir(&dir).expect("read_dir").flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    fs::OpenOptions::new()
                        .write(true)
                        .open(&p)
                        .expect("open")
                        .set_modified(old)
                        .expect("set_modified");
                }
            }
        }
    }

    fn rewrite_ledger(&self, f: impl FnOnce(String) -> String) {
        let text = fs::read_to_string(&self.ledger).expect("ledger");
        fs::write(&self.ledger, f(text)).expect("rewrite ledger");
    }

    fn git(&self, args: &[&str]) {
        run(&self.env, &self.clone, "git", args);
    }

    fn git_head(&self) -> String {
        let mut cmd = command("git");
        cmd.args(["rev-parse", "HEAD"]).current_dir(&self.clone).env_clear();
        for (k, v) in &self.env.0 {
            cmd.env(k, v);
        }
        run_bounded(cmd, Limits::new(Duration::from_secs(60)))
            .expect("git rev-parse")
            .stdout_text()
            .trim()
            .to_string()
    }

    fn archive_dir(&self) -> PathBuf {
        self.cargo_home
            .join("registry")
            .join("cache")
            .join("index.crates.io-0000000000000000")
    }

    /// Plants a leak.
    pub fn plant(&mut self, leak: Leak) {
        match leak {
            Leak::LedgerMissingKind => {
                self.rewrite_ledger(|t| t.lines().filter(|l| !l.starts_with("sccache\t")).collect::<Vec<_>>().join("\n") + "\n")
            }
            Leak::LedgerSharedMode => self.rewrite_ledger(|t| {
                t.lines()
                    .map(|l| {
                        if l.starts_with("angular\t") {
                            l.replacen("\tjob\t", "\tshared\t", 1)
                        } else {
                            l.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            }),
            Leak::FolderReplaced => {
                let aside = self.npm.with_extension("old");
                fs::rename(&self.npm, &aside).expect("move aside");
                fs::create_dir(&self.npm).expect("new folder at the same path");
            }
            Leak::KeptFolder => {
                let later = now_ns() + 3_600_000_000_000;
                self.rewrite_ledger(|t| {
                    let mut lines: Vec<String> = t.lines().map(str::to_string).collect();
                    let head: Vec<&str> = lines[0].split('\t').collect();
                    lines[0] = format!("#job\t{}\t{later}", head[1]);
                    lines.join("\n") + "\n"
                });
            }
            Leak::OutsideJobRoot => {
                let outside = self.root.join("kept-node-modules");
                fs::create_dir_all(&outside).expect("outside folder");
                let id = identity_of(&outside).expect("identity");
                let path = outside.display().to_string();
                self.rewrite_ledger(|t| {
                    t.lines()
                        .map(|l| {
                            if l.starts_with("node-modules\t") {
                                format!("node-modules\t{path}\tjob\t{}\t{}\t{}", id.dev, id.ino, id.birth_ns)
                            } else {
                                l.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                        + "\n"
                });
            }
            Leak::CargoHomeElsewhere => {
                let other = self.root.join("user-cargo-home");
                fs::create_dir_all(&other).expect("other cargo home");
                self.env.0.insert("CARGO_HOME".into(), other.display().to_string());
            }
            Leak::SccacheDirShared => {
                let shared = self.root.join("sccache-shared");
                fs::create_dir_all(&shared).expect("shared sccache");
                self.env.0.insert("SCCACHE_DIR".into(), shared.display().to_string());
            }
            Leak::RustcWrapper => {
                self.env.0.insert("RUSTC_WRAPPER".into(), "sccache".into());
            }
            Leak::SccacheRequests => {
                let stats = self.opts.sccache_stats.clone().expect("stats");
                fs::write(stats, r#"{"stats":{"compile_requests":7}}"#).expect("stats");
            }
            Leak::SccacheStatsMissing => self.opts.sccache_stats = None,
            Leak::CrateHashMismatch => fs::write(self.archive_dir().join("serde-1.0.0.crate"), b"poisoned").expect("poison"),
            Leak::CrateNotInLock => fs::write(self.archive_dir().join("evil-9.9.9.crate"), b"planted").expect("plant"),
            Leak::NoCrates => fs::remove_dir_all(self.cargo_home.join("registry").join("cache")).expect("remove archives"),
            Leak::ConfigInParent => {
                fs::create_dir_all(self.temp.join(".cargo")).expect(".cargo");
                fs::write(
                    self.temp.join(".cargo").join("config.toml"),
                    "[build]\nrustc-wrapper = \"/tmp/w\"\n",
                )
                .expect("config");
            }
            Leak::ConfigInCargoHome => fs::write(self.cargo_home.join("config.toml"), "[net]\noffline = true\n").expect("config"),
            Leak::ConfigUntrackedInClone => {
                fs::create_dir_all(self.clone.join(".cargo")).expect(".cargo");
                fs::write(self.clone.join(".cargo").join("config.toml"), "[build]\nrustflags = []\n").expect("config");
            }
            Leak::CiTomlExtra => {
                fs::create_dir_all(self.clone.join(".cargo")).expect(".cargo");
                fs::write(
                    self.clone.join(".cargo").join("ci.toml"),
                    format!("[build]\ntarget-dir = '{}'\nrustc-wrapper = \"x\"\n", self.target.display()),
                )
                .expect("ci.toml");
            }
            Leak::GitHooksPath => fs::write(self.home.join(".gitconfig"), "[core]\n\thooksPath = /evil/hooks\n").expect("gitconfig"),
            Leak::GitInsteadOf => fs::write(
                self.home.join(".gitconfig"),
                "[url \"https://evil.invalid/\"]\n\tinsteadOf = https://github.com/\n",
            )
            .expect("gitconfig"),
            Leak::UserNpmrc => fs::write(self.home.join(".npmrc"), "ignore-scripts=false\n").expect("npmrc"),
            Leak::AngularLink => {
                #[cfg(unix)]
                {
                    let cache = self.clone.join("app").join(".angular").join("cache");
                    let kept = self.root.join("kept-angular");
                    fs::create_dir_all(&kept).expect("kept angular cache");
                    fs::remove_dir(&cache).expect("remove the real cache");
                    std::os::unix::fs::symlink(&kept, &cache).expect("link");
                }
            }
            Leak::NpmGlobalNpmrc => {
                // On Windows the fake npm names the node folder's own `etc\npmrc`.
                let etc = if cfg!(windows) {
                    self.node_prefix.join("bin").join("etc")
                } else {
                    self.node_prefix.join("etc")
                };
                fs::create_dir_all(&etc).expect("etc");
                fs::write(etc.join("npmrc"), "ignore-scripts=false\n").expect("npmrc");
            }
            Leak::NpmHomebrewLink => {
                #[cfg(unix)]
                {
                    let brew = self.root.join("brew");
                    let cellar_bin = brew.join("Cellar").join("node").join("26").join("bin");
                    fs::create_dir_all(&cellar_bin).expect("cellar");
                    fs::write(cellar_bin.join("node"), "x").expect("node");
                    fs::create_dir_all(brew.join("bin")).expect("bin");
                    std::os::unix::fs::symlink(cellar_bin.join("node"), brew.join("bin").join("node")).expect("link");
                    fs::create_dir_all(brew.join("etc")).expect("etc");
                    fs::write(brew.join("etc").join("npmrc"), "registry=https://evil.invalid/\n").expect("npmrc");
                    // The real node is first on PATH; the fixture's fake one goes.
                    fs::remove_file(self.node_prefix.join("bin").join(NODE_NAME)).expect("remove fake node");
                    let path = std::env::join_paths(std::iter::once(brew.join("bin")).chain(std::env::split_paths(&self.env.0["PATH"])))
                        .expect("PATH");
                    self.env.0.insert("PATH".into(), path.to_string_lossy().into_owned());
                }
            }
            Leak::NpmMissing => {
                fs::remove_file(self.node_prefix.join("bin").join(if cfg!(windows) { "npm.cmd" } else { "npm" })).expect("remove npm");
                path_without(&mut self.env, &["npm", "npm.cmd"]);
            }
            Leak::GitMissing => {
                let empty = self.root.join("empty-path");
                fs::create_dir_all(&empty).expect("empty");
                self.env.0.insert("PATH".into(), empty.display().to_string());
            }
            Leak::OtherRegistryIndex => {
                let other = self.cargo_home.join("registry").join("cache").join("evil.example-1111111111111111");
                fs::create_dir_all(&other).expect("other index");
                fs::copy(self.archive_dir().join("serde-1.0.0.crate"), other.join("serde-1.0.0.crate")).expect("copy");
            }
            Leak::GitCheckoutsInCargoHome => fs::create_dir_all(self.cargo_home.join("git").join("checkouts")).expect("git"),
            Leak::DevToolWrongName => {
                let from = self.root.join("tools-home").join("release-1234-bundle");
                let to = self.root.join("tools-home").join("cached-bundle");
                fs::rename(&from, &to).expect("rename");
                let (from, to) = (from.display().to_string(), to.display().to_string());
                self.rewrite_ledger(|t| t.replace(&from, &to));
            }
            Leak::DevToolOff => self.rewrite_ledger(|t| {
                t.lines()
                    .map(|l| {
                        if l.starts_with("dev-tool\t") {
                            "dev-tool\t-\toff\t\t\t".to_string()
                        } else {
                            l.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            }),
            Leak::FolderKindRun => self.rewrite_ledger(|t| {
                t.lines()
                    .map(|l| {
                        if l.starts_with("npm\t") {
                            l.replacen("\tjob\t", "\trun\t", 1)
                        } else {
                            l.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            }),
            Leak::LinkRegistrySrc | Leak::LinkNodeModules | Leak::LinkTargetRelease => {
                #[cfg(unix)]
                {
                    let kept = self.root.join("kept");
                    fs::create_dir_all(&kept).expect("kept folder");
                    let link = match leak {
                        Leak::LinkRegistrySrc => {
                            fs::remove_dir_all(self.cargo_home.join("registry").join("src")).expect("remove src");
                            self.cargo_home.join("registry").join("src")
                        }
                        Leak::LinkNodeModules => self.clone.join("node_modules"),
                        _ => self.target.join("release"),
                    };
                    std::os::unix::fs::symlink(&kept, link).expect("link");
                }
            }
            Leak::LinkJunction => {
                #[cfg(windows)]
                {
                    let kept = self.root.join("kept");
                    fs::create_dir_all(&kept).expect("kept folder");
                    let link = self.target.join("release");
                    let mut cmd = command("cmd");
                    cmd.args(["/c", "mklink", "/J"]).arg(&link).arg(&kept);
                    let out = run_bounded(cmd, Limits::new(Duration::from_secs(30))).expect("cmd starts");
                    assert!(out.success(), "mklink /J: {}{}", out.stdout_text(), out.stderr_text());
                }
            }
            Leak::ConfigTrackedModified => {
                let cfg_dir = self.clone.join(".cargo");
                fs::create_dir_all(&cfg_dir).expect(".cargo");
                fs::write(cfg_dir.join("config.toml"), "[net]\nretry = 2\n").expect("config");
                self.git(&["add", "-A"]);
                self.git(&[
                    "-c",
                    "user.name=x",
                    "-c",
                    "user.email=x@x.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-qm",
                    "c",
                ]);
                self.env.0.insert("GITHUB_SHA".into(), self.git_head());
                fs::write(cfg_dir.join("config.toml"), "[build]\nrustc-wrapper = \"/kept/sccache\"\n").expect("edit");
            }
            Leak::CloneNotAtTag => {
                self.env.0.insert("GITHUB_SHA".into(), "0".repeat(40));
            }
            Leak::AllowUnpinnedInCiReleaseMode => {
                self.opts.allow_unpinned_tauri = true;
                self.opts.tauri_pins = None;
                self.env.0.insert("GITHUB_ACTIONS".into(), "true".into());
                self.env.0.insert("PADDOCK_RELEASE_MODE".into(), "1".into());
            }
            Leak::TauriPinsMissing => self.opts.tauri_pins = None,
            Leak::AllowUnpinnedOnTag => {
                self.opts.allow_unpinned_tauri = true;
                self.env.0.insert("GITHUB_REF".into(), "refs/tags/dyno-v9.9.9".into());
            }
            Leak::TauriFileTampered => {
                fs::write(self.tauri_tools.join("NSIS").join("makensis.exe"), b"planted installer tool").expect("tamper")
            }
        }
    }
}
