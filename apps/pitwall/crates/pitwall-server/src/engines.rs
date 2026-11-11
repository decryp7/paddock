//! Engines (ADR 0004, ADR 0010): the [`Engine`] trait the server serves models through, and llama.cpp's install
//! management. The install flow (pinned catalog, SHA-256 checked before unpacking, defensive unpacking, self-test,
//! roll back on failure) is adapted from Debrief's `imt-ai` runtime manager.

use crate::egress::{CancelFlag, DownloadError, Egress};
use crate::paths::Dirs;
use crate::process;
use crate::serving::{Blocked, CapabilityGate};
use pitwall_core::api::{ApiError, ErrorKind};
use pitwall_core::hardware::Hardware;
use pitwall_core::llama::{self, Launch};
use pitwall_core::mgmt::{Availability, EngineKind, SelfTest};
use pitwall_core::privacy::DownloadUrl;
use pitwall_core::runtimes::capabilities::{BuildFacts, Gate, Matrix};
use pitwall_core::runtimes::pins::{self, EnginePins, Installed, PinError, Selection};
use pitwall_core::runtimes::{self, builds, BuildChoice, BuildId, RuntimeBuild, RuntimeCatalog};
use pitwall_core::select::Capability;
use pitwall_loopback::{Client, LoopbackPort};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

/// Why an engine operation failed (the message says what to do).
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// No usable engine build is installed.
    #[error("llama.cpp is not installed yet. Pitwall installs the build suggested for this machine by itself; try again in a moment.")]
    NotInstalled,
    /// The engine does not exist on this machine.
    #[error("{0}")]
    NotAvailable(String),
    /// The process failed to start or become healthy.
    #[error("the engine failed to start: {0}")]
    Start(String),
    /// A download failed.
    #[error(transparent)]
    Download(#[from] DownloadError),
    /// The archive or build was unusable.
    #[error("{0}")]
    Install(String),
    /// The build is older than the minimum Pitwall can run (`--jinja`, `--offline`).
    #[error("{0}")]
    TooOld(String),
    /// The build pinned for the model cannot be used (ADR 0018).
    #[error(transparent)]
    Pin(#[from] PinError),
    /// The engine could not load because the Microsoft Visual C++ runtime is missing (Windows `STATUS_DLL_NOT_FOUND`); the
    /// names are the runtime files absent from System32.
    #[error("{}", missing_runtime_message(.0))]
    MissingRuntime(Vec<String>),
    /// The engine could not load because a DLL it imports is missing although the Visual C++ runtime is installed.
    #[error(
        "The engine could not start because a DLL it needs is missing (Windows status 0xC0000135, STATUS_DLL_NOT_FOUND), although the \
         Visual C++ runtime is installed. Next step: a file of the engine's folder may have been removed (for example by antivirus \
         software); reinstall the engine from Settings > Engines, and check the antivirus quarantine."
    )]
    EngineDllMissing,
}

/// The Windows status a process ends with when the loader cannot find a DLL it imports (`STATUS_DLL_NOT_FOUND`).
pub const STATUS_DLL_NOT_FOUND: u32 = 0xC000_0135;

/// The runtime files a llama.cpp engine for Windows imports from the Visual C++ Redistributable (the installer brings them).
pub const VC_RUNTIME_FILES: [&str; 3] = ["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll"];

/// Whether `name` is in the Windows System32 folder (always `true` off Windows: nothing to check there).
pub fn in_system32(name: &str) -> bool {
    if !cfg!(windows) {
        return true;
    }
    let root = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("windir"))
        .unwrap_or_else(|| "C:\\Windows".into());
    Path::new(&root).join("System32").join(name).is_file()
}

/// What an engine's exit status says about why it never ran: `Some` for a status Pitwall can explain. `code` is the raw
/// exit code (`ExitStatus::code`; on Windows the 32-bit status, which Rust reports as an `i32`); `present` says whether a
/// runtime file exists in System32, so the words blame the runtime only when it is really missing.
pub fn exit_problem(code: Option<i32>, present: &dyn Fn(&str) -> bool) -> Option<EngineError> {
    // `as u32` reinterprets the i32 Windows status; a Unix exit code is never 0xC0000135 (they are 0 to 255).
    code.filter(|c| *c as u32 == STATUS_DLL_NOT_FOUND)?;
    let missing: Vec<String> = VC_RUNTIME_FILES.iter().filter(|f| !present(f)).map(|f| (*f).to_string()).collect();
    Some(if missing.is_empty() {
        EngineError::EngineDllMissing
    } else {
        EngineError::MissingRuntime(missing)
    })
}

/// The words for a PC without the Visual C++ runtime: the cause, the raw status and the next step.
fn missing_runtime_message(missing: &[String]) -> String {
    let installer = if cfg!(target_arch = "aarch64") {
        "vc_redist.arm64.exe"
    } else {
        "vc_redist.x64.exe"
    };
    format!(
        "The engine could not start because this PC has no Microsoft Visual C++ runtime ({} missing from System32; Windows status \
         0xC0000135, STATUS_DLL_NOT_FOUND). Next step: install \"Microsoft Visual C++ Redistributable\" ({installer} from \
         https://aka.ms/vc14/{installer}), or reinstall Pitwall, whose installer brings it, then try again.",
        missing.join(", ")
    )
}

/// A boxed future (the trait stays object-safe without extra crates).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A running engine process serving one model on a loopback port.
pub struct Running {
    /// The child; dropping it kills the process (`None` for in-process test engines).
    pub child: Option<tokio::process::Child>,
    /// Anything else that must live as long as the engine (test doubles).
    pub guard: Option<Box<dyn Send + Sync>>,
    /// The client to reach it.
    pub client: Client,
    /// The loopback port.
    pub port: u16,
    /// What `/props` reported.
    pub props: llama::Props,
}

