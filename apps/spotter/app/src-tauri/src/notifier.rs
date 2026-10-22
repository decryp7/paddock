//! What the engine's effects become on this machine. [`ShellNotifier`] is the engine's `Notifier`: it shows
//! notifications through [`OsToast`] (the OS, or a fake in tests) and runs a watch's actions on a new release (open its
//! location, download it into a folder with checksum verification, or run a program the user chose). Both hand the
//! work to another task and return at once, as the engine's contract asks.
//!
//! E-mail channels are not delivered yet (the engine leaves them to the shell); see `docs/roadmap.md`.

use spotter_core::action::{command_args, Action};
use spotter_engine::notifier::{ActionRequest, Notification, Notifier};
use spotter_engine::Engine;
use spotter_sources::http::Http;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// How long a program started by an action may run.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);

/// Shows a notification on the OS. Contract: returns quickly, never panics.
pub trait OsToast: Send + Sync {
    /// Shows `title` and `body`.
    fn show(&self, title: &str, body: &str);
}

/// Opens a release's location with the system's handler. Contract: returns an error message the user can act on.
pub trait OpenLocation: Send + Sync {
    /// Opens a URL or a path.
    fn open(&self, location: &str) -> Result<(), String>;
}

struct Inner {
    toast: Arc<dyn OsToast>,
    opener: Arc<dyn OpenLocation>,
    engine: OnceLock<Engine>,
    http: Http,
    default_folder: PathBuf,
}

/// The engine's `Notifier` for the desktop app. Cheap to clone; clones share everything.
#[derive(Clone)]
pub struct ShellNotifier {
    inner: Arc<Inner>,
}

impl ShellNotifier {
    /// A notifier. Its engine is attached with [`ShellNotifier::attach`] once the engine exists (the engine needs the
    /// notifier first).
    pub fn new(toast: Arc<dyn OsToast>, opener: Arc<dyn OpenLocation>, http: Http, default_folder: PathBuf) -> ShellNotifier {
        ShellNotifier {
            inner: Arc::new(Inner {
                toast,
                opener,
                engine: OnceLock::new(),
                http,
                default_folder,
            }),
        }
    }

    /// Gives the notifier the engine whose journal records the outcomes of actions.
    pub fn attach(&self, engine: Engine) {
        let _ = self.inner.engine.set(engine);
    }

    /// The folder a download action uses: its own, else the user's downloads folder.
    pub fn folder_for(&self, configured: &str) -> PathBuf {
        let c = configured.trim();
        if c.is_empty() {
            self.inner.default_folder.clone()
        } else {
            PathBuf::from(c)
        }
    }

    /// Runs the actions of one request in order. A failed action is recorded and the next one still runs.
    pub async fn run(&self, request: &ActionRequest) {
        let mut downloaded: Option<String> = None;
        for action in &request.actions {
            let failure = match action {
                Action::OpenLocation { variant, extension } => self
                    .inner
                    .opener
                    .open(&request.release.with_artifact(variant, extension).location)
                    .err()
                    .map(|e| ("open-location", e)),
                Action::Download {
                    folder,
                    verify,
                    variant,
                    extension,
                } => {
                    let release = request.release.with_artifact(variant, extension);
                    self.download(request, &release, &self.folder_for(folder), *verify, &mut downloaded)
                        .await
                        .err()
                        .map(|e| ("download", e))
                }
                Action::RunCommand { program, args } => {
                    let path = downloaded.clone().unwrap_or_else(|| request.release.location.clone());
                    run_command(program, &command_args(args, &request.release, &path))
                        .await
                        .err()
                        .map(|e| ("run-command", e))
                }
            };
            if let Some((name, message)) = failure {
                self.report_failure(request, name, &message);
            }
        }
    }

    async fn download(
        &self,
        request: &ActionRequest,
        release: &spotter_core::release::Release,
        folder: &Path,
        verify: bool,
        downloaded: &mut Option<String>,
    ) -> Result<(), String> {
        let result = spotter_sources::download::download(&self.inner.http, release, folder, verify).await;
        if let Some(engine) = self.inner.engine.get() {
            let outcome = result
                .as_ref()
                .map(|d| (d.path.display().to_string(), d.size, d.verified))
                .map_err(Clone::clone);
            // The journal entry is the record; a failure to write it is not worth failing the action for.
            let _ = engine.record_download(&request.watch_id, &release.version.text, outcome);
        }
        result.map(|d| *downloaded = Some(d.path.display().to_string()))
    }

    fn report_failure(&self, request: &ActionRequest, action: &str, message: &str) {
        crate::applog::write(&format!("action {action} for watch {} failed", request.watch_id));
        // A failed download is already in the journal (record_download); other actions are recorded here.
        if action != "download" {
            if let Some(engine) = self.inner.engine.get() {
                let _ = engine.record_action_failure(&request.watch_id, action, message);
            }
        }
        self.inner.toast.show(&format!("{}: {action} failed", request.watch_name), message);
    }
}

impl Notifier for ShellNotifier {
    fn notify(&self, notification: &Notification) {
        self.inner.toast.show(&notification.title, &notification.body);
    }

    fn run_actions(&self, request: &ActionRequest) {
        // Actions can take minutes, so they get a task of their own and the engine is not held up.
        let (request, this) = (request.clone(), self.clone());
        tauri::async_runtime::spawn(async move { this.run(&request).await });
    }
}

/// Starts `program` with `args` (never through a shell), waits at most [`COMMAND_TIMEOUT`] and kills it after that.
pub async fn run_command(program: &str, args: &[String]) -> Result<(), String> {
    run_command_for(program, args, COMMAND_TIMEOUT).await
}

