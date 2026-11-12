//! The built-in plugins: one binary, `debrief-plugins`, serves each of them in its own process
//! (`debrief-plugins --plugin <id>`). At start the host asks the binary for every built-in's manifest once
//! (`debrief-plugins --manifests`, a short run that loads no file); each plugin's process then starts on first use.

use crate::process::{PluginProcess, PluginSpec};
use crate::remote::RemotePlugin;
use debrief_core::Plugin;
use debrief_plugin_api::manifest::Manifest;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The flag that prints every built-in's manifest (a JSON array) and exits.
pub const MANIFESTS_FLAG: &str = "--manifests";
/// The flag that serves one built-in.
pub const PLUGIN_FLAG: &str = "--plugin";
/// Longest the manifest run may take.
const MANIFESTS_TIMEOUT: Duration = Duration::from_secs(30);
/// Largest manifest list read.
const MAX_MANIFESTS_BYTES: u64 = 4 << 20;

/// File name of the plugins binary on this platform (as bundled: no target-triple suffix).
pub fn binary_name() -> &'static str {
    if cfg!(windows) {
        "debrief-plugins.exe"
    } else {
        "debrief-plugins"
    }
}

/// Where the bundled plugins binary lives: next to the application executable (Tauri puts `externalBin`
/// sidecars there). `DEBRIEF_PLUGINS` overrides it in a debug build only: a release build ignores it (and logs its name
/// once), because the environment must not decide which program runs, possibly elevated. A test binary (in cargo's
/// `deps` folder) finds the one cargo built next to that folder.
pub fn default_binary() -> PathBuf {
    default_binary_with(cfg!(debug_assertions), std::env::var_os(PLUGINS_VAR))
}

/// The variable that names another plugins binary (development builds only).
pub const PLUGINS_VAR: &str = "DEBRIEF_PLUGINS";

/// [`default_binary`] for a given build mode and value of [`PLUGINS_VAR`].
fn default_binary_with(debug: bool, env: Option<std::ffi::OsString>) -> PathBuf {
    if let Some(p) = paddock_env::dev_override::resolve(debug, PLUGINS_VAR, env) {
        return PathBuf::from(p);
    }
    let dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(PathBuf::from))
        .unwrap_or_default();
    binary_in(&dir)
}

fn binary_in(dir: &Path) -> PathBuf {
    let here = dir.join(binary_name());
    match dir.parent() {
        Some(up) if !here.exists() && dir.file_name().is_some_and(|n| n == "deps") => up.join(binary_name()),
        _ => here,
    }
}

/// Runs `exe --manifests` and reads the manifests it prints; every manifest is checked.
pub fn load_manifests(exe: &Path) -> Result<Vec<Manifest>, String> {
    // The plugins binary is Debrief's own; it runs with a cleared environment and no stdin.
    #[allow(clippy::disallowed_methods)]
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(MANIFESTS_FLAG)
        .env_clear()
        .envs(
            debrief_ipc::process::KEPT_ENV
                .iter()
                .filter_map(|k| std::env::var_os(k).map(|v| (k, v))),
        )
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start the plugins ({}): {e}", exe.display()))?;
    let reader = child.stdout.take().map(|s| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = s.take(MAX_MANIFESTS_BYTES).read_to_end(&mut buf);
            buf
        })
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() < MANIFESTS_TIMEOUT => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("the plugins did not describe themselves in time".into());
            }
        }
    };
    let out = reader.and_then(|r| r.join().ok()).unwrap_or_default();
    if !status.success() {
        return Err(format!("the plugins could not describe themselves ({status})"));
    }
    let manifests: Vec<Manifest> = serde_json::from_slice(&out).map_err(|e| format!("the plugins' manifests are not valid: {e}"))?;
    for m in &manifests {
        m.check()?;
    }
    Ok(manifests)
}

/// The built-in plugins of this Debrief: their processes (one per plugin, shared by every window).
pub struct Builtins {
    processes: Vec<Arc<PluginProcess>>,
    /// Why the plugins are unavailable, when they are.
    problem: Option<String>,
}

impl Builtins {
    /// The built-ins `exe` serves, each granted what it asks for (they are first-party).
    pub fn load(exe: &Path) -> Builtins {
        Builtins::load_with(exe, |_| {})
    }

    /// Like [`Builtins::load`], with each plugin's start adjusted by `adjust` (tests: a connect window of
    /// [`debrief_ipc::process::UNTIL_CONNECTED`]).
    pub fn load_with(exe: &Path, adjust: impl Fn(&mut PluginSpec)) -> Builtins {
        match load_manifests(exe) {
            Ok(manifests) => Builtins::from_manifests_with(exe, manifests, adjust),
            Err(e) => {
                log::error!("Plugins unavailable: {e}");
                Builtins {
                    processes: Vec::new(),
                    problem: Some(e),
                }
            }
        }
    }

