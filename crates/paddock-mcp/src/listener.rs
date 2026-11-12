//! The app-side loopback listener (ADR 0007 rule 1): [`Listener::reconcile`] makes the listener match the user's choice.
//! It serves on 127.0.0.1 (an ephemeral port, so a second instance or another program never clashes) only while AI access
//! is on and at least one unrevoked key exists, and writes a discovery file ([`Discovery`]: pid, start time, port, never a
//! key). The file is only a hint (rule 53): a client checks that the process named in it is the one that started when the
//! file says before it sends a key anywhere ([`crate::bridge`]).

use crate::keys::KeyStore;
use crate::loopback::{Serving, serve_loopback};
use crate::server::Server;
use paddock_proc::hint::{self, Hint};
use paddock_proc::{ProcessId, StartTime};
use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

/// The file name of the discovery file in the app's config folder.
pub const DISCOVERY_FILE: &str = "mcp-listener.json";

/// Where the discovery file of an app with config folder `config_dir` lives.
pub fn discovery_path(config_dir: &Path) -> PathBuf {
    config_dir.join(DISCOVERY_FILE)
}

/// What the discovery file says. No key is ever in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovery {
    /// The serving process.
    pub pid: u32,
    /// When it started, in microseconds since the Unix epoch (as the OS reports it).
    pub started_us: u64,
    /// The loopback port.
    pub port: u16,
    /// The app's name.
    pub app: String,
    /// The app's version.
    pub version: String,
}

impl Discovery {
    /// The file's content, `None` when missing or not one.
    pub fn read(path: &Path) -> Option<Discovery> {
        serde_json::from_slice(&hint::read_bounded(path)?).ok()
    }

    /// Whether the process that wrote it still runs (same pid and same start time, and not one that was killed and is only
    /// waiting to be collected by its parent).
    pub fn writer_alive(&self) -> bool {
        ProcessId::new(self.pid, StartTime::from_micros(self.started_us)).is_alive()
    }
}

impl Hint for Discovery {
    fn pid(&self) -> u32 {
        self.pid
    }
    fn started_ms(&self) -> i64 {
        i64::try_from(self.started_us / 1000).unwrap_or(i64::MAX)
    }
}

/// What the settings page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ListenerView {
    /// This app has no listener.
    Unavailable,
    /// AI access is off: nothing listens.
    Off,
    /// AI access is on but no unrevoked key exists: nothing listens until one is issued.
    NoKey,
    /// Listening on 127.0.0.1.
    #[serde(rename_all = "camelCase")]
    Listening {
        /// The loopback port.
        port: u16,
        /// The discovery file a client reads to find the port.
        discovery_file: String,
    },
    /// The listener could not start; `message` says why and what to do.
    Failed {
        /// The cause and the next step.
        message: String,
    },
}

struct Active {
    serving: Serving,
    hint: Discovery,
}

/// Keeps the listener in line with the settings.
pub struct Listener {
    server: Arc<Server>,
    keys: Arc<KeyStore>,
    file: PathBuf,
    app: String,
    version: String,
    max_connections: usize,
    state: Mutex<(Option<Active>, ListenerView)>,
}

impl Listener {
    /// A listener for `server`, pairing with `keys`, writing its hint to `file`, serving at most `max_connections` clients.
    pub fn new(
        server: Arc<Server>,
        keys: Arc<KeyStore>,
        file: PathBuf,
        max_connections: usize,
    ) -> Listener {
        let info = server.server_info();
        Listener {
            server,
            keys,
            file,
            app: info.0,
            version: info.1,
            max_connections,
            state: Mutex::new((None, ListenerView::Off)),
        }
    }

    /// What the page shows.
    pub fn view(&self) -> ListenerView {
        self.lock().1.clone()
    }

    /// Where it listens now.
    pub fn addr(&self) -> Option<SocketAddr> {
        self.lock().0.as_ref().map(|a| a.serving.addr())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, (Option<Active>, ListenerView)> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts or stops the listener so that it serves exactly when `enabled` and an unrevoked key exists. Quick (a bind or a
    /// join of threads that notice a stop within 100 ms); the shell calls it from its blocking pool, never the UI thread.
    pub fn reconcile(&self, enabled: bool) {
        let want = enabled && self.keys.has_active();
        let mut st = self.lock();
        if !want {
            if let Some(active) = st.0.take() {
                let began = Instant::now();
                active.serving.stop();
                hint::remove_if_same(&self.file, &active.hint, Discovery::read);
                log::info!(
                    "paddock_mcp: listener on port {} stopped in {} ms ({})",
                    active.hint.port,
                    began.elapsed().as_millis(),
                    if enabled {
                        "no unrevoked key"
                    } else {
                        "AI access is off"
                    }
                );
            }
            st.1 = if enabled {
                ListenerView::NoKey
            } else {
                ListenerView::Off
            };
            return;
        }
        if st.0.is_some() {
            return;
        }
        match self.start() {
            Ok(active) => {
                st.1 = ListenerView::Listening {
                    port: active.hint.port,
                    discovery_file: self.file.display().to_string(),
                };
                st.0 = Some(active);
            }
            Err(message) => {
                log::error!("paddock_mcp: the listener did not start: {message}");
                st.1 = ListenerView::Failed { message };
            }
        }
    }

    fn start(&self) -> Result<Active, String> {
        let began = Instant::now();
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|e| format!("Could not open a local port ({e}). Turn AI access off and on again, or restart the app."))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("Could not read the local port ({e}).",))?
            .port();
        let me = ProcessId::of(std::process::id()).ok_or_else(|| {
            "Could not read this program's start time, so clients could not verify it.".to_string()
        })?;
        let hint_value = Discovery {
            pid: me.pid(),
            started_us: me.started().as_micros(),
            port,
            app: self.app.clone(),
            version: self.version.clone(),
        };
        let body = serde_json::to_vec(&hint_value).map_err(|e| e.to_string())?;
        let serving = serve_loopback(
            self.server.clone(),
            self.keys.clone(),
            listener,
            self.max_connections,
        )
        .map_err(|e| format!("Could not start serving ({e})."))?;
        if let Err(e) = hint::write_private_atomic(&self.file, &body) {
            serving.stop();
            return Err(format!(
                "Could not write {} ({e}). Check that the folder is writable.",
                self.file.display()
            ));
        }
        log::info!(
            "paddock_mcp: listening on 127.0.0.1:{port} (pid {}), discovery file {}, started in {} ms",
            me.pid(),
            self.file.display(),
            began.elapsed().as_millis()
        );
        Ok(Active {
            serving,
            hint: hint_value,
        })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.reconcile(false);
    }
}
