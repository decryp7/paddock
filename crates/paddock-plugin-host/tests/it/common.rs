//! A rig for the host's tests: a temp folder, a fake clock, an in-memory or persistent quarantine, the fixture packed and
//! installed, and supervisors over it. Nothing touches the user's machine.

use paddock_guard::{Quarantine, QuarantinePolicy};
use paddock_ipc::FrameLimit;
use paddock_jobs::time::{Clock, FakeClock};
use paddock_plugin_api::compat::{HostInfo, NamespaceSupport};
use paddock_plugin_host::services::TaskObserver;
use paddock_plugin_host::{Grants, HostConfig, Installed, PluginsDir, Supervisor};
use paddock_plugin_package::{SecretKey, TrustPolicy, TrustSettings};
use paddock_plugin_test::fixture;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct Rig {
    pub dir: tempfile::TempDir,
    pub clock: FakeClock,
    pub key: SecretKey,
    pub trust: TrustPolicy,
    pub plugins: PluginsDir,
    pub cfg: Arc<HostConfig>,
    pub quarantine: Arc<Quarantine>,
    pub seen: Arc<Seen>,
}

#[derive(Default)]
pub struct Seen(pub Mutex<Vec<String>>);

impl TaskObserver for Seen {
    fn started(&self, plugin: &str, tool: &str, task: &paddock_plugin_api::tools::ToolTask) {
        self.0
            .lock()
            .unwrap()
            .push(format!("started {plugin} {tool} {}", task.id));
    }
    fn report(&self, plugin: &str, task: &str, progress: Option<f32>, note: &str) {
        self.0
            .lock()
            .unwrap()
            .push(format!("report {plugin} {task} {progress:?} {note}"));
    }
    fn finished(&self, plugin: &str, task: &str) {
        self.0
            .lock()
            .unwrap()
            .push(format!("finished {plugin} {task}"));
    }
}

pub fn host_info() -> HostInfo {
    HostInfo::new("TestApp").serving(NamespaceSupport::new("paddock.fixture.v1", 1).since(1, "1.0"))
}

impl Rig {
    pub fn new() -> Rig {
        Rig::with(|_| {})
    }

    pub fn with(tweak: impl FnOnce(&mut HostConfig)) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let clock = FakeClock::new();
        let key = SecretKey::generate().unwrap();
        let trust = TrustPolicy::new(
            &[("Test publisher", &key.public().to_text())],
            TrustSettings::default(),
        )
        .unwrap();
        let shared: Arc<dyn Clock> = Arc::new(clock.clone());
        let (quarantine, problem) = Quarantine::persistent(
            QuarantinePolicy::default(),
            shared.clone(),
            dir.path().join("quarantine.json"),
        );
        assert!(problem.is_none());
        let quarantine = Arc::new(quarantine);
        let seen = Arc::new(Seen::default());
        let mut cfg = HostConfig::new(
            host_info(),
            shared.clone(),
            quarantine.clone(),
            dir.path().join("outputs"),
        );
        cfg.tasks = seen.clone();
        // Plugins that are ended by force must not leave half-written coverage data behind.
        cfg.child_env = vec![(
            "LLVM_PROFILE_FILE".into(),
            if cfg!(windows) { "NUL" } else { "/dev/null" }.into(),
        )];
        cfg.max_frame = FrameLimit::new(512 * 1024);
        tweak(&mut cfg);
        Rig {
            plugins: PluginsDir::new(dir.path().join("plugins"), shared),
            dir,
            clock,
            key,
            trust,
            cfg: Arc::new(cfg),
            quarantine,
            seen,
        }
    }

    /// The fixture program copied to a file of its own (tests change and delete it).
    pub fn program(&self, name: &str) -> PathBuf {
        let source = if name.ends_with("-v1") {
            fixture::locate_v1()
        } else if name.starts_with("misbehaving") {
            fixture::locate_misbehaving()
        } else {
            fixture::locate()
        }
        .unwrap();
        let copy = self.dir.path().join(format!("src-{name}"));
        std::fs::copy(source, &copy).unwrap();
        copy
    }

    /// Packs a manifest and a program into a signed package.
    pub fn package(&self, manifest: paddock_plugin_package::Manifest, program: &Path) -> PathBuf {
        let out = self.dir.path().join("pkgs");
        std::fs::create_dir_all(&out).unwrap();
        fixture::build_package(&out, manifest, program, Some(&self.key)).unwrap()
    }

    /// Installs the fixture with `permissions` asked for and `grants` given.
    pub fn install(&self, permissions: &str, grants: Grants) -> Installed {
        let program = self.program("paddock-plugin-fixture");
        self.install_manifest(
            fixture::manifest("2.0", "paddock-plugin-fixture", permissions),
            &program,
            grants,
        )
    }

    /// Packs `manifest` with `program` and installs it.
    pub fn install_manifest(
        &self,
        manifest: paddock_plugin_package::Manifest,
        program: &Path,
        grants: Grants,
    ) -> Installed {
        let pkg = self.package(manifest, program);
        let inspection = self
            .plugins
            .inspect(&pkg, &self.trust, &self.cfg.info)
            .unwrap();
        inspection.installable().unwrap();
        self.plugins
            .install(
                &pkg,
                &self.trust,
                &self.cfg.info,
                &inspection.digest,
                grants,
            )
            .unwrap()
    }

    pub fn supervisor(&self, permissions: &str, grants: Grants) -> Supervisor {
        Supervisor::new(self.cfg.clone(), self.install(permissions, grants))
    }
}

pub const READ_DOCS: &str =
    "[[permissions]]\npermission = \"readFolders\"\ngrants = [\"docs\"]\nreason = \"reads docs\"\n";
pub const ELEVATE: &str =
    "[[permissions]]\npermission = \"elevated\"\nops = [\"collect\"]\nreason = \"collects\"\n";