    /// Processes for `manifests` served by `exe`.
    pub fn from_manifests(exe: &Path, manifests: Vec<Manifest>) -> Builtins {
        Builtins::from_manifests_with(exe, manifests, |_| {})
    }

    /// [`Builtins::from_manifests`], each spec adjusted by `adjust`.
    pub fn from_manifests_with(exe: &Path, manifests: Vec<Manifest>, adjust: impl Fn(&mut PluginSpec)) -> Builtins {
        let processes = manifests
            .into_iter()
            .map(|m| {
                let mut spec = PluginSpec::new(exe, &m.id);
                spec.granted = m.permissions.iter().map(|p| p.permission).collect();
                adjust(&mut spec);
                PluginProcess::new(m, spec)
            })
            .collect();
        Builtins { processes, problem: None }
    }

    /// Adds an installed third-party plugin's process (ADR 0027); a built-in with the same id wins.
    pub fn add_installed(&mut self, process: Arc<PluginProcess>) -> Result<(), String> {
        if self.processes.iter().any(|p| p.id() == process.id()) {
            return Err(format!("plugin \"{}\" is already part of Debrief", process.id()));
        }
        self.processes.push(process);
        Ok(())
    }

    /// Settings ▸ Performance's limits for every plugin process here (from each one's next start).
    pub fn set_limits(&self, limits: &crate::PluginLimits) {
        for p in &self.processes {
            p.set_limits(limits.clone());
        }
    }

    /// No plugins (safe mode).
    pub fn none() -> Builtins {
        Builtins {
            processes: Vec::new(),
            problem: None,
        }
    }

    /// Why no plugin is available, when the plugins binary could not be used.
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// One plugin per process, to install into a window's registry.
    pub fn plugins(&self) -> Vec<Arc<dyn Plugin>> {
        self.processes
            .iter()
            .map(|p| Arc::new(RemotePlugin::new(p.clone())) as Arc<dyn Plugin>)
            .collect()
    }

    pub fn process(&self, id: &str) -> Option<&Arc<PluginProcess>> {
        self.processes.iter().find(|p| p.id() == id)
    }

    pub fn processes(&self) -> &[Arc<PluginProcess>] {
        &self.processes
    }

    /// Starts every plugin's process in the background, so the first import does not wait for one.
    pub fn warm_up(&self) {
        for p in &self.processes {
            let p = p.clone();
            let _ = std::thread::Builder::new()
                .name("debrief-plugin-start".into())
                .spawn(move || p.warm_up());
        }
    }

    /// Stops every plugin process (on exit; they would also exit when Debrief does).
    pub fn shutdown(&self) {
        // Together: each stop may wait for its process (a hung one is killed after its grace), and one after the other two
        // hung plugins overran the worker's whole stop budget.
        std::thread::scope(|scope| {
            for p in &self.processes {
                scope.spawn(move || p.shutdown());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_binary_in_deps_finds_the_one_cargo_built_beside_it() {
        let t = tempfile::tempdir().unwrap();
        let deps = t.path().join("deps");
        std::fs::create_dir(&deps).unwrap();
        assert_eq!(binary_in(&deps), t.path().join(binary_name()));
        std::fs::write(deps.join(binary_name()), b"").unwrap();
        assert_eq!(binary_in(&deps), deps.join(binary_name()));
        assert_eq!(binary_in(t.path()), t.path().join(binary_name()));
    }

    #[test]
    fn the_binary_is_next_to_the_executable_unless_overridden_in_a_debug_build() {
        let custom = Some(std::ffi::OsString::from("/custom/plugins"));
        assert_eq!(
            default_binary_with(true, None).file_name().unwrap().to_str().unwrap(),
            binary_name()
        );
        assert_eq!(default_binary_with(true, custom.clone()), PathBuf::from("/custom/plugins"));
        // A release build ignores the variable: the plugins come from the install location only.
        let release = default_binary_with(false, custom);
        assert_ne!(release, PathBuf::from("/custom/plugins"));
        assert_eq!(release.file_name().unwrap().to_str().unwrap(), binary_name());
    }

    #[test]
    fn a_missing_binary_means_no_plugins_with_a_reason() {
        let b = Builtins::load(Path::new("/definitely/not/debrief-plugins"));
        assert!(b.plugins().is_empty());
        assert!(b.problem().unwrap().contains("cannot start the plugins"));
        assert!(Builtins::none().problem().is_none());
    }
}
