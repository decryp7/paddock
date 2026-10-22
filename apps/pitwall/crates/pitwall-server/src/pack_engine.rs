//! The generic engine-pack adapter (ADR 0011): runs any local engine that serves an OpenAI-compatible API, as
//! declared by a validated [`EnginePack`]. It installs the pinned, checksummed download for this platform, fills the
//! pack's argument template (the engine always binds 127.0.0.1 on a port Pitwall chose), waits for the health
//! endpoint and hands back a loopback client.

use crate::egress::{CancelFlag, Egress};
use crate::engines::{hex, make_executable, unpack_with, BoxFuture, Engine, EngineError, Running};
use crate::paths::Dirs;
use crate::process;
use pitwall_core::llama::Launch;
use pitwall_core::mgmt::{Availability, EngineKind};
use pitwall_core::packs::{self, Archive, EnginePack};
use pitwall_core::runtimes;
use pitwall_loopback::{Client, LoopbackPort};
use std::path::PathBuf;
use std::time::Duration;

/// One engine pack as an [`Engine`].
#[derive(Clone)]
pub struct PackEngine {
    /// The validated pack.
    pub pack: EnginePack,
    dirs: Dirs,
    egress: Egress,
    platform: String,
}

impl PackEngine {
    /// An adapter for `pack` on this platform.
    pub fn new(pack: EnginePack, dirs: Dirs, egress: Egress) -> PackEngine {
        PackEngine {
            pack,
            dirs,
            egress,
            platform: runtimes::current_platform(),
        }
    }

    fn dir(&self) -> PathBuf {
        self.dirs.engines().join(format!("pack-{}", self.pack.id))
    }

    /// A pack with no download runs an engine the user installed themselves (`mlx_lm.server`, a vLLM environment).
    fn external(&self) -> bool {
        self.pack.downloads.is_empty()
    }

    fn external_found(&self) -> bool {
        let cmd = self.pack.command.split(' ').next().unwrap_or_default();
        if cmd.contains(['/', '\\']) {
            return std::path::Path::new(cmd).is_file();
        }
        std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file() || d.join(format!("{cmd}.exe")).is_file()))
    }

    fn download(&self) -> Option<&packs::Download> {
        self.pack.downloads.iter().find(|d| d.platform == self.platform)
    }

    /// The installed executable, when present and installed at the pinned version.
    pub fn binary(&self) -> Option<PathBuf> {
        let d = self.download()?;
        let ok = std::fs::read_to_string(self.dir().join("version"))
            .ok()
            .is_some_and(|v| v.trim() == self.pack.version);
        let p = self.dir().join(&d.binary);
        (ok && p.is_file()).then_some(p)
    }

    /// Download, verify and unpack the pinned build.
    pub async fn install(&self, cancel: &CancelFlag, mut progress: impl FnMut(u64, u64) + Send) -> Result<(), EngineError> {
        let d = self
            .download()
            .ok_or_else(|| EngineError::NotAvailable(format!("the {} pack has no download for {}", self.pack.name, self.platform)))?;
        let part = self.dirs.partial().join(format!("pack-{}-{}", self.pack.id, self.pack.version));
        let size = d.size;
        self.egress
            .download(&d.url, &part, Some(&d.sha256), cancel, |done, _| progress(done, size))
            .await?;
        let stage = self.dirs.engines().join(format!("pack-{}.new", self.pack.id));
        let _ = std::fs::remove_dir_all(&stage);
        std::fs::create_dir_all(&stage).map_err(|e| EngineError::Install(e.to_string()))?;
        let binary = d.binary.clone();
        let res = match d.archive {
            Archive::None => std::fs::rename(&part, stage.join(&binary)).map_err(|e| EngineError::Install(e.to_string())),
            Archive::Zip | Archive::TarGz => {
                let name = if d.archive == Archive::Zip { "a.zip" } else { "a.tar.gz" };
                let r = unpack_with(&part, &stage, name, &|n| runtimes::safe_name(n));
                let _ = std::fs::remove_file(&part);
                r
            }
        };
        let finish = res.and_then(|()| {
            let bin = stage.join(&binary);
            if !bin.is_file() {
                return Err(EngineError::Install(format!("the download does not contain {binary}")));
            }
            make_executable(&bin);
            std::fs::write(stage.join("version"), &self.pack.version).map_err(|e| EngineError::Install(e.to_string()))?;
            let live = self.dir();
            let _ = std::fs::remove_dir_all(&live);
            std::fs::rename(&stage, &live).map_err(|e| EngineError::Install(e.to_string()))
        });
        if finish.is_err() {
            let _ = std::fs::remove_dir_all(&stage);
        }
        finish
    }

    /// Remove the installed build.
    pub fn remove(&self) -> std::io::Result<()> {
        std::fs::remove_dir_all(self.dir())
    }

    /// The command and arguments for `launch`, from the pack's templates.
    pub fn command_line(&self, l: &Launch, binary_name: &str, engine_dir: &str) -> Result<(String, Vec<String>), String> {
        let v: Vec<(&str, String)> = vec![
            ("host", "127.0.0.1".into()),
            ("port", l.port.to_string()),
            ("model", l.model.clone()),
            ("mmproj", l.mmproj.clone().unwrap_or_default()),
            // `Launch::context` is the total of all slots; a pack's `{context}` is what one request may use.
            ("context", (l.context / l.parallel.max(1)).to_string()),
            ("slots", l.parallel.max(1).to_string()),
            ("alias", l.alias.clone()),
            ("threads", l.threads.max(1).to_string()),
            ("engine_dir", engine_dir.to_string()),
            ("binary", binary_name.to_string()),
        ];
        let cmd = packs::fill(&self.pack.command, &v)?;
        let args = self.pack.args.iter().map(|a| packs::fill(a, &v)).collect::<Result<Vec<_>, _>>()?;
        Ok((cmd, args))
    }
}

