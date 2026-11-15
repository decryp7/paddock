//! `./dev app bootstrap <app>` and `./dev app verify-installed <app>`: build an app on this machine and check an installed one
//! (the port of `tools/bootstrap.sh`, `tools/verify-installed.sh` and the apps' `{linux,macos}-{bootstrap,verify}.sh`, shell-to-Rust
//! WP-7; one command for every app, rule 55).
//!
//! The facts come from where they already live: the Node major from the root `.nvmrc` ([`node_major`] is its one reader), and the
//! installed layout (program, launchd label, systemd unit, socket, log, runner logs) from the `install` section of
//! `apps/<app>/paddock.ci.json` ([`Install`], documented in `.gitea/actions/README.md`). Both commands install nothing and verify
//! changes nothing: bootstrap runs `npm ci` and the Tauri build; verify only reads and runs the service's own `status`.

use crate::ci_tool::{Output, ToolCall};
use clap::{Args, Subcommand};
use paddock_cli::{CliError, Report};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `./dev app <command>`.
#[derive(Subcommand)]
pub enum AppCommand {
    /// Check the prerequisites, run `npm ci`, build the installers and say where they are.
    Bootstrap(BootstrapArgs),
    /// Read-only check of an installed app: bundle, launch agent or user unit, socket, service answer, logs.
    VerifyInstalled(VerifyArgs),
}

/// The arguments of `app bootstrap`.
#[derive(Args)]
pub struct BootstrapArgs {
    /// The app under `apps/`.
    app: String,
    /// Only check the prerequisites.
    #[arg(long)]
    check: bool,
    /// The oldest macOS (the launch agent registration needs 13).
    #[arg(long, default_value_t = 13)]
    min_macos: u32,
    /// The repository root (default: the current repository).
    #[arg(long)]
    root: Option<PathBuf>,
    /// Pretend to be this OS: `macos` or `linux` (tests).
    #[arg(long, hide = true)]
    os: Option<String>,
    /// Pretend to be this architecture (tests).
    #[arg(long, hide = true)]
    arch: Option<String>,
}

/// The arguments of `app verify-installed`.
#[derive(Args)]
pub struct VerifyArgs {
    /// The app under `apps/`.
    app: String,
    /// The app bundle (macOS; default `/Applications/<name>.app`).
    #[arg(long)]
    bundle: Option<PathBuf>,
    /// The app data folder (default: paddock-env's: `~/Library/Application Support/<name>` or `$XDG_DATA_HOME/<name>`).
    #[arg(long)]
    data: Option<PathBuf>,
    /// The repository root (default: the current repository).
    #[arg(long)]
    root: Option<PathBuf>,
    /// Pretend to be this OS: `macos` or `linux` (tests).
    #[arg(long, hide = true)]
    os: Option<String>,
}

/// The installed layout of an app: the `install` section of its `paddock.ci.json`.
#[derive(Debug, Clone, Default)]
pub struct Install {
    /// The product name (bundle `<name>.app`, data folder `<name>`).
    pub name: String,
    /// The background program: in the bundle's `Contents/MacOS`, on `PATH`, or in `<data>/bin`.
    pub service: Option<String>,
    /// The launchd label of the bundled agent (macOS).
    pub label: Option<String>,
    /// The systemd user unit (Linux).
    pub unit: Option<String>,
    /// The service's socket file name, in the data folder (Linux: or `$XDG_RUNTIME_DIR/<id>`).
    pub socket: Option<String>,
    /// The service log, relative to the data folder (default `logs/service.log`).
    pub log: Option<String>,
    /// Runner logs: `{ "dir": "runners", "log": "logs/runner.log", "accepted": "declare successfully" }`.
    pub runners: Option<Runners>,
}

/// Per-runner logs under the data folder.
#[derive(Debug, Clone)]
pub struct Runners {
    /// The folder of runner folders, in the data folder.
    pub dir: String,
    /// A runner's log, relative to its folder.
    pub log: String,
    /// The line that says the server accepted the runner.
    pub accepted: String,
}