/// An engine adapter: starts a process that serves one model. Implementations honour the same contract: the returned
/// engine is healthy, bound to 127.0.0.1 with a private key, and killed when dropped.
pub trait Engine: Send + Sync {
    /// Which engine this is.
    fn kind(&self) -> EngineKind;
    /// The id models name it by (`llama.cpp`, or a pack's id).
    fn id(&self) -> String {
        "llama.cpp".into()
    }
    /// Names the build that would serve `model` (part of cache keys, ADR 0019); empty when there is no such notion.
    fn build_tag(&self, _model: &str) -> String {
        String::new()
    }
    /// Resolves when the engine can start a model: waits while it is being set up (llama.cpp installing itself) and
    /// fails with the reason when setup failed. Engines with nothing to set up are ready at once.
    fn ready(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(async { Ok(()) })
    }
    /// Whether it can run here, and why not.
    fn availability(&self) -> Availability;
    /// Start serving `launch` (its port is chosen by the caller).
    fn start<'a>(&'a self, launch: &'a Launch) -> BoxFuture<'a, Result<Running, EngineError>>;
}

/// The metadata saved next to an installed build.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    /// Build id (`metal`, `cpu`, ...).
    pub id: String,
    /// llama.cpp tag.
    pub tag: String,
    /// Self-test result.
    pub self_test: String,
    /// Message from the self-test.
    pub message: Option<String>,
    /// Devices it lists.
    pub devices: Vec<String>,
    /// Size on disk.
    pub size: u64,
}

fn server_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

/// Where llama.cpp's automatic installation stands (see [`LlamaCpp::ready`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Setup {
    /// Nothing is being installed.
    #[default]
    Idle,
    /// The suggested build is being installed.
    Installing,
    /// The install failed; the message says what to do.
    Failed(String),
}

/// Starts the automatic install (set by the application once it exists; tests with a fake engine never set it).
pub type Kick = std::sync::Arc<dyn Fn() + Send + Sync>;

/// Installs llama.cpp builds: the seam the automatic install is tested through (no network in tests).
pub trait BuildInstaller: Send + Sync {
    /// The build suggested for this machine.
    fn suggested(&self) -> Option<String>;
    /// Download, verify, self-test and activate build `id`, reporting `(done, total)` bytes.
    fn install<'a>(
        &'a self,
        id: &'a str,
        cancel: &'a CancelFlag,
        progress: &'a mut (dyn FnMut(u64, u64) + Send),
    ) -> BoxFuture<'a, Result<BuildInfo, EngineError>>;
}

impl BuildInstaller for LlamaCpp {
    fn suggested(&self) -> Option<String> {
        LlamaCpp::suggested(self)
    }
    fn install<'a>(
        &'a self,
        id: &'a str,
        cancel: &'a CancelFlag,
        progress: &'a mut (dyn FnMut(u64, u64) + Send),
    ) -> BoxFuture<'a, Result<BuildInfo, EngineError>> {
        Box::pin(async move { LlamaCpp::install(self, id, cancel, progress).await })
    }
}

/// llama.cpp: install management and the [`Engine`] implementation.
#[derive(Clone)]
pub struct LlamaCpp {
    dirs: Dirs,
    egress: Egress,
    catalog: std::sync::Arc<RuntimeCatalog>,
    hardware: Hardware,
    choice: std::sync::Arc<std::sync::RwLock<BuildChoice>>,
    pins: std::sync::Arc<std::sync::RwLock<EnginePins>>,
    matrix: std::sync::Arc<Matrix>,
    /// The build each model was last started on (read with the loaded models, so a stale entry is harmless).
    started_on: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, BuildId>>>,
    /// The automatic install's state; models that need a build wait on it.
    setup: std::sync::Arc<tokio::sync::watch::Sender<Setup>>,
    /// Starts the automatic install.
    kick: std::sync::Arc<std::sync::RwLock<Option<Kick>>>,
}

