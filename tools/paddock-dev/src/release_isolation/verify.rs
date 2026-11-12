//! `verify`: the check of ADR 0085 §5. It reads the ledger and the machine and prints one finding per fault, each with the
//! next step. A probe that could not look (a missing kind, no archive seen, a stats file that is absent) is a finding
//! that starts with `check broken`, never a pass (rule 71 / ER-33).

use super::config;
use super::crates;
use super::identity::identity_of;
use super::ledger::{Kind, Ledger, Mode};
use paddock_env::winpath;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The environment a job runs in, captured once so that a test can give its own.
#[derive(Debug, Clone, Default)]
pub struct Env(pub BTreeMap<String, String>);

impl Env {
    /// This process's environment.
    pub fn from_process() -> Env {
        Env(super::pathfind::normalise(std::env::vars().collect(), cfg!(windows), &|k| {
            std::env::var(k).ok()
        }))
    }

    /// A variable that is set and not empty. On Windows the name is matched ignoring case (`Path` is `PATH`).
    pub fn get(&self, key: &str) -> Option<&str> {
        super::pathfind::lookup(&self.0, key, cfg!(windows))
    }

    /// Sets a variable under the spelling it already has (on Windows, ignoring case), so one variable never has two keys.
    pub fn set(&mut self, key: &str, value: String) {
        let existing = cfg!(windows)
            .then(|| self.0.keys().find(|k| k.eq_ignore_ascii_case(key)).cloned())
            .flatten();
        self.0.insert(existing.unwrap_or_else(|| key.to_string()), value);
    }

    /// The user's home folder: `HOME`, else `USERPROFILE` (Windows).
    pub fn home(&self) -> Option<PathBuf> {
        self.get("HOME").or_else(|| self.get("USERPROFILE")).map(PathBuf::from)
    }
}

/// One group of checks. A test turns one off to show that its planted leak is caught by that probe and no other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// The ledger is complete and well formed.
    Ledger,
    /// Every folder is the one `setup` or `record` saw: same device and inode, change time not older than the run.
    Identity,
    /// The job's own pointers (`CARGO_HOME`, the npm cache, `SCCACHE_DIR`) lead to the ledger's folders.
    Pointers,
    /// No compile cache: no wrapper, no remote backend, 0 sccache requests.
    CompileCache,
    /// Every `.crate` archive matches the tagged `Cargo.lock` files.
    Crates,
    /// Every cargo configuration cargo would read is a file of the tagged tree.
    CargoConfig,
    /// No `core.hooksPath` and no `url.*.insteadOf` in any git configuration scope.
    Git,
    /// No npm configuration outside the tagged tree.
    Npm,
    /// The Angular cache is a real folder inside the checkout.
    Angular,
    /// No link inside a job folder leads outside the job.
    Links,
    /// Every file of the Tauri tool folder has its pinned SHA-256.
    TauriTools,
}

impl Probe {
    /// Every probe.
    pub const ALL: [Probe; 11] = [
        Probe::Ledger,
        Probe::Identity,
        Probe::Pointers,
        Probe::CompileCache,
        Probe::Crates,
        Probe::CargoConfig,
        Probe::Git,
        Probe::Npm,
        Probe::Angular,
        Probe::Links,
        Probe::TauriTools,
    ];

    /// The name printed in a finding.
    pub fn id(self) -> &'static str {
        match self {
            Probe::Ledger => "ledger",
            Probe::Identity => "identity",
            Probe::Pointers => "pointers",
            Probe::CompileCache => "compile-cache",
            Probe::Crates => "crates",
            Probe::CargoConfig => "cargo-config",
            Probe::Git => "git",
            Probe::Npm => "npm",
            Probe::Angular => "angular",
            Probe::Links => "links",
            Probe::TauriTools => "tauri-tools",
        }
    }
}

/// One fault, with the next step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The probe that saw it.
    pub probe: Probe,
    /// What is wrong, naming the path or variable.
    pub what: String,
    /// What to do next.
    pub next: String,
}

impl Finding {
    /// The `::error::` line.
    pub fn line(&self) -> String {
        format!("::error::release-isolation {}: {} Next: {}", self.probe.id(), self.what, self.next)
    }