/// The `install` section of `apps/<app>/paddock.ci.json`.
pub fn load_install(root: &Path, app: &str) -> Result<Install, CliError> {
    if app.is_empty() || app.contains(['/', '\\']) || app.starts_with('.') {
        return Err(CliError::usage(format!("`{app}` is not an app folder name.")));
    }
    let file = root.join("apps").join(app).join("paddock.ci.json");
    let text = std::fs::read_to_string(&file).map_err(|e| {
        CliError::usage(format!("{} cannot be read: {e}.", file.display())).with_hint("Name an app under apps/, for example `pitcrew`.")
    })?;
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| CliError::failure(format!("::error::{} is not JSON: {e}. Next step: fix the file.", file.display())))?;
    let section = v.get("install").ok_or_else(|| {
        CliError::failure(format!(
            "::error::{} has no `install` section. Next step: add one ({{\"name\": \"...\", ...}}; the fields are in .gitea/actions/README.md).",
            file.display()
        ))
    })?;
    parse_install(section).map_err(|e| {
        CliError::failure(format!(
            "::error::{}: bad `install` section: {e}. Next step: fix it.",
            file.display()
        ))
    })
}

/// Reads the section by hand (no serde derive here): known keys only, each a string.
fn parse_install(v: &serde_json::Value) -> Result<Install, String> {
    const KEYS: [&str; 6] = ["name", "service", "label", "unit", "socket", "log"];
    let o = v.as_object().ok_or("it is not an object")?;
    for k in o.keys() {
        if !KEYS.contains(&k.as_str()) && k != "runners" {
            return Err(format!("unknown field `{k}`"));
        }
    }
    let text = |m: &serde_json::Map<String, serde_json::Value>, k: &str| -> Result<Option<String>, String> {
        match m.get(k) {
            None => Ok(None),
            Some(x) => x
                .as_str()
                .filter(|t| !t.is_empty())
                .map(|t| Some(t.to_string()))
                .ok_or(format!("`{k}` is not a non-empty string")),
        }
    };
    let runners = match o.get("runners") {
        None => None,
        Some(r) => {
            let m = r.as_object().ok_or("`runners` is not an object")?;
            for k in m.keys() {
                if !["dir", "log", "accepted"].contains(&k.as_str()) {
                    return Err(format!("unknown field `runners.{k}`"));
                }
            }
            let need = |k: &str| text(m, k)?.ok_or(format!("`runners.{k}` is missing"));
            Some(Runners {
                dir: need("dir")?,
                log: need("log")?,
                accepted: need("accepted")?,
            })
        }
    };
    Ok(Install {
        name: text(o, "name")?.ok_or("`name` is missing")?,
        service: text(o, "service")?,
        label: text(o, "label")?,
        unit: text(o, "unit")?,
        socket: text(o, "socket")?,
        log: text(o, "log")?,
        runners,
    })
}

/// The Node major of the root `.nvmrc` (`24`, `v24.1.0` and `24.1` all give `24`): the one reader of the file for the family's tools.
pub fn node_major(root: &Path) -> Result<String, CliError> {
    let file = root.join(".nvmrc");
    let text = std::fs::read_to_string(&file).map_err(|e| {
        CliError::failure(format!(
            "::error::{} cannot be read: {e}. Next step: run from the repository root.",
            file.display()
        ))
    })?;
    let cleaned: String = text.chars().filter(|c| !c.is_whitespace() && *c != 'v').collect();
    let major = cleaned.split('.').next().unwrap_or_default().to_string();
    if major.is_empty() || !major.chars().all(|c| c.is_ascii_digit()) {
        return Err(CliError::failure(format!(
            "::error::{} holds `{}`, not a Node version. Next step: write the major, for example 24.",
            file.display(),
            text.trim()
        )));
    }
    Ok(major)
}