/// [`run_command`] with another time limit.
pub async fn run_command_for(program: &str, args: &[String], limit: Duration) -> Result<(), String> {
    if !Path::new(program).is_absolute() {
        return Err("The program to run needs its full path.".into());
    }
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn().map_err(|e| format!("The program could not be started: {e}"))?;
    match tokio::time::timeout(limit, child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(format!("The program ended with {status}.")),
        Ok(Err(e)) => Err(format!("The program could not be waited for: {e}")),
        Err(_) => {
            let _ = child.kill().await;
            Err(format!("The program was stopped after {} seconds.", limit.as_secs()))
        }
    }
}

/// A release's location is something the OS may open: a web address or an absolute path.
pub fn openable(location: &str) -> bool {
    let l = location;
    !l.trim().is_empty()
        && !l.chars().any(char::is_control)
        && (l.starts_with("https://") || l.starts_with("http://") || Path::new(l).is_absolute() || l.starts_with(r"\\"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use spotter_core::release::Release;
    use spotter_core::version::Version;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Toasts(Mutex<Vec<(String, String)>>);
    impl OsToast for Toasts {
        fn show(&self, title: &str, body: &str) {
            self.0.lock().unwrap().push((title.into(), body.into()));
        }
    }

    #[derive(Default)]
    struct Opened {
        seen: Mutex<Vec<String>>,
        fail: bool,
    }
    impl OpenLocation for Opened {
        fn open(&self, location: &str) -> Result<(), String> {
            self.seen.lock().unwrap().push(location.into());
            if self.fail {
                Err("no handler".into())
            } else {
                Ok(())
            }
        }
    }

    fn release(location: &str) -> Release {
        Release {
            version: Version::new("1.2.0", vec![1, 2, 0], ""),
            name: "App".into(),
            location: location.into(),
            published_at: None,
            size: None,
            sha256: None,
            detail: Default::default(),
        }
    }

    fn notifier(opened: Arc<Opened>, toasts: Arc<Toasts>) -> ShellNotifier {
        ShellNotifier::new(toasts, opened, Http::new("test").unwrap(), PathBuf::from("/nonexistent-default"))
    }

    fn request(actions: Vec<Action>, location: &str) -> ActionRequest {
        ActionRequest {
            watch_id: "w1".into(),
            watch_name: "App".into(),
            release: release(location),
            actions,
        }
    }

    #[test]
    fn notifications_become_toasts() {
        let toasts = Arc::new(Toasts::default());
        let n = notifier(Arc::new(Opened::default()), toasts.clone());
        n.notify(&Notification {
            kind: spotter_engine::notifier::NotificationKind::NewRelease,
            watch_id: Some("w1".into()),
            title: "App 1.2.0".into(),
            body: "A new release".into(),
            release: None,
            channels: Vec::new(),
        });
        assert_eq!(
            toasts.0.lock().unwrap().as_slice(),
            [("App 1.2.0".to_string(), "A new release".to_string())]
        );
    }

    #[test]
    fn the_download_folder_falls_back_to_the_default() {
        let n = notifier(Arc::new(Opened::default()), Arc::new(Toasts::default()));
        assert_eq!(n.folder_for("  "), PathBuf::from("/nonexistent-default"));
        assert_eq!(n.folder_for("/data/in"), PathBuf::from("/data/in"));
    }

    #[tokio::test]
    async fn a_failed_action_is_toasted_and_does_not_stop_the_next() {
        let opened = Arc::new(Opened {
            fail: true,
            ..Opened::default()
        });
        let toasts = Arc::new(Toasts::default());
        let n = notifier(opened.clone(), toasts.clone());
        n.run(&request(
            vec![Action::open_location(), Action::open_location()],
            "https://example.org/a",
        ))
        .await;
        assert_eq!(opened.seen.lock().unwrap().len(), 2, "the second action still ran");
        let t = toasts.0.lock().unwrap();
        assert_eq!(t.len(), 2);
        assert!(t[0].0.contains("open-location failed") && t[0].1.contains("no handler"));
    }

    #[tokio::test]
    async fn a_download_that_cannot_start_is_reported() {
        let toasts = Arc::new(Toasts::default());
        let n = notifier(Arc::new(Opened::default()), toasts.clone());
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().display().to_string();
        n.run(&request(vec![Action::download(&folder, true)], "not-a-location")).await;
        assert!(toasts.0.lock().unwrap()[0].0.contains("download failed"));
    }

    #[tokio::test]
    async fn a_relative_program_is_refused_and_a_missing_one_fails_cleanly() {
        assert!(run_command("echo", &[]).await.unwrap_err().contains("full path"));
        let missing = if cfg!(windows) { r"C:\nonexistent\x.exe" } else { "/nonexistent/x" };
        assert!(run_command(missing, &[]).await.unwrap_err().contains("could not be started"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn programs_run_without_a_shell_and_are_stopped_at_the_limit() {
        assert!(run_command("/bin/sh", &["-c".into(), "exit 0".into()]).await.is_ok());
        assert!(run_command("/bin/sh", &["-c".into(), "exit 3".into()])
            .await
            .unwrap_err()
            .contains("ended with"));
        let slow = run_command_for("/bin/sh", &["-c".into(), "sleep 30".into()], Duration::from_millis(100)).await;
        assert!(slow.unwrap_err().contains("stopped after"));
    }

    #[test]
    fn only_urls_and_absolute_paths_are_openable() {
        assert!(openable("https://example.org/a"));
        assert!(openable("/srv/drops/app.msi") || cfg!(windows));
        assert!(openable(r"\\server\drops"));
        assert!(!openable("javascript:alert(1)"));
        assert!(!openable("relative/path"));
        assert!(!openable("https://x\n"));
        assert!(!openable(""));
    }
}