    /// The probe could not look (as opposed to looking and finding a leak).
    pub fn is_broken(&self) -> bool {
        self.what.starts_with("check broken")
    }
}

/// What `verify` is asked.
#[derive(Debug, Clone)]
pub struct VerifyOptions {
    /// The ledger file.
    pub ledger: PathBuf,
    /// The kinds the job must have recorded.
    pub expect: Vec<Kind>,
    /// The Cargo workspaces the job builds, relative to the clone (their `Cargo.lock` files form the union).
    pub cargo_workspaces: Vec<String>,
    /// The Angular apps, relative to the clone (each `.angular/cache` is checked).
    pub app_dirs: Vec<String>,
    /// The JSON of `sccache --show-stats --stats-format=json`, taken after the build.
    pub sccache_stats: Option<PathBuf>,
    /// The pinned SHA-256 values of the Tauri tool folder (`<sha256>  <relative path>` per line).
    pub tauri_pins: Option<PathBuf>,
    /// Accept a missing pin file (a local trial only; refused when `GITHUB_REF` is a tag).
    pub allow_unpinned_tauri: bool,
    /// How far before the job's start a folder's birth may be before it counts as older (file systems stamp with a coarse
    /// clock that can lag).
    pub slack_ns: i128,
    /// Tests only: where the walk up from a Cargo workspace for `.cargo/config*` stops (inclusive), so that a test does not
    /// depend on the machine's folders above its scratch root. `None` walks to the filesystem root.
    pub walk_stop: Option<PathBuf>,
    /// Probes turned off (tests only).
    pub skip: Vec<Probe>,
}

impl VerifyOptions {
    /// The defaults for a ledger: the default expected kinds and a 2 s slack.
    pub fn new(ledger: PathBuf) -> VerifyOptions {
        VerifyOptions {
            ledger,
            expect: Kind::DEFAULT_EXPECTED.to_vec(),
            cargo_workspaces: Vec::new(),
            app_dirs: Vec::new(),
            sccache_stats: None,
            tauri_pins: None,
            allow_unpinned_tauri: false,
            walk_stop: None,
            slack_ns: 1_000_000_000,
            skip: Vec::new(),
        }
    }
}

/// What `verify` found, and what it counted when it found nothing.
#[derive(Debug, Clone, Default)]
pub struct Verdict {
    /// Every fault.
    pub findings: Vec<Finding>,
    /// Things that were not measured and are not faults (printed as notices).
    pub notices: Vec<String>,
    /// Folders made by this run and confirmed.
    pub folders_confirmed: usize,
    /// `.crate` archives that matched a lock file.
    pub crates_matched: usize,
    /// Files of the Tauri tool folder that matched a pin.
    pub tauri_files_matched: usize,
}

impl Verdict {
    /// Nothing was found and nothing was broken.
    pub fn passed(&self) -> bool {
        self.findings.is_empty()
    }

    /// The one-line summary of a pass (ADR 0085 §5 item 6).
    pub fn summary(&self) -> String {
        format!(
            "release isolation: {} kinds made by this run, 0 shared; {} crate archives match Cargo.lock; no compile cache; no cargo or npm configuration outside the tag",
            self.folders_confirmed, self.crates_matched
        )
    }
}

/// Shared state of the probes.
pub(crate) struct Ctx<'a> {
    pub opts: &'a VerifyOptions,
    pub env: &'a Env,
    pub ledger: &'a Ledger,
    pub verdict: Verdict,
}

impl Ctx<'_> {
    pub fn on(&self, p: Probe) -> bool {
        !self.opts.skip.contains(&p)
    }

    pub fn add(&mut self, probe: Probe, what: impl Into<String>, next: impl Into<String>) {
        self.verdict.findings.push(Finding {
            probe,
            what: what.into(),
            next: next.into(),
        });
    }

    /// The canonical path of a kind's folder, when the ledger has one.
    pub fn folder(&self, kind: Kind) -> Option<PathBuf> {
        let e = self.ledger.get(kind)?;
        if e.mode == Mode::Off.name() {
            return None;
        }
        winpath::real(Path::new(&e.path)).ok()
    }
}