fn env_map() -> HashMap<String, String> {
    std::env::vars().collect()
}

/// Runs a probe program; `Some(stdout)` when it exits 0.
fn probe(program: &str, args: &[&str], env: &HashMap<String, String>) -> Option<String> {
    let r = ToolCall::new(program)
        .args(args.iter().copied())
        .pass(["XDG_*", "DBUS_*", "USER", "LOGNAME", "UID"])
        .timeout(Duration::from_secs(30))
        .run(env, &mut std::io::sink())
        .ok()?;
    r.success.then_some(r.stdout)
}

fn os_of(given: Option<&str>) -> &str {
    given.unwrap_or(std::env::consts::OS)
}

/// One checklist line the way the scripts printed it.
struct Lines {
    text: String,
    problems: usize,
}

impl Lines {
    fn new() -> Lines {
        Lines {
            text: String::new(),
            problems: 0,
        }
    }
    fn ok(&mut self, s: &str) {
        let _ = writeln!(self.text, "  ok    {s}");
    }
    fn missing(&mut self, s: &str, next: &str) {
        let _ = writeln!(self.text, "  missing  {s}\n      {next}");
        self.problems += 1;
    }
    fn fail(&mut self, s: &str, next: &str) {
        let _ = writeln!(self.text, "  fail  {s}\n        {next}");
        self.problems += 1;
    }
    fn note(&mut self, s: &str) {
        let _ = writeln!(self.text, "  ..    {s}");
    }
}