impl LlamaCpp {
    /// The manager.
    pub fn new(dirs: Dirs, egress: Egress, hardware: Hardware, choice: BuildChoice) -> LlamaCpp {
        LlamaCpp {
            dirs,
            egress,
            catalog: std::sync::Arc::new(RuntimeCatalog::embedded()),
            hardware,
            choice: std::sync::Arc::new(std::sync::RwLock::new(choice)),
            pins: std::sync::Arc::new(std::sync::RwLock::new(EnginePins::default())),
            matrix: std::sync::Arc::new(Matrix::embedded()),
            started_on: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            setup: std::sync::Arc::new(tokio::sync::watch::channel(Setup::Idle).0),
            kick: std::sync::Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Where the automatic install stands.
    pub fn setup(&self) -> Setup {
        self.setup.borrow().clone()
    }

    /// Records the automatic install's state and wakes everything waiting for a build.
    pub fn set_setup(&self, s: Setup) {
        self.setup.send_replace(s);
    }

    /// Lets the engine start its own installation when a model needs a build and none is installed.
    pub fn set_kick(&self, k: Kick) {
        if let Ok(mut g) = self.kick.write() {
            *g = Some(k);
        }
    }

    /// Starts the automatic install now if a build is missing and nothing is running (no-op without a kick).
    pub fn install_if_missing(&self) {
        if !matches!(self.binary_for(None), Err(EngineError::NotInstalled)) || self.setup() == Setup::Installing {
            return;
        }
        let kick = self.kick.read().ok().and_then(|g| g.clone());
        if let Some(k) = kick {
            k();
        }
    }

    /// The capability matrix (ADR 0018).
    pub fn matrix(&self) -> &Matrix {
        &self.matrix
    }

    /// The pins now.
    pub fn pins(&self) -> EnginePins {
        self.pins.read().map(|p| p.clone()).unwrap_or_default()
    }

    /// Replaces the pins (after they were validated and saved).
    pub fn set_pins(&self, p: EnginePins) {
        if let Ok(mut g) = self.pins.write() {
            *g = p;
        }
    }

    /// The installed builds as selection sees them.
    pub fn facts(&self) -> Vec<Installed> {
        self.installed()
            .iter()
            .filter_map(|b| {
                Some(Installed {
                    id: BuildId::parse(&b.id).ok()?,
                    tag: b.tag.clone(),
                    passed: b.self_test == "passed",
                })
            })
            .collect()
    }

    /// The build that serves `model` (`None`: models in general) and why: model pin, global pin, then automatic.
    pub fn select_for(&self, model: Option<&str>) -> Result<Selection, PinError> {
        pins::select(&self.pins(), model, &self.facts(), &self.choice())
    }

    /// The matrix facts of the build that serves `model`.
    pub fn build_facts(&self, model: Option<&str>) -> Result<BuildFacts, PinError> {
        let sel = self.select_for(model)?;
        let tag = self
            .facts()
            .into_iter()
            .find(|i| i.id == sel.build)
            .map(|i| i.tag)
            .unwrap_or_default();
        Ok(BuildFacts::new(sel.build.as_str(), &tag))
    }

    /// Records the build a model was started on (done by `start`; also how tests stand in for a running engine).
    pub fn note_started(&self, model: &str, build: BuildId) {
        if let Ok(mut g) = self.started_on.lock() {
            g.insert(model.to_string(), build);
        }
    }

    /// The models that run now, with the build each runs on (those whose engine this is).
    pub fn running_on(&self, loaded: &[String]) -> Vec<(String, BuildId)> {
        let map = self.started_on.lock().map(|g| g.clone()).unwrap_or_default();
        loaded.iter().filter_map(|m| map.get(m).map(|b| (m.clone(), b.clone()))).collect()
    }

    /// The pinned catalog.
    pub fn catalog(&self) -> &RuntimeCatalog {
        &self.catalog
    }

    /// The platform key (`macos-arm64`).
    pub fn platform(&self) -> String {
        runtimes::current_platform()
    }

    /// The build suggested for this hardware.
    pub fn suggested(&self) -> Option<String> {
        runtimes::suggest(&self.platform(), &self.hardware, &self.catalog)
    }

    /// Change which build is preferred.
    pub fn set_choice(&self, c: BuildChoice) {
        if let Ok(mut g) = self.choice.write() {
            *g = c;
        }
    }

    /// The preferred build (`inference_build`).
    pub fn choice(&self) -> BuildChoice {
        self.choice.read().map(|c| c.clone()).unwrap_or(BuildChoice::Auto)
    }

    fn build_dir(&self, id: &str) -> PathBuf {
        self.dirs.engines().join(id)
    }

    /// Installed builds that passed their self-test, with their info.
    pub fn installed(&self) -> Vec<BuildInfo> {
        let Ok(rd) = std::fs::read_dir(self.dirs.engines()) else {
            return vec![];
        };
        let mut v: Vec<BuildInfo> = rd
            .filter_map(Result::ok)
            .filter_map(|e| std::fs::read_to_string(e.path().join("build.json")).ok())
            .filter_map(|t| serde_json::from_str::<BuildInfo>(&t).ok())
            .filter(|b| builds::is_build_id(&b.id))
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    /// The build that will serve models in general (the global pin, else the automatic choice), if any.
    pub fn active(&self) -> Option<BuildInfo> {
        let sel = self.select_for(None).ok()?;
        self.installed().into_iter().find(|b| b.id == sel.build.as_str())
    }

    /// Path of the `llama-server` that serves `model` (`None`: models in general), and the build it belongs to.
    pub fn binary_for(&self, model: Option<&str>) -> Result<(PathBuf, BuildId), EngineError> {
        let sel = self.select_for(model).map_err(|e| match e {
            PinError::NoGoodBuild => EngineError::NotInstalled,
            e => EngineError::Pin(e),
        })?;
        let tag = self
            .facts()
            .into_iter()
            .find(|i| i.id == sel.build)
            .map(|i| i.tag)
            .unwrap_or_default();
        if let Some(m) = builds::too_old_message(&tag) {
            return Err(EngineError::TooOld(m));
        }
        let p = self.build_dir(sel.build.as_str()).join(server_name());
        if p.is_file() {
            Ok((p, sel.build))
        } else {
            Err(EngineError::NotInstalled)
        }
    }

    /// Path of the active `llama-server`.
    pub fn binary(&self) -> Option<PathBuf> {
        self.binary_for(None).ok().map(|(p, _)| p)
    }

    /// Remove an installed build.
    pub fn remove(&self, id: &str) -> std::io::Result<()> {
        if !builds::is_build_id(id) {
            return Err(std::io::Error::other("not a build id"));
        }
        std::fs::remove_dir_all(self.build_dir(id))
    }

    /// Download, verify, unpack, self-test and activate build `id`. The previous install of the same id stays
    /// untouched until the new one passes (roll back = keep it).
    pub async fn install(
        &self,
        id: &str,
        cancel: &CancelFlag,
        mut progress: impl FnMut(u64, u64) + Send,
    ) -> Result<BuildInfo, EngineError> {
        let platform = self.platform();
        let build: RuntimeBuild = self
            .catalog
            .find(&platform, id)
            .cloned()
            .ok_or_else(|| EngineError::NotAvailable(format!("there is no build \"{id}\" for {platform}")))?;
        if let Some(m) = builds::too_old_message(&self.catalog.tag) {
            return Err(EngineError::TooOld(m));
        }
        let total = build.download_bytes();
        let stage = self.dirs.engines().join(format!("{id}.new"));
        let _ = tokio::fs::remove_dir_all(&stage).await;
        tokio::fs::create_dir_all(&stage)
            .await
            .map_err(|e| EngineError::Install(e.to_string()))?;
        let mut base = 0u64;
        for asset in &build.assets {
            let url = DownloadUrl::parse(&asset.url).map_err(|e| EngineError::Install(e.to_string()))?;
            let part = self.dirs.partial().join(&asset.name);
            let b = base;
            let r = self
                .egress
                .download(&url, &part, Some(&asset.sha256), cancel, |d, _| progress(b + d, total))
                .await;
            if let Err(e) = r {
                let _ = tokio::fs::remove_dir_all(&stage).await;
                return Err(e.into());
            }
            base += asset.size;
            let (p2, s2, name) = (part.clone(), stage.clone(), asset.name.clone());
            let res = tokio::task::spawn_blocking(move || unpack(&p2, &s2, &name))
                .await
                .map_err(|e| EngineError::Install(e.to_string()))?;
            let _ = tokio::fs::remove_file(&part).await;
            if let Err(e) = res {
                let _ = tokio::fs::remove_dir_all(&stage).await;
                return Err(e);
            }
        }
        let bin = stage.join(server_name());
        if !bin.is_file() {
            let _ = tokio::fs::remove_dir_all(&stage).await;
            return Err(EngineError::Install("the archive does not contain llama-server".into()));
        }
        make_executable(&bin);
        let test = self_test(&bin).await;
        let (self_test_s, message, devices) = match &test {
            Ok(d) => ("passed", None, d.clone()),
            Err(m) => ("failed", Some(m.clone()), vec![]),
        };
        let size = dir_size(&stage);
        let info = BuildInfo {
            id: id.into(),
            tag: self.catalog.tag.clone(),
            self_test: self_test_s.into(),
            message,
            devices,
            size,
        };
        let _ = std::fs::write(stage.join("build.json"), serde_json::to_vec_pretty(&info).unwrap_or_default());
        if let Err(m) = test {
            let _ = tokio::fs::remove_dir_all(&stage).await;
            return Err(EngineError::Install(format!(
                "the new engine failed its self-test and was discarded ({m}); the previous build stays active"
            )));
        }
        let live = self.build_dir(id);
        let old = self.dirs.engines().join(format!("{id}.old"));
        let _ = tokio::fs::remove_dir_all(&old).await;
        if live.exists() {
            tokio::fs::rename(&live, &old)
                .await
                .map_err(|e| EngineError::Install(e.to_string()))?;
        }
        if let Err(e) = tokio::fs::rename(&stage, &live).await {
            let _ = tokio::fs::rename(&old, &live).await;
            return Err(EngineError::Install(e.to_string()));
        }
        let _ = tokio::fs::remove_dir_all(&old).await;
        Ok(info)
    }
}

fn dir_size(p: &Path) -> u64 {
    std::fs::read_dir(p)
        .map(|rd| rd.filter_map(Result::ok).filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

pub(crate) fn make_executable(p: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = p;
}

/// Run the engine once: it must print its version, and `--list-devices` tells what it can use.
pub async fn self_test(bin: &Path) -> Result<Vec<String>, String> {
    let dir = bin.parent().map(Path::to_path_buf);
    let run = |arg: &'static str| {
        let mut c = process::command(bin, dir.as_deref());
        c.arg(arg);
        async move { tokio::time::timeout(Duration::from_secs(30), c.output()).await }
    };
    let v = run("--version")
        .await
        .map_err(|_| "it did not answer within 30 seconds".to_string())?
        .map_err(|e| format!("it could not start: {e}"))?;
    if !v.status.success() {
        if let Some(problem) = exit_problem(v.status.code(), &in_system32) {
            return Err(problem.to_string());
        }
        let tail: String = String::from_utf8_lossy(&v.stderr)
            .chars()
            .rev()
            .take(300)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return Err(format!("--version failed: {}", tail.trim()));
    }
    let devices = match run("--list-devices").await {
        Ok(Ok(o)) => {
            let text = format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
            runtimes::parse_list_devices(&text).iter().map(|d| d.label()).collect()
        }
        _ => vec![],
    };
    Ok(devices)
}

/// Unpack `archive` (a `.zip` or `.tar.gz`) into `dest`, keeping only the files Pitwall expects, flat, with
/// bounded sizes. Symlinks are recreated only when their target is a plain sibling name.
pub fn unpack(archive: &Path, dest: &Path, name: &str) -> Result<(), EngineError> {
    unpack_with(archive, dest, name, &runtimes::is_expected)
}

/// [`unpack`] keeping the entries `keep` accepts (by flat file name).
pub fn unpack_with(archive: &Path, dest: &Path, name: &str, keep: &dyn Fn(&str) -> bool) -> Result<(), EngineError> {
    const MAX_FILE: u64 = 3 * 1024 * 1024 * 1024;
    const MAX_ENTRIES: usize = 2000;
    let bad = |e: &dyn std::fmt::Display| EngineError::Install(format!("the engine archive is damaged: {e}"));
    if name.ends_with(".zip") {
        let f = std::fs::File::open(archive).map_err(|e| bad(&e))?;
        let mut z = zip::ZipArchive::new(f).map_err(|e| bad(&e))?;
        if z.len() > MAX_ENTRIES {
            return Err(bad(&"too many entries"));
        }
        for i in 0..z.len() {
            let mut e = z.by_index(i).map_err(|e| bad(&e))?;
            if e.is_dir() {
                continue;
            }
            let Ok(zname) = e.name() else { continue };
            let Some(flat) = runtimes::flat_name(&zname).filter(|n| keep(n)).map(str::to_string) else {
                continue;
            };
            if e.size() > MAX_FILE {
                return Err(bad(&"an entry is too large"));
            }
            let mut out = std::fs::File::create(dest.join(&flat)).map_err(|e| bad(&e))?;
            std::io::copy(&mut std::io::Read::take(&mut e, MAX_FILE), &mut out).map_err(|e| bad(&e))?;
        }
    } else {
        let f = std::fs::File::open(archive).map_err(|e| bad(&e))?;
        let mut t = tar::Archive::new(flate2::read::GzDecoder::new(f));
        let mut links: Vec<(String, String)> = vec![];
        for (n, e) in t.entries().map_err(|e| bad(&e))?.enumerate() {
            if n > MAX_ENTRIES {
                return Err(bad(&"too many entries"));
            }
            let mut e = e.map_err(|e| bad(&e))?;
            let path = e.path().map_err(|e| bad(&e))?.to_string_lossy().into_owned();
            let Some(flat) = runtimes::flat_name(&path).filter(|n| keep(n)).map(str::to_string) else {
                continue;
            };
            let kind = e.header().entry_type();
            if kind.is_symlink() {
                if let Some(target) = e.link_name().ok().flatten().map(|t| t.to_string_lossy().into_owned()) {
                    if runtimes::safe_name(&target) {
                        links.push((flat, target));
                    }
                }
            } else if kind.is_file() {
                if e.size() > MAX_FILE {
                    return Err(bad(&"an entry is too large"));
                }
                let mut out = std::fs::File::create(dest.join(&flat)).map_err(|e| bad(&e))?;
                std::io::copy(&mut std::io::Read::take(&mut e, MAX_FILE), &mut out).map_err(|e| bad(&e))?;
            }
        }
        for (link, target) in links {
            let p = dest.join(&link);
            let _ = std::fs::remove_file(&p);
            #[cfg(unix)]
            if std::os::unix::fs::symlink(&target, &p).is_err() {
                return Err(bad(&"cannot create a library link"));
            }
            #[cfg(not(unix))]
            let _ = (p, target);
        }
    }
    Ok(())
}

impl Engine for LlamaCpp {
    fn kind(&self) -> EngineKind {
        EngineKind::LlamaCpp
    }

    fn build_tag(&self, model: &str) -> String {
        self.select_for(Some(model))
            .map(|s| s.build.as_str().to_string())
            .unwrap_or_default()
    }

    fn ready(&self) -> BoxFuture<'_, Result<(), EngineError>> {
        Box::pin(async move {
            let mut rx = self.setup.subscribe();
            loop {
                // Anything but "no build" is the start's own business (a pin, an old build): it explains itself.
                if !matches!(self.binary_for(None), Err(EngineError::NotInstalled)) {
                    return Ok(());
                }
                let state = rx.borrow_and_update().clone();
                match state {
                    Setup::Installing => {}
                    Setup::Failed(m) => return Err(EngineError::Install(m)),
                    Setup::Idle => {
                        self.install_if_missing();
                        if *rx.borrow() == Setup::Idle {
                            return Err(EngineError::NotInstalled);
                        }
                        continue;
                    }
                }
                // A finished install flips the state to Idle (or Failed); a lost sender cannot happen (we hold it).
                if rx.changed().await.is_err() {
                    return Err(EngineError::NotInstalled);
                }
            }
        })
    }

    fn availability(&self) -> Availability {
        if self.binary().is_some() {
            Availability::Available
        } else if let Err(e @ (EngineError::Pin(_) | EngineError::TooOld(_))) = self.binary_for(None) {
            Availability::NotInstalled { reason: e.to_string() }
        } else if self.catalog.builds(&self.platform()).is_empty() {
            Availability::NotAvailable {
                reason: format!("llama.cpp has no build for {}", self.platform()),
            }
        } else {
            Availability::NotInstalled {
                reason: "No llama.cpp build is installed yet. Pitwall installs the suggested one; you can also pick another below.".into(),
            }
        }
    }

    fn start<'a>(&'a self, launch: &'a Launch) -> BoxFuture<'a, Result<Running, EngineError>> {
        Box::pin(async move {
            let (bin, build) = self.binary_for(Some(&launch.alias))?;
            let port = LoopbackPort::new(launch.port).ok_or_else(|| EngineError::Start("bad port".into()))?;
            let key = paddock_secrets::random_token(16).map_err(|e| EngineError::Start(e.to_string()))?;
            let mut c = process::command(&bin, bin.parent());
            c.args(llama::args(launch))
                .env(llama::API_KEY_ENV, &key)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped());
            let mut child = c.spawn().map_err(|e| EngineError::Start(e.to_string()))?;
            let stderr = child.stderr.take();
            let tail = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
            if let Some(mut s) = stderr {
                let t = tail.clone();
                tokio::spawn(async move {
                    use tokio::io::AsyncReadExt;
                    let mut buf = [0u8; 4096];
                    while let Ok(n) = s.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        if let Ok(mut g) = t.lock() {
                            g.push_str(&String::from_utf8_lossy(&buf[..n]));
                            let len = g.len();
                            if len > 4000 {
                                let cut = g.char_indices().map(|(i, _)| i).find(|i| *i >= len - 4000).unwrap_or(0);
                                g.drain(..cut);
                            }
                        }
                    }
                });
            }
            let client = Client::new(port, Some(key));
            let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
            loop {
                if let Ok(Some(status)) = child.try_wait() {
                    if let Some(problem) = exit_problem(status.code(), &in_system32) {
                        return Err(problem);
                    }
                    let t = tail.lock().map(|g| g.clone()).unwrap_or_default();
                    let last: Vec<&str> = t.lines().rev().take(4).collect();
                    return Err(EngineError::Start(format!(
                        "the engine exited ({status}): {}",
                        last.into_iter().rev().collect::<Vec<_>>().join(" | ")
                    )));
                }
                if let Ok((200, _)) = client.json("GET", "/health", None, Duration::from_secs(2)).await {
                    break;
                }
                if tokio::time::Instant::now() > deadline {
                    return Err(EngineError::Start("the model did not become ready within 5 minutes".into()));
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            let props = match client.json("GET", "/props", None, Duration::from_secs(5)).await {
                Ok((200, v)) => llama::parse_props(&v),
                _ => llama::Props::default(),
            };
            self.note_started(&launch.alias, build);
            Ok(Running {
                child: Some(child),
                guard: None,
                client,
                port: launch.port,
                props,
            })
        })
    }
}

impl CapabilityGate for LlamaCpp {
    fn check(&self, model: &str, needed: &[Capability]) -> Result<(), ApiError> {
        let facts = match self.build_facts(Some(model)) {
            Ok(f) => f,
            // Nothing installed: the engine's own availability error explains it when the model loads.
            Err(PinError::NoGoodBuild) => return Ok(()),
            Err(e) => return Err(ApiError::new(ErrorKind::EnginePin, e.to_string())),
        };
        let label = self.build_label(&facts);
        match self.matrix.gate(needed, &facts).into_error(&self.matrix, model, &label) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn blocked(&self, model: &str, caps: &[Capability]) -> Vec<Blocked> {
        let Ok(facts) = self.build_facts(Some(model)) else {
            return vec![];
        };
        caps.iter()
            .filter_map(|&c| match self.matrix.gate(&[c], &facts) {
                Gate::Allowed => None,
                g => Some(Blocked {
                    capability: c,
                    reason: g
                        .clone()
                        .into_error(&self.matrix, model, &self.build_label(&facts))
                        .map(|e| e.message)
                        .unwrap_or_default(),
                    min_build: match g {
                        Gate::NeedsBuild { min_build, .. } => Some(min_build),
                        _ => None,
                    },
                }),
            })
            .collect()
    }
}

impl LlamaCpp {
    /// `vulkan b11669` for messages.
    fn build_label(&self, f: &BuildFacts) -> String {
        match f.number {
            Some(n) => format!("{} b{n}", f.id),
            None => f.id.clone(),
        }
    }
}

pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The self-test state as the UI shows it.
pub fn self_test_state(b: &BuildInfo) -> SelfTest {
    match b.self_test.as_str() {
        "passed" => SelfTest::Passed,
        "failed" => SelfTest::Failed,
        _ => SelfTest::NotRun,
    }
}

/// The build choice from the setting string.
pub fn parse_choice(s: &str) -> BuildChoice {
    BuildChoice::parse(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn dirs() -> (tempfile::TempDir, Dirs) {
        let d = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(d.path());
        (d, dirs)
    }

    fn mgr(dirs: &Dirs) -> LlamaCpp {
        LlamaCpp::new(dirs.clone(), Egress::unreachable(), Hardware::default(), BuildChoice::Auto)
    }

    #[test]
    fn unzips_only_expected_flat_files_and_rejects_escapes() {
        let (d, _) = dirs();
        let zp = d.path().join("a.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&zp).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for (n, c) in [
            ("llama-server.exe", "x"),
            ("sub/ggml.dll", "y"),
            ("../evil.dll", "z"),
            ("llama-bench.exe", "b"),
            ("deep/a/b.dll", "q"),
        ] {
            z.start_file(n, o).unwrap();
            z.write_all(c.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        let out = d.path().join("out");
        std::fs::create_dir(&out).unwrap();
        unpack(&zp, &out, "a.zip").unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&out)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["ggml.dll", "llama-server.exe"]);
        assert!(!d.path().join("evil.dll").exists());
    }

    #[cfg(unix)]
    #[test]
    fn untars_with_library_links() {
        let (d, _) = dirs();
        let tp = d.path().join("a.tar.gz");
        let gz = flate2::write::GzEncoder::new(std::fs::File::create(&tp).unwrap(), flate2::Compression::fast());
        let mut t = tar::Builder::new(gz);
        let mut h = tar::Header::new_gnu();
        h.set_size(1);
        h.set_mode(0o755);
        h.set_cksum();
        t.append_data(&mut h, "llama-b1/llama-server", &b"x"[..]).unwrap();
        let mut h = tar::Header::new_gnu();
        h.set_size(1);
        h.set_cksum();
        t.append_data(&mut h, "llama-b1/libllama.0.dylib", &b"y"[..]).unwrap();
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_size(0);
        t.append_link(&mut h, "llama-b1/libllama.dylib", "libllama.0.dylib").unwrap();
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_size(0);
        t.append_link(&mut h, "llama-b1/libevil.dylib", "../../etc/passwd").unwrap();
        t.into_inner().unwrap().finish().unwrap();
        let out = d.path().join("out");
        std::fs::create_dir(&out).unwrap();
        unpack(&tp, &out, "a.tar.gz").unwrap();
        assert!(out.join("llama-server").is_file());
        assert_eq!(
            std::fs::read_link(out.join("libllama.dylib")).unwrap().to_string_lossy(),
            "libllama.0.dylib"
        );
        assert!(!out.join("libevil.dylib").exists());
    }

    #[test]
    fn damaged_archive_is_an_error() {
        let (d, _) = dirs();
        let p = d.path().join("bad.zip");
        std::fs::write(&p, b"nope").unwrap();
        assert!(unpack(&p, d.path(), "bad.zip").is_err());
        assert!(unpack(&p, d.path(), "bad.tar.gz").is_err());
    }

    #[test]
    fn an_engine_that_exits_with_status_dll_not_found_is_explained_as_a_missing_vc_runtime() {
        // Windows reports the 32-bit status; Rust shows it as an i32. Nothing of the runtime is in System32.
        let e = exit_problem(Some(STATUS_DLL_NOT_FOUND as i32), &|_| false).expect("0xC0000135 is explained");
        assert!(matches!(e, EngineError::MissingRuntime(_)));
        let m = e.to_string();
        for needed in [
            "Visual C++",
            "vcruntime140.dll",
            "vcruntime140_1.dll",
            "msvcp140.dll",
            "0xC0000135",
            "vc_redist",
            "reinstall Pitwall",
        ] {
            assert!(m.contains(needed), "the message must contain {needed:?}: {m}");
        }
        // Other exits stay with their own words.
        assert!(exit_problem(Some(1), &|_| false).is_none());
        assert!(exit_problem(Some(0), &|_| false).is_none());
        assert!(exit_problem(None, &|_| false).is_none());
        // The same status as the unsigned value cast the other way.
        assert!(exit_problem(Some(-1_073_741_515), &|_| false).is_some());
    }

    #[test]
    fn the_runtime_is_blamed_only_for_the_files_that_are_missing_and_an_installed_runtime_points_at_the_engine() {
        // Only vcruntime140_1.dll is absent: the words name it, and not the two that are there.
        let e = exit_problem(Some(STATUS_DLL_NOT_FOUND as i32), &|f| f != "vcruntime140_1.dll").unwrap();
        let m = e.to_string();
        assert!(
            matches!(e, EngineError::MissingRuntime(ref v) if v == &["vcruntime140_1.dll"]),
            "{m}"
        );
        assert!(m.contains("vcruntime140_1.dll missing") && !m.contains("msvcp140.dll"), "{m}");
        // The whole runtime is there: a DLL of the engine itself is missing (quarantined, for example).
        let e = exit_problem(Some(STATUS_DLL_NOT_FOUND as i32), &|_| true).unwrap();
        assert!(matches!(e, EngineError::EngineDllMissing));
        let m = e.to_string();
        assert!(
            m.contains("a DLL it needs is missing") && m.contains("quarantine") && m.contains("reinstall the engine"),
            "{m}"
        );
        assert!(!m.contains("no Microsoft Visual C++ runtime"), "must not blame the runtime: {m}");
    }

    #[tokio::test]
    async fn availability_and_failed_install() {
        let (_d, dirs) = dirs();
        let m = mgr(&dirs);
        assert!(m.installed().is_empty() && m.binary().is_none());
        assert!(matches!(
            m.availability(),
            Availability::NotInstalled { .. } | Availability::NotAvailable { .. }
        ));
        let id = m.suggested().unwrap_or_else(|| "cpu".into());
        let err = m.install(&id, &CancelFlag::default(), |_, _| {}).await;
        assert!(err.is_err());
        assert!(
            !dirs.engines().join(format!("{id}.new")).exists(),
            "a failed install leaves nothing behind"
        );
        let launch = Launch {
            model: "m".into(),
            mmproj: None,
            alias: "m".into(),
            port: 4100,
            context: 1024,
            gpu_layers: None,
            threads: 0,
            parallel: 1,
            kind: llama::ServeKind::Chat,
            cache_ram_mib: 0,
            slot_save_path: None,
        };
        assert!(matches!(m.start(&launch).await, Err(EngineError::NotInstalled)));
        assert!(m.remove("../x").is_err());
    }

    #[tokio::test]
    async fn self_test_failure_and_rollback_metadata() {
        let (d, dirs) = dirs();
        let m = mgr(&dirs);
        let b = dirs.engines().join("cpu");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(
            b.join("build.json"),
            serde_json::to_vec(&BuildInfo {
                id: "cpu".into(),
                tag: "t".into(),
                self_test: "passed".into(),
                message: None,
                devices: vec![],
                size: 1,
            })
            .unwrap(),
        )
        .unwrap();
        std::fs::write(b.join(server_name()), b"").unwrap();
        assert_eq!(m.active().unwrap().id, "cpu");
        assert!(m.binary().is_some());
        assert_eq!(self_test_state(&m.active().unwrap()), SelfTest::Passed);
        assert!(self_test(&d.path().join("missing")).await.is_err());
        m.remove("cpu").unwrap();
        assert!(m.active().is_none());
    }

    fn put(dirs: &Dirs, id: &str, tag: &str, self_test: &str, binary: bool) {
        let b = dirs.engines().join(id);
        std::fs::create_dir_all(&b).unwrap();
        let info = BuildInfo {
            id: id.into(),
            tag: tag.into(),
            self_test: self_test.into(),
            message: None,
            devices: vec![],
            size: 1,
        };
        std::fs::write(b.join("build.json"), serde_json::to_vec(&info).unwrap()).unwrap();
        if binary {
            std::fs::write(b.join(server_name()), b"").unwrap();
        }
    }

    #[test]
    fn the_binary_follows_the_model_pin_then_the_global_pin_and_pins_are_strict() {
        let (_d, dirs) = dirs();
        let m = mgr(&dirs);
        put(&dirs, "cpu", "b11669", "passed", true);
        put(&dirs, "vulkan", "b11669", "passed", true);
        let facts = m.facts();
        let ok = |p: &mut EnginePins, model: Option<&str>, build: &str| {
            p.set_model(model.unwrap_or("x"), BuildId::parse(build).ok(), &facts).unwrap();
        };
        let (_, auto) = m.binary_for(Some("m")).unwrap();
        assert_eq!(auto.as_str(), "vulkan");
        let mut p = EnginePins::default();
        p.set_global(Some(BuildId::parse("cpu").unwrap()), &facts).unwrap();
        ok(&mut p, Some("m"), "vulkan");
        m.set_pins(p);
        assert_eq!(m.binary_for(Some("m")).unwrap().1.as_str(), "vulkan");
        assert_eq!(m.binary_for(Some("other")).unwrap().1.as_str(), "cpu");
        assert_eq!(m.binary_for(None).unwrap().1.as_str(), "cpu");
        assert_eq!(m.active().unwrap().id, "cpu");
        // The pinned build is removed: a typed error that says what to do, and the engine says it is not usable.
        m.remove("cpu").unwrap();
        let e = m.binary_for(Some("other")).unwrap_err();
        assert!(
            matches!(e, EngineError::Pin(PinError::NotInstalled { .. })) && e.to_string().contains("Install it on the llama.cpp page"),
            "{e}"
        );
        assert!(m.binary().is_none() && m.active().is_none());
        assert!(matches!(m.availability(), Availability::NotInstalled { reason } if reason.contains("Install it on the llama.cpp page")));
        // A pin does not apply to a model that has its own.
        assert_eq!(m.binary_for(Some("m")).unwrap().1.as_str(), "vulkan");
    }

    #[tokio::test]
    async fn ready_is_immediate_when_a_build_is_installed_and_never_installs() {
        let (_d, dirs) = dirs();
        let m = mgr(&dirs);
        put(&dirs, "cpu", "b11669", "passed", true);
        let kicked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let k = kicked.clone();
        m.set_kick(std::sync::Arc::new(move || {
            k.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
        assert!(m.ready().await.is_ok());
        assert_eq!(kicked.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn ready_without_a_build_starts_the_install_and_waits_for_it() {
        let (_d, dirs) = dirs();
        let m = mgr(&dirs);
        let (m2, dirs2) = (m.clone(), dirs.clone());
        m.set_kick(std::sync::Arc::new({
            let m = m.clone();
            move || {
                m.set_setup(Setup::Installing);
                let (m2, dirs2) = (m2.clone(), dirs2.clone());
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    put(&dirs2, "cpu", "b11669", "passed", true);
                    m2.set_setup(Setup::Idle);
                });
            }
        }));
        assert_eq!(m.setup(), Setup::Idle);
        m.ready().await.unwrap();
        assert!(m.binary().is_some());
    }

    #[tokio::test]
    async fn ready_reports_a_failed_install_and_without_any_way_to_install() {
        let (_d, dirs) = dirs();
        let m = mgr(&dirs);
        // No kick (nothing can install): the plain "not installed" error.
        assert!(matches!(m.ready().await, Err(EngineError::NotInstalled)));
        m.set_setup(Setup::Failed("the network is unreachable. Press Retry.".into()));
        let e = m.ready().await.unwrap_err();
        assert!(e.to_string().contains("Retry"), "{e}");
    }

    #[tokio::test]
    async fn with_no_build_installed_the_message_does_not_point_at_the_page_that_shows_it() {
        let (_d, dirs) = dirs();
        let m = mgr(&dirs);
        let Availability::NotInstalled { reason } = m.availability() else {
            panic!("expected NotInstalled");
        };
        assert!(!reason.contains("on the llama.cpp page"), "{reason}");
        assert!(reason.contains("suggested"), "{reason}");
    }

    #[tokio::test]
    async fn a_build_older_than_b5501_is_refused_at_start_with_a_message_that_says_what_to_do() {
        let (_d, dirs) = dirs();
        let m = mgr(&dirs);
        put(&dirs, "cpu", "b5500", "passed", true);
        let e = m.binary_for(None).unwrap_err();
        assert!(matches!(e, EngineError::TooOld(_)));
        assert_eq!(
            e.to_string(),
            "This llama.cpp build (b5500) is older than b5501, which Pitwall needs. Update it on the llama.cpp page."
        );
        assert!(m.binary().is_none());
        assert!(matches!(m.availability(), Availability::NotInstalled { reason } if reason.contains("b5500")));
        put(&dirs, "cpu", "b5501", "passed", true);
        assert!(m.binary_for(None).is_ok());
    }

    #[tokio::test]
    async fn installing_a_catalog_older_than_b5501_is_refused_before_downloading() {
        let (_d, dirs) = dirs();
        let mut m = mgr(&dirs);
        let mut c = (*m.catalog).clone();
        c.tag = "b5000".into();
        m.catalog = std::sync::Arc::new(c);
        let id = m.suggested().unwrap_or_else(|| "cpu".into());
        let e = m.install(&id, &CancelFlag::default(), |_, _| {}).await.unwrap_err();
        assert!(matches!(e, EngineError::TooOld(_)), "{e}");
        assert!(!dirs.engines().join(format!("{id}.new")).exists());
    }

    #[test]
    fn a_build_whose_server_file_is_missing_is_not_installed() {
        let (_d, dirs) = dirs();
        let m = mgr(&dirs);
        assert!(matches!(m.binary_for(None), Err(EngineError::NotInstalled)));
        put(&dirs, "cpu", "b11669", "passed", false);
        assert!(matches!(m.binary_for(None), Err(EngineError::NotInstalled)));
        // A build that did not pass its self-test is not chosen and not pinnable.
        put(&dirs, "vulkan", "b11669", "failed", true);
        assert_eq!(m.facts().len(), 2);
        assert!(m.select_for(None).is_ok_and(|s| s.build.as_str() == "cpu"));
        assert!(m.build_facts(Some("m")).is_ok_and(|f| f.number == Some(11669)));
    }

    #[test]
    fn the_gate_names_what_is_missing_and_stays_quiet_without_builds() {
        let (_d, dirs) = dirs();
        let m = mgr(&dirs);
        let tools = [Capability::Tools, Capability::Vision];
        assert!(m.check("m", &tools).is_ok(), "no builds: the load explains that");
        assert!(m.blocked("m", &tools).is_empty());
        put(&dirs, "cpu", "b4000", "passed", true);
        let e = m.check("m", &tools).unwrap_err();
        assert!(e.message.contains("b5331") && e.message.contains("cpu b4000"), "{}", e.message);
        let b = m.blocked("m", &[Capability::Chat, Capability::Tools, Capability::Vision]);
        assert_eq!(
            b.iter().map(|x| (x.capability, x.min_build)).collect::<Vec<_>>(),
            vec![(Capability::Tools, Some(4599)), (Capability::Vision, Some(5331))]
        );
        // A pin to something that is not installed is a typed pin error, not a capability error.
        let mut p = EnginePins::default();
        p.set_global(Some(BuildId::parse("cpu").unwrap()), &m.facts()).unwrap();
        m.set_pins(p);
        m.remove("cpu").unwrap();
        assert_eq!(m.check("m", &tools).unwrap_err().kind, ErrorKind::EnginePin);
        assert!(m.blocked("m", &tools).is_empty());
    }

    #[test]
    fn running_on_reports_only_models_it_started() {
        let (_d, dirs) = dirs();
        let m = mgr(&dirs);
        m.note_started("a", BuildId::parse("cpu").unwrap());
        let r = m.running_on(&["a".to_string(), "b".to_string()]);
        assert_eq!(r, vec![("a".to_string(), BuildId::parse("cpu").unwrap())]);
    }
}