impl Engine for PackEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Pack
    }

    fn id(&self) -> String {
        self.pack.id.clone()
    }

    fn availability(&self) -> Availability {
        if self.external() {
            return if self.external_found() {
                Availability::Available
            } else {
                Availability::NotInstalled {
                    reason: format!(
                        "{} is not installed on this machine: install it yourself so \"{}\" can be found",
                        self.pack.name, self.pack.command
                    ),
                }
            };
        }
        if self.download().is_none() && self.binary().is_none() {
            Availability::NotAvailable {
                reason: format!("the {} pack has no build for {}", self.pack.name, self.platform),
            }
        } else if self.binary().is_some() {
            Availability::Available
        } else {
            Availability::NotInstalled {
                reason: format!("Install {} {} in Engines", self.pack.name, self.pack.version),
            }
        }
    }

    fn start<'a>(&'a self, l: &'a Launch) -> BoxFuture<'a, Result<Running, EngineError>> {
        Box::pin(async move {
            let (dir, name) = if self.external() {
                if !self.external_found() {
                    return Err(EngineError::NotAvailable(format!(
                        "{} is not installed on this machine",
                        self.pack.name
                    )));
                }
                (std::env::temp_dir(), String::new())
            } else {
                let bin = self
                    .binary()
                    .ok_or_else(|| EngineError::NotAvailable(format!("{} is not installed. Install it in Engines.", self.pack.name)))?;
                (
                    self.dir(),
                    bin.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                )
            };
            let (cmd, args) = self.command_line(l, &name, &dir.to_string_lossy()).map_err(EngineError::Start)?;
            let port = LoopbackPort::new(l.port).ok_or_else(|| EngineError::Start("bad port".into()))?;
            let key = self.pack.api_key_env.as_ref().map(|_| hex(&crate::paths::random32()[..16]));
            let mut c = process::command(&cmd, Some(&dir));
            c.args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            if let (Some(env), Some(k)) = (&self.pack.api_key_env, &key) {
                c.env(env, k);
            }
            let mut child = c.spawn().map_err(|e| EngineError::Start(format!("{cmd}: {e}")))?;
            let client = Client::new(port, key);
            let deadline = tokio::time::Instant::now() + Duration::from_secs(self.pack.start_timeout_secs);
            loop {
                if let Ok(Some(status)) = child.try_wait() {
                    return Err(EngineError::Start(format!("{} exited during start ({status})", self.pack.name)));
                }
                if let Ok((200, _)) = client.json("GET", &self.pack.health_path, None, Duration::from_secs(2)).await {
                    break;
                }
                if tokio::time::Instant::now() > deadline {
                    return Err(EngineError::Start(format!(
                        "{} did not become healthy within {} seconds",
                        self.pack.name, self.pack.start_timeout_secs
                    )));
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Ok(Running {
                child: Some(child),
                guard: None,
                client,
                port: l.port,
                props: pitwall_core::llama::Props {
                    n_ctx: Some(l.context / l.parallel.max(1)),
                    vision: Some(self.pack.caps.vision && l.mmproj.is_some()),
                    ..Default::default()
                },
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitwall_core::llama::ServeKind;
    use pitwall_core::packs::{parse_engine, Problem};

    fn pack(extra: &str) -> EnginePack {
        let text = format!(
            "format = 1\nid = \"p\"\nname = \"P\"\nversion = \"1\"\n[capabilities]\nchat = true\n[launch]\ncommand = \"{{engine_dir}}/{{binary}}\"\nargs = [\"--host\", \"{{host}}\", \"--port\", \"{{port}}\", \"-m\", \"{{model}}\", \"-c\", \"{{context}}\"]\n[health]\npath = \"/health\"\n{extra}"
        );
        let mut w: Vec<Problem> = vec![];
        parse_engine(&text, "p.engine.toml", &mut w).unwrap()
    }

    fn engine(extra: &str) -> (tempfile::TempDir, PackEngine) {
        let d = tempfile::tempdir().unwrap();
        let e = PackEngine::new(
            pack(extra),
            Dirs::at(d.path()),
            Egress::new(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true))),
        );
        (d, e)
    }

    fn launch() -> Launch {
        Launch {
            model: "/m/x.gguf".into(),
            mmproj: None,
            alias: "x".into(),
            port: 5555,
            context: 2048,
            gpu_layers: None,
            threads: 0,
            parallel: 1,
            kind: ServeKind::Chat,
            cache_ram_mib: 0,
            slot_save_path: None,
        }
    }

    #[test]
    fn the_command_line_binds_loopback_on_pitwalls_port() {
        let (_d, e) = engine("");
        let (cmd, args) = e.command_line(&launch(), "bin", "/eng").unwrap();
        assert_eq!(cmd, "/eng/bin");
        assert_eq!(args, ["--host", "127.0.0.1", "--port", "5555", "-m", "/m/x.gguf", "-c", "2048"]);
    }

    #[test]
    fn a_packs_context_is_per_slot() {
        let (_d, e) = engine("");
        let four = Launch {
            context: 8192,
            parallel: 4,
            ..launch()
        };
        let (_, args) = e.command_line(&four, "bin", "/eng").unwrap();
        assert_eq!(args.last().map(String::as_str), Some("2048"), "the total of 8192 over 4 slots");
    }

    #[tokio::test]
    async fn an_uninstalled_pack_says_how_to_install_and_will_not_start() {
        let platform = runtimes::current_platform();
        let dl = format!(
            "[[download]]\nplatform = \"{platform}\"\nurl = \"https://github.com/x/y/releases/download/1/p\"\nsha256 = \"{}\"\nsize = 5\narchive = \"none\"\nbinary = \"p\"\n",
            "0".repeat(64)
        );
        let (d, e) = engine(&dl);
        assert!(matches!(e.availability(), Availability::NotInstalled { .. }));
        assert!(e.start(&launch()).await.is_err());
        // Offline: the install refuses before connecting and leaves nothing behind.
        assert!(e.install(&CancelFlag::default(), |_, _| {}).await.is_err());
        assert!(!d.path().join("engines/pack-p.new").exists());
        // Installed at the pinned version it is available; another version is not.
        let dir = d.path().join("engines/pack-p");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("p"), b"x").unwrap();
        std::fs::write(dir.join("version"), "0").unwrap();
        assert!(e.binary().is_none());
        std::fs::write(dir.join("version"), "1").unwrap();
        assert!(e.binary().is_some() && matches!(e.availability(), Availability::Available));
        e.remove().unwrap();
        assert!(e.binary().is_none());
        assert_eq!((e.kind(), e.id().as_str()), (EngineKind::Pack, "p"));
    }

    #[tokio::test]
    async fn an_external_pack_is_available_only_when_its_command_exists() {
        let (_d, ext) = engine("");
        // External engines (no downloads): available only when the command can be found.
        assert!(matches!(ext.availability(), Availability::NotInstalled { .. }));
        assert!(matches!(ext.start(&launch()).await, Err(EngineError::NotAvailable(_))));
    }
}
