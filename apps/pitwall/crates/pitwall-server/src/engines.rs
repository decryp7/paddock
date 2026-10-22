//! Engines (ADR 0004, ADR 0010): the [`Engine`] trait the server serves models through, and llama.cpp's install
//! management. The install flow (pinned catalog, SHA-256 checked before unpacking, defensive unpacking, self-test,
//! roll back on failure) is adapted from Debrief's `imt-ai` runtime manager.

use crate::egress::{CancelFlag, DownloadError, Egress};
use crate::paths::Dirs;
use crate::process;
use pitwall_core::hardware::Hardware;
use pitwall_core::llama::{self, Launch};
use pitwall_core::mgmt::{Availability, EngineKind, SelfTest};
use pitwall_core::privacy::DownloadUrl;
use pitwall_core::runtimes::{self, builds, BuildChoice, RuntimeBuild, RuntimeCatalog};
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
    #[error("no llama.cpp engine is installed. Open Engines and install one (it is suggested for this machine).")]
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
    /// Whether it can run here, and why not.
    fn availability(&self) -> Availability;
    /// Start serving `launch` (its port is chosen by the caller).
    fn start<'a>(&'a self, launch: &'a Launch) -> BoxFuture<'a, Result<Running, EngineError>>;
}

/// Engines planned but not built yet: they report why, so the UI can say so.
pub struct Planned {
    /// The kind.
    pub kind: EngineKind,
    /// Why it is unavailable.
    pub reason: String,
}

impl Engine for Planned {
    fn kind(&self) -> EngineKind {
        self.kind
    }
    fn availability(&self) -> Availability {
        Availability::NotAvailable {
            reason: self.reason.clone(),
        }
    }
    fn start<'a>(&'a self, _: &'a Launch) -> BoxFuture<'a, Result<Running, EngineError>> {
        Box::pin(async move { Err(EngineError::NotAvailable(self.reason.clone())) })
    }
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

/// llama.cpp: install management and the [`Engine`] implementation.
#[derive(Clone)]
pub struct LlamaCpp {
    dirs: Dirs,
    egress: Egress,
    catalog: std::sync::Arc<RuntimeCatalog>,
    hardware: Hardware,
    choice: std::sync::Arc<std::sync::RwLock<BuildChoice>>,
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
        }
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

    fn choice(&self) -> BuildChoice {
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

    /// The build that will serve: the first of the try order that passed its self-test.
    pub fn active(&self) -> Option<BuildInfo> {
        let ok: Vec<BuildInfo> = self.installed().into_iter().filter(|b| b.self_test == "passed").collect();
        let ids: Vec<String> = ok.iter().map(|b| b.id.clone()).collect();
        builds::try_order(&ids, &self.choice())
            .into_iter()
            .next()
            .and_then(|id| ok.into_iter().find(|b| b.id == id))
    }

    /// Path of the active `llama-server`.
    pub fn binary(&self) -> Option<PathBuf> {
        let b = self.active()?;
        let p = self.build_dir(&b.id).join(server_name());
        p.is_file().then_some(p)
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
            let Some(flat) = runtimes::flat_name(&zname)
                .filter(|n| keep(n))
                .map(str::to_string)
            else {
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

    fn availability(&self) -> Availability {
        if self.binary().is_some() {
            Availability::Available
        } else if self.catalog.builds(&self.platform()).is_empty() {
            Availability::NotAvailable {
                reason: format!("llama.cpp has no build for {}", self.platform()),
            }
        } else {
            Availability::NotInstalled {
                reason: "Install a llama.cpp build in Engines".into(),
            }
        }
    }

    fn start<'a>(&'a self, launch: &'a Launch) -> BoxFuture<'a, Result<Running, EngineError>> {
        Box::pin(async move {
            let bin = self.binary().ok_or(EngineError::NotInstalled)?;
            let port = LoopbackPort::new(launch.port).ok_or_else(|| EngineError::Start("bad port".into()))?;
            let key = hex(&crate::paths::random32()[..16]);
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
        LlamaCpp::new(
            dirs.clone(),
            Egress::new(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true))),
            Hardware::default(),
            BuildChoice::Auto,
        )
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

    #[tokio::test]
    async fn availability_and_offline_install() {
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
        };
        assert!(matches!(m.start(&launch).await, Err(EngineError::NotInstalled)));
        assert!(m.remove("../x").is_err());
    }

    #[tokio::test]
    async fn planned_engines_say_why() {
        let p = Planned {
            kind: EngineKind::Mlx,
            reason: "MLX is planned".into(),
        };
        assert!(matches!(p.availability(), Availability::NotAvailable { .. }));
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
        };
        assert!(p.start(&launch).await.is_err());
        assert_eq!(p.kind(), EngineKind::Mlx);
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
}