/// Runs every probe.
pub fn verify(opts: &VerifyOptions, env: &Env) -> Verdict {
    let ledger = match Ledger::read(&opts.ledger) {
        Ok(l) => l,
        Err(e) => {
            let mut v = Verdict::default();
            v.findings.push(Finding {
                probe: Probe::Ledger,
                what: format!("check broken: {e}"),
                next: "Run `./dev ci release-isolation setup` as the first step of the job, before any step that records.".into(),
            });
            return v;
        }
    };
    let mut ctx = Ctx {
        opts,
        env,
        ledger: &ledger,
        verdict: Verdict::default(),
    };
    if ctx.on(Probe::Ledger) {
        ledger_probe(&mut ctx);
    }
    if ctx.on(Probe::Identity) {
        identity_probe(&mut ctx);
    }
    if ctx.on(Probe::Pointers) {
        pointers_probe(&mut ctx);
    }
    if ctx.on(Probe::CompileCache) {
        config::compile_cache(&mut ctx);
    }
    if ctx.on(Probe::Crates) {
        crates::crates_probe(&mut ctx);
    }
    if ctx.on(Probe::CargoConfig) {
        config::cargo_config(&mut ctx);
    }
    if ctx.on(Probe::Git) {
        config::git_probe(&mut ctx);
    }
    if ctx.on(Probe::Npm) {
        config::npm(&mut ctx);
    }
    if ctx.on(Probe::Angular) {
        config::angular(&mut ctx);
    }
    if ctx.on(Probe::Links) {
        super::links::links_probe(&mut ctx);
    }
    if ctx.on(Probe::TauriTools) {
        crates::tauri_probe(&mut ctx);
    }
    ctx.verdict
}

fn ledger_probe(ctx: &mut Ctx) {
    let setup_next = "Every action that picks a folder or a cache must call `./dev ci release-isolation record` (ADR 0085 §5 item 1).";
    if ctx.ledger.entries.is_empty() {
        ctx.add(
            Probe::Ledger,
            "check broken: the ledger has 0 lines.",
            "Run `setup` first, and `record` from each action.",
        );
    }
    for kind in ctx.opts.expect.clone() {
        if ctx.ledger.get(kind).is_none() {
            ctx.add(
                Probe::Ledger,
                format!("check broken: the ledger has no line for the expected kind `{kind}`."),
                setup_next,
            );
        }
    }
    let mut seen: Vec<&str> = Vec::new();
    for e in &ctx.ledger.entries.clone() {
        if Kind::parse(&e.kind).is_none() {
            ctx.add(
                Probe::Ledger,
                format!("line {}: `{}` is not a kind this tool knows.", e.line, e.kind),
                setup_next,
            );
        }
        if seen.contains(&e.kind.as_str()) {
            ctx.add(
                Probe::Ledger,
                format!("line {}: a second line for `{}`.", e.line, e.kind),
                "One line per kind: remove the second `record`.",
            );
        }
        seen.push(&e.kind);
        match Mode::parse(&e.mode) {
            None => ctx.add(
                Probe::Ledger,
                format!("line {}: `{}` has the mode `{}`, not job, run or off.", e.line, e.kind, e.mode),
                "The action chose a shared or kept folder: make it use a folder this run made (release mode), then `record` it as `job`.",
            ),
            Some(Mode::Off) => {}
            Some(_) if e.id.is_none() => ctx.add(Probe::Ledger, format!("line {}: `{}` has no identity.", e.line, e.kind), setup_next),
            Some(_) => {}
        }
        if let (Some(kind), Some(mode)) = (Kind::parse(&e.kind), Mode::parse(&e.mode)) {
            if !kind.allowed_modes().contains(&mode) {
                let names: Vec<&str> = kind.allowed_modes().iter().map(|m| m.name()).collect();
                ctx.add(
                    Probe::Ledger,
                    format!("line {}: `{kind}` is recorded as `{}`; only {} is allowed for it.", e.line, e.mode, names.join(" or ")),
                    "Only `dev-tool` may be `run`, only `sccache` (and an unexpected `libduckdb`) may be `off`; every other kind is a folder this run made inside the job folder.",
                );
            } else if kind == Kind::Libduckdb && mode == Mode::Off && ctx.opts.expect.contains(&kind) {
                ctx.add(
                    Probe::Ledger,
                    format!("line {}: `libduckdb` is `off` but this job expects it.", e.line),
                    "Debrief's libduckdb must be downloaded and checked in every run.",
                );
            }
            if kind == Kind::DevTool && mode == Mode::Run {
                dev_tool_name(ctx, e);
            }
        }
    }
}