/// `./dev app bootstrap`.
pub fn bootstrap(a: &BootstrapArgs) -> Result<Report, CliError> {
    let root = crate::gates::command::repo_root(a.root.as_deref())?;
    let install = load_install(&root, &a.app)?;
    let name = install.name.clone();
    let env = env_map();
    let os = os_of(a.os.as_deref());
    let mut l = Lines::new();
    let (kind, bundles) = match os {
        "macos" => ("macos", "app,dmg"),
        "linux" => ("linux", "deb,appimage"),
        other => {
            return Err(CliError::failure(format!(
                "::error::{other} is not supported by bootstrap (macOS and Linux; on Windows the NSIS build runs in CI). Next step: build on a Mac or Linux machine."
            )));
        }
    };
    let _ = writeln!(l.text, "{name} {kind} bootstrap");
    if kind == "macos" {
        let ver = probe("sw_vers", &["-productVersion"], &env).unwrap_or_default();
        let ver = ver.trim().to_string();
        let major: u32 = ver.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0);
        if major >= a.min_macos {
            l.ok(&format!("macOS {ver} ({} or newer)", a.min_macos));
        } else {
            l.missing(
                &format!("macOS {} or newer", a.min_macos),
                &format!(
                    "found {}; the launch agent registration (SMAppService) needs macOS {}",
                    if ver.is_empty() { "0" } else { &ver },
                    a.min_macos
                ),
            );
        }
        if probe("xcode-select", &["-p"], &env).is_some() {
            l.ok("Xcode command line tools");
        } else {
            l.missing("Xcode command line tools", "run: xcode-select --install");
        }
    } else {
        let arch = a.arch.clone().unwrap_or_else(|| std::env::consts::ARCH.to_string());
        if arch == "x86_64" || arch == "aarch64" {
            l.ok(&format!("architecture {arch}"));
        } else {
            l.missing("architecture", &format!("{name} builds for x86_64 and aarch64 only (found {arch})"));
        }
    }
    match probe("rustc", &["--version"], &env) {
        Some(v) if probe("cargo", &["--version"], &env).is_some() => {
            l.ok(&format!("Rust {}", v.split_whitespace().nth(1).unwrap_or("")));
        }
        _ => l.missing("Rust", "install rustup from https://rustup.rs (user-level), then open a new shell"),
    }
    let want = node_major(&root)?;
    match probe("node", &["--version"], &env) {
        Some(v) => {
            let have = v.trim().trim_start_matches('v').split('.').next().unwrap_or("").to_string();
            if have == want {
                l.ok(&format!("Node {}", v.trim()));
            } else {
                l.missing(
                    &format!("Node {want}"),
                    &format!("found Node {}; install Node {want} (.nvmrc; e.g. with nvm)", v.trim()),
                );
            }
        }
        None => l.missing(&format!("Node {want}"), &format!("install Node {want} (https://nodejs.org or nvm)")),
    }
    if kind == "linux" {
        if probe("pkg-config", &["--version"], &env).is_some() {
            for pkg in ["webkit2gtk-4.1", "gtk+-3.0", "librsvg-2.0", "ayatana-appindicator3-0.1"] {
                if probe("pkg-config", &["--exists", pkg], &env).is_some() {
                    l.ok(&format!("{pkg} (development files)"));
                } else {
                    l.missing(pkg, "sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf file xdg-utils");
                }
            }
        } else {
            l.missing("pkg-config", "sudo apt install pkg-config build-essential");
        }
        if probe("systemctl", &["--version"], &env).is_some() {
            l.ok("systemd (a background service runs as a user unit)");
        } else {
            l.note("no systemd: a background service can run in the foreground, but the service switch in Settings needs systemd");
        }
    }
    if l.problems > 0 {
        let text = format!("{}\nInstall what is missing, then run this again.", l.text);
        return Err(CliError::failure(text).with_hint("Install what is missing, then run `./dev app bootstrap` again."));
    }
    if a.check {
        return Ok(Report::new(
            serde_json::json!({ "app": a.app, "ok": true }),
            l.text.trim_end().to_string(),
        ));
    }
    let _ = writeln!(l.text, "\nBuilding (a first build takes several minutes)...");
    print!("{}", l.text);
    l.text.clear();
    let stream = |program: &str, args: &[&str], dir: PathBuf, ci: bool| -> Result<(), CliError> {
        let mut call = ToolCall::new(program)
            .args(args.iter().copied())
            .cwd(dir)
            .output(Output::Stream)
            .pass([
                "HOME",
                "CARGO_*",
                "RUSTUP_*",
                "TAURI_*",
                "npm_*",
                "NPM_*",
                "APPLE_*",
                "SCCACHE_*",
                "RUSTC_WRAPPER",
            ])
            .timeout(Duration::from_secs(3 * 3600));
        if ci {
            call = call.env("CI", "true");
        }
        let ran = call.run(&env, &mut std::io::stderr()).map_err(CliError::failure)?;
        if ran.success {
            Ok(())
        } else {
            Err(CliError::failure(format!(
                "::error::`{program} {}` failed: {}. Next step: read its output above and run it again.",
                args.join(" "),
                ran.how
            )))
        }
    };
    // The monorepo has one npm workspace at the root. Unsigned updater artefacts unless the signing key is in the environment.
    stream("npm", &["ci", "--no-audit", "--no-fund"], root.clone(), false)?;
    stream(
        "npx",
        &["tauri", "build", "--bundles", bundles, "--", "--profile", "dist"],
        root.join("apps").join(&a.app).join("app"),
        true,
    )?;
    let target = env
        .get("CARGO_TARGET_DIR")
        .map_or_else(|| root.join("apps").join(&a.app).join("target"), PathBuf::from);
    let find = |sub: &[&str], ext: Option<&str>, exact: Option<&str>| -> String {
        for base in [target.join("dist/bundle"), target.join("dist/release/bundle")] {
            let dir = sub.iter().fold(base, |p, s| p.join(s));
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.filter_map(Result::ok) {
                let n = e.file_name().to_string_lossy().into_owned();
                if exact.is_some_and(|x| x == n) || ext.is_some_and(|x| n.ends_with(x)) {
                    return e.path().display().to_string();
                }
            }
        }
        "not found".to_string()
    };
    let mut out = String::from("\nBuilt:\n");
    if kind == "macos" {
        let _ = writeln!(out, "  app: {}", find(&["macos"], None, Some(&format!("{name}.app"))));
        let _ = writeln!(out, "  dmg: {}", find(&["dmg"], Some(".dmg"), None));
    } else {
        let _ = writeln!(out, "  deb:      {}", find(&["deb"], Some(".deb"), None));
        let _ = writeln!(out, "  AppImage: {}", find(&["appimage"], Some(".AppImage"), None));
    }
    let _ = write!(
        out,
        "\nNext steps: install the build and start {name} (apps/{}/docs/{}.md has the details), then run:\n  ./dev app verify-installed {}",
        a.app,
        if kind == "macos" { "macos" } else { "linux" },
        a.app
    );
    Ok(Report::new(serde_json::json!({ "app": a.app, "built": true }), out))
}