/// The dev-tool folder is named `release-<GITHUB_RUN_ID>-<job>` (ADR 0085 §4 item 3).
fn dev_tool_name(ctx: &mut Ctx, e: &super::ledger::Entry) {
    let Some(run_id) = ctx.env.get("GITHUB_RUN_ID") else {
        ctx.add(
            Probe::Ledger,
            "check broken: GITHUB_RUN_ID is not set, so the name of the dev-tool folder cannot be checked.",
            "Run verify inside the CI job.",
        );
        return;
    };
    let name = Path::new(&e.path).file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let prefix = format!("release-{run_id}-");
    if !name.starts_with(&prefix) || name.len() == prefix.len() {
        ctx.add(
            Probe::Ledger,
            format!("line {}: the dev-tool folder `{name}` is not named `{prefix}<job>`.", e.line),
            "Release mode builds `./dev` into a new `release-<run id>-<job>` folder (PADDOCK_DEV_HOME), never a cached one.",
        );
    }
}

/// Identity of every `job` and `run` folder (ADR 0085 §5 item 2 and its Rust-tool amendment).
///
/// **Device and inode are the equality check:** a folder that was replaced after it was recorded is another object, whatever
/// its times say (128-bit ids on Windows). **The birth time is a floor against the job's start**: a folder born before the job
/// began is a kept one, whatever was written into it since (its change and modification times are not looked at: they move
/// with every write and can be set). The job's start is the birth time of `$RUNNER_TEMP`, not of `setup`, because `./dev` is
/// built into the dev-tool folder before `setup` runs. A birth time never changes for an object, so it is also compared for
/// equality with the recorded one. A file system that reports no birth time makes the check broken.
fn identity_probe(ctx: &mut Ctx) {
    let root = winpath::real(&ctx.ledger.job_root).unwrap_or_else(|_| ctx.ledger.job_root.clone());
    let started = ctx.ledger.started_ns;
    for e in ctx.ledger.entries.clone() {
        let (Some(mode), Some(recorded)) = (Mode::parse(&e.mode), e.id) else {
            continue;
        };
        if mode == Mode::Off {
            continue;
        }
        let canonical = match winpath::real(Path::new(&e.path)) {
            Ok(p) => p,
            Err(err) => {
                ctx.add(
                    Probe::Identity,
                    format!("`{}` {} is gone ({err}).", e.kind, e.path),
                    "Something removed it during the job: verify must run before prune-build-folder and host-cleanup.",
                );
                continue;
            }
        };
        if mode == Mode::Job && !canonical.starts_with(&root) {
            ctx.add(
                Probe::Identity,
                format!(
                    "`{}` {} is outside the job folder {}: a shared or kept folder.",
                    e.kind,
                    canonical.display(),
                    root.display()
                ),
                "Make the action create this folder under $RUNNER_TEMP in release mode.",
            );
            continue;
        }
        match identity_of(&canonical) {
            Err(err) if err.kind() == std::io::ErrorKind::Unsupported => ctx.add(
                Probe::Identity,
                format!("check broken: {err}."),
                "Use a file system with birth times (ext4, xfs, btrfs, tmpfs on a recent kernel, APFS, NTFS) for the job folder.",
            ),
            Err(err) => ctx.add(
                Probe::Identity,
                format!("`{}` {}: its identity cannot be read ({err}).", e.kind, canonical.display()),
                "Check the folder's permissions.",
            ),
            Ok(now) => {
                if (now.dev, now.ino, now.birth_ns) != (recorded.dev, recorded.ino, recorded.birth_ns) {
                    ctx.add(
                        Probe::Identity,
                        format!(
                            "`{}` {} is not the folder the ledger recorded: device {} inode {} born {} instead of device {} inode {} born {}.",
                            e.kind,
                            canonical.display(),
                            now.dev,
                            now.ino,
                            now.birth_ns,
                            recorded.dev,
                            recorded.ino,
                            recorded.birth_ns
                        ),
                        "A step replaced the folder after it was recorded (a move, a link, a restored cache): remove that step in release mode.",
                    );
                } else if recorded.birth_ns < started - ctx.opts.slack_ns {
                    ctx.add(
                        Probe::Identity,
                        format!(
                            "`{}` {} was born before this job started: it is a kept folder.",
                            e.kind,
                            canonical.display()
                        ),
                        "Create the folder during the job (mktemp-style, or `./dev` into a new release-<run id>-<job> folder), never adopt a kept one.",
                    );
                } else {
                    ctx.verdict.folders_confirmed += 1;
                }
            }
        }
    }
    if let (Some(t), Some(target)) = (ctx.folder(Kind::TauriTools), ctx.folder(Kind::Target))
        && !t.starts_with(&target)
    {
        ctx.add(
            Probe::Identity,
            format!("`tauri-tools` {} is not inside the build folder {}.", t.display(), target.display()),
            "Pass `--config '{\"bundle\":{\"useLocalToolsDir\":true}}'` to `tauri build` so the tools live in <build folder>/.tauri.",
        );
    }
}

fn same(a: &str, b: &Path) -> bool {
    winpath::real(Path::new(a)).map(|p| p == b).unwrap_or(false)
}

fn pointers_probe(ctx: &mut Ctx) {
    let env = ctx.env;
    let root = winpath::real(&ctx.ledger.job_root).unwrap_or_else(|_| ctx.ledger.job_root.clone());
    let expected = |k: Kind| ctx.opts.expect.contains(&k) || ctx.ledger.get(k).is_some();
    if expected(Kind::CargoHome) {
        match (env.get("CARGO_HOME"), ctx.folder(Kind::CargoHome)) {
            (None, Some(f)) => ctx.add(
                Probe::Pointers,
                format!(
                    "CARGO_HOME is not set: cargo would use the runner user's ~/.cargo, not {}.",
                    f.display()
                ),
                "Export the CARGO_HOME that `setup` printed (append it to $GITHUB_ENV).",
            ),
            (Some(v), Some(f)) if !same(v, &f) => ctx.add(
                Probe::Pointers,
                format!("CARGO_HOME is {v}, not the folder {} that the ledger holds.", f.display()),
                "A step overrides CARGO_HOME: remove it in release mode.",
            ),
            _ => {}
        }
    }
    if let (Some(v), Some(f)) = (env.get("CARGO_TARGET_DIR"), ctx.folder(Kind::Target))
        && !same(v, &f)
    {
        ctx.add(
            Probe::Pointers,
            format!("CARGO_TARGET_DIR is {v}, not the build folder {} of the ledger.", f.display()),
            "A step points the build at another folder: remove it in release mode.",
        );
    }
    if expected(Kind::Npm) {
        let v = env.get("npm_config_cache").or_else(|| env.get("NPM_CONFIG_CACHE"));
        match (v, ctx.folder(Kind::Npm)) {
            (None, Some(f)) => ctx.add(
                Probe::Pointers,
                format!(
                    "npm_config_cache is not set: npm would use the runner user's cache, not {}.",
                    f.display()
                ),
                "Export the npm_config_cache that `setup` printed.",
            ),
            (Some(v), Some(f)) if !same(v, &f) => ctx.add(
                Probe::Pointers,
                format!("npm_config_cache is {v}, not the folder {} of the ledger.", f.display()),
                "A step overrides the npm cache: remove it in release mode.",
            ),
            _ => {}
        }
    }
    if let Some(v) = env.get("SCCACHE_DIR") {
        let inside = winpath::real(Path::new(v)).map(|p| p.starts_with(&root)).unwrap_or(false);
        if !inside {
            ctx.add(
                Probe::Pointers,
                format!("SCCACHE_DIR is {v}, outside the job folder {}.", root.display()),
                "Unset SCCACHE_DIR in release mode (ADR 0085 §4 item 2).",
            );
        }
    }
}