/// The `state` and `pid` of `launchctl print` output (tab-indented `state = running`).
pub fn parse_launchctl(text: &str) -> (Option<String>, Option<String>) {
    let field = |key: &str| {
        text.lines()
            .find_map(|l| {
                l.strip_prefix('\t')
                    .and_then(|r| r.strip_prefix(key))
                    .and_then(|r| r.strip_prefix(" = "))
            })
            .map(|v| v.trim().to_string())
    };
    (field("state"), field("pid"))
}

/// `./dev app verify-installed`.
pub fn verify_installed(a: &VerifyArgs) -> Result<Report, CliError> {
    let root = crate::gates::command::repo_root(a.root.as_deref())?;
    let i = load_install(&root, &a.app)?;
    let env = env_map();
    let os = os_of(a.os.as_deref());
    let home = env.get("HOME").cloned().unwrap_or_default();
    let name = &i.name;
    let mut l = Lines::new();
    let _ = writeln!(l.text, "{name} on this machine");
    let mut svc: Option<PathBuf> = None;
    let data;
    match os {
        "macos" => {
            let bundle = a
                .bundle
                .clone()
                .unwrap_or_else(|| PathBuf::from(format!("/Applications/{name}.app")));
            data = a
                .data
                .clone()
                .unwrap_or_else(|| Path::new(&home).join("Library/Application Support").join(name));
            if bundle.is_dir() {
                l.ok(&format!("app bundle {}", bundle.display()));
            } else {
                l.fail("app bundle", &format!("not found at {} (install from the .dmg)", bundle.display()));
            }
            if let Some(s) = &i.service {
                let p = bundle.join("Contents/MacOS").join(s);
                if p.is_file() {
                    l.ok("service binary in the bundle");
                    svc = Some(p);
                } else {
                    l.fail("service binary", &format!("{} is missing; rebuild", p.display()));
                }
            }
            if let Some(label) = &i.label {
                let plist = bundle.join("Contents/Library/LaunchAgents").join(format!("{label}.plist"));
                if plist.is_file() {
                    l.ok("launch agent plist in the bundle");
                    if probe("plutil", &["-lint", &plist.to_string_lossy()], &env).is_some() {
                        l.ok("plist is valid");
                    } else {
                        l.note("plist could not be checked");
                    }
                } else {
                    l.fail(
                        "launch agent plist",
                        &format!("Contents/Library/LaunchAgents/{label}.plist is missing; rebuild"),
                    );
                }
                let uid = probe("id", &["-u"], &env).unwrap_or_default();
                match probe("launchctl", &["print", &format!("gui/{}/{label}", uid.trim())], &env) {
                    Some(out) => {
                        let (state, pid) = parse_launchctl(&out);
                        if state.as_deref() == Some("running") {
                            l.ok(&format!("launchd runs the agent (pid {})", pid.as_deref().unwrap_or("?")));
                        } else {
                            l.fail("launch agent", &format!("launchd knows it but its state is '{}' (a stale registration: turn the service off and on in Settings)", state.as_deref().unwrap_or("?")));
                        }
                    }
                    None => l.fail("launch agent", &format!("not loaded: open {name} > Settings > Service > Turn on, and allow it in System Settings > General > Login Items")),
                }
            }
        }
        "linux" => {
            data = a.data.clone().unwrap_or_else(|| {
                env.get("XDG_DATA_HOME")
                    .filter(|v| !v.is_empty())
                    .map_or_else(|| Path::new(&home).join(".local/share"), PathBuf::from)
                    .join(name)
            });
            if let Some(s) = &i.service {
                let on_path = env.get("PATH").and_then(|p| crate::pathfind::find_program(p, s, false, &[]));
                let in_data = Some(data.join("bin").join(s)).filter(|p| p.is_file());
                match on_path.or(in_data) {
                    Some(p) => {
                        l.ok(&format!("service program {}", p.display()));
                        svc = Some(p);
                    }
                    None => l.fail(
                        "service program",
                        &format!("{s} not found: install the .deb (it is /usr/bin/{s}) or turn the service on from the AppImage"),
                    ),
                }
            }
            if let Some(unit) = &i.unit {
                if probe("systemctl", &["--version"], &env).is_some() {
                    if probe("systemctl", &["--user", "show-environment"], &env).is_some() {
                        if probe("systemctl", &["--user", "cat", unit], &env).is_some() {
                            l.ok(&format!("user unit {unit} is installed"));
                        } else {
                            l.fail("user unit", &format!("not installed: open {name} > Settings > Service > Turn on"));
                        }
                        let state = probe("systemctl", &["--user", "is-active", unit], &env).unwrap_or_default();
                        if state.trim() == "active" {
                            l.ok("systemd runs the service (active)");
                        } else {
                            l.fail(
                                "service state",
                                &format!(
                                    "'{}': systemctl --user status {unit}; journalctl --user -u {unit} -n 30",
                                    if state.trim().is_empty() { "unknown" } else { state.trim() }
                                ),
                            );
                        }
                        if probe("systemctl", &["--user", "is-enabled", unit], &env).is_some() {
                            l.ok("enabled: it starts at log-in");
                        } else {
                            l.note("not enabled (starts only when turned on)");
                        }
                    } else {
                        l.fail("systemd user instance", "not reachable from this shell (ssh without a session?). Log in on the machine, or run 'loginctl enable-linger $USER' and use a full session");
                    }
                    let user = env.get("USER").cloned().unwrap_or_default();
                    let linger = probe("loginctl", &["show-user", &user, "-p", "Linger", "--value"], &env).unwrap_or_default();
                    if linger.trim() == "yes" {
                        l.ok("lingering is on: the service keeps going after log-out");
                    } else {
                        l.note("lingering is off: the service stops at log-out. To keep it running: loginctl enable-linger $USER");
                    }
                } else if let Some(s) = &i.service {
                    l.note(&format!("no systemd: start the service by hand ({s} run)"));
                }
            }
        }
        other => return Err(CliError::usage(format!("{other} is not supported (macOS and Linux)."))),
    }
    if let Some(sock) = &i.socket {
        let id = a.app.as_str();
        let mut dir = data.clone();
        if os == "linux"
            && let Some(rt) = env.get("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())
            && Path::new(rt).join(id).join(sock).exists()
        {
            dir = Path::new(rt).join(id);
        }
        if dir.join(sock).exists() {
            l.ok(&format!("service socket {}", dir.join(sock).display()));
        } else {
            l.fail(
                "service socket",
                &format!("{} is missing: the service is not running", dir.join(sock).display()),
            );
        }
    }
    let log = data.join(i.log.as_deref().unwrap_or("logs/service.log"));
    if let Some(s) = &svc {
        let _ = writeln!(l.text);
        let ran = ToolCall::new(s.to_string_lossy())
            .arg_status()
            .timeout(Duration::from_secs(60))
            .run(&env, &mut std::io::sink());
        match ran {
            Ok(r) if r.success => l.text.push_str(&r.stdout),
            Ok(r) => {
                l.text.push_str(&r.stdout);
                l.fail("service status", &format!("the service did not answer; see {}", log.display()));
            }
            Err(_) => l.fail("service status", &format!("the service did not answer; see {}", log.display())),
        }
    }
    if let Some(r) = &i.runners
        && let Ok(rd) = std::fs::read_dir(data.join(&r.dir))
    {
        {
            let mut names: Vec<_> = rd
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            for n in names {
                let f = data.join(&r.dir).join(&n).join(&r.log);
                let Ok(text) = std::fs::read_to_string(&f) else { continue };
                let _ = writeln!(l.text, "\nRunner {n}, last lines of {}", f.display());
                for line in text.lines().rev().take(8).collect::<Vec<_>>().into_iter().rev() {
                    let _ = writeln!(l.text, "    {line}");
                }
                if text.contains(&r.accepted) {
                    l.ok(&format!("{n} was accepted by its server"));
                } else {
                    l.note(&format!("{n} has not been accepted by its server yet"));
                }
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string(&log) {
        let _ = writeln!(l.text, "\nService log (last lines of {})", log.display());
        for line in text.lines().rev().take(5).collect::<Vec<_>>().into_iter().rev() {
            let _ = writeln!(l.text, "    {line}");
        }
    }
    if l.problems == 0 {
        let _ = write!(l.text, "\nEverything looks right.");
        Ok(Report::new(serde_json::json!({ "app": a.app, "problems": 0 }), l.text))
    } else {
        Err(CliError::failure(format!("{}\n{} problem(s) found.", l.text, l.problems)))
    }
}

trait StatusArg {
    fn arg_status(self) -> Self;
}

impl StatusArg for ToolCall {
    fn arg_status(self) -> Self {
        self.args(["status"])
    }
}

/// Runs one `app` command.
pub fn command(c: AppCommand) -> Result<Report, CliError> {
    match c {
        AppCommand::Bootstrap(a) => bootstrap(&a),
        AppCommand::VerifyInstalled(a) => verify_installed(&a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchctl_state_and_pid_are_read_from_tab_indented_lines() {
        let (s, p) = parse_launchctl("gui/501/x = {\n\tstate = running\n\tpid = 42\n\tother = 1\n}\n");
        assert_eq!((s.as_deref(), p.as_deref()), (Some("running"), Some("42")));
        assert_eq!(parse_launchctl("nothing here"), (None, None));
    }

    #[test]
    fn the_node_major_is_read_from_every_spelling() {
        let d = tempfile::tempdir().unwrap();
        for (text, want) in [("24\n", "24"), ("v24.1.0\r\n", "24"), (" 22.3 ", "22")] {
            std::fs::write(d.path().join(".nvmrc"), text).unwrap();
            assert_eq!(node_major(d.path()).unwrap(), want);
        }
        std::fs::write(d.path().join(".nvmrc"), "lts/*\n").unwrap();
        assert!(node_major(d.path()).is_err());
    }

    #[test]
    fn an_install_section_with_an_unknown_field_is_refused_and_a_missing_one_named() {
        let d = tempfile::tempdir().unwrap();
        let app = d.path().join("apps/x");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("paddock.ci.json"), r#"{"cargo":"."}"#).unwrap();
        assert!(load_install(d.path(), "x").unwrap_err().text().contains("no `install` section"));
        std::fs::write(app.join("paddock.ci.json"), r#"{"install":{"name":"X","bogus":1}}"#).unwrap();
        assert!(load_install(d.path(), "x").unwrap_err().text().contains("bad `install` section"));
        std::fs::write(app.join("paddock.ci.json"), r#"{"install":{"name":"X","unit":"x.service"}}"#).unwrap();
        assert_eq!(load_install(d.path(), "x").unwrap().unit.as_deref(), Some("x.service"));
        assert!(load_install(d.path(), "../x").is_err());
    }
}
