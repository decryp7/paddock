//! The demo's background agent (plan X4): a small program next to the app (`shell-demo-agent`) that the app registers
//! as the user's launch agent / systemd user unit / `Run` entry through `paddock-service`, so the shell's Background
//! service page, the health entry with its Repair, the Start at login switch and the update smoke (catalogue C21:
//! install build N, then N+1, and the agent runs N+1, never EX_CONFIG) have something real to act on.
//!
//! The agent only writes a heartbeat (`<data>/agent/heartbeat.json`: version, pid, time) every few seconds; the app
//! reads it to know whether the agent answers. `stop` leaves a stop file the running agent sees within one beat; a
//! supervised agent also exits when its executable changed (the update) and the OS starts the new one.

use paddock_env::AppDirs;
use paddock_service::backend::{Side, in_app_bundle};
use paddock_service::{Manager, ProcessType, ServiceSpec, Verb, render};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The launchd label (and the bundled plist's file name without `.plist`).
pub const LABEL: &str = "net.decryptology.paddock-shell-demo.agent";
/// The agent's executable name.
pub const PROGRAM: &str = "shell-demo-agent";
/// The health entry of the agent.
pub const HEALTH_ID: &str = "agent";
/// The variable the OS sets when it started the agent.
pub const SUPERVISED_ENV: &str = "SHELL_DEMO_AGENT";
/// How often the agent beats.
pub const BEAT: Duration = Duration::from_secs(2);
/// A heartbeat older than this means the agent does not answer.
pub const STALE_AFTER: Duration = Duration::from_secs(15);

/// The agent's spec (every OS file is rendered from it).
pub fn spec() -> ServiceSpec {
    ServiceSpec::builder(LABEL, "PaddockShellDemo", PROGRAM)
        .unit("paddock-shell-demo-agent")
        .bundle_id("net.decryptology.paddock-shell-demo")
        .display_name("Paddock Shell Demo agent")
        .description(
            "The Paddock Shell Demo's sample background agent (writes a heartbeat, nothing else).",
        )
        .args(["run"])
        .supervised_env(SUPERVISED_ENV)
        .process_type(ProcessType::Background)
        .low_priority()
        .docs_url("https://dev.decryptology.net/decryp7/paddock")
        .build()
        .unwrap_or_else(|e| unreachable!("the demo's own spec is valid: {e}"))
}

/// What the agent writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    /// The agent's version.
    pub version: String,
    /// Its process id.
    pub pid: u32,
    /// Seconds since the Unix epoch.
    pub at: u64,
}

fn agent_dir(data: &Path) -> PathBuf {
    data.join("agent")
}

/// The heartbeat file.
pub fn heartbeat_path(data: &Path) -> PathBuf {
    agent_dir(data).join("heartbeat.json")
}

fn stop_path(data: &Path) -> PathBuf {
    agent_dir(data).join("stop")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The last heartbeat, if readable.
pub fn read_heartbeat(data: &Path) -> Option<Heartbeat> {
    let text = std::fs::read_to_string(heartbeat_path(data)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Whether the agent answers: a heartbeat at most [`STALE_AFTER`] old at `now` (seconds since the epoch).
pub fn answers_at(beat: Option<&Heartbeat>, now: u64) -> bool {
    beat.is_some_and(|b| now.saturating_sub(b.at) <= STALE_AFTER.as_secs())
}

/// Whether the agent answers now.
pub fn answers(data: &Path) -> bool {
    answers_at(read_heartbeat(data).as_ref(), now_secs())
}

/// Writes one heartbeat.
pub fn beat(data: &Path, version: &str) -> std::io::Result<()> {
    let b = Heartbeat {
        version: version.to_string(),
        pid: std::process::id(),
        at: now_secs(),
    };
    let text = serde_json::to_string(&b).map_err(std::io::Error::other)?;
    paddock_env::write_atomic(&heartbeat_path(data), text.as_bytes(), false)
        .map_err(std::io::Error::other)
}

/// Asks the running agent to stop (it sees the file within one beat).
pub fn request_stop(data: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(agent_dir(data))?;
    std::fs::write(stop_path(data), b"stop")
}

/// The agent's work: beat until asked to stop, or until `exe_changed` says the program was replaced (it then exits so
/// the OS starts the new one). `wait` is the pause between beats (a stop request ends it at once in the real loop).
pub fn run(
    data: &Path,
    version: &str,
    wait: &mut dyn FnMut(Duration),
    exe_changed: &mut dyn FnMut() -> bool,
) {
    let _ = std::fs::remove_file(stop_path(data));
    loop {
        if let Err(e) = beat(data, version) {
            log::warn!(error:% = e; "the heartbeat could not be written");
        }
        if stop_path(data).exists() {
            let _ = std::fs::remove_file(stop_path(data));
            log::info!("stop requested");
            return;
        }
        if exe_changed() {
            log::info!("the program was updated; exiting so the OS starts the new one");
            return;
        }
        wait(BEAT);
    }
}

/// The manager of the agent as seen from `side`.
pub fn manager(
    data: &Path,
    program: &Path,
    side: Side,
    version: &str,
) -> Result<Manager, paddock_service::ServiceError> {
    let backend = paddock_service::native::host_backend(&spec(), program, side, data)?;
    let beat_dir = data.to_path_buf();
    Ok(Manager::new(
        "Paddock Shell Demo",
        backend,
        data,
        LABEL,
        version,
        move || answers(&beat_dir),
    ))
}

/// The bundled launch agent the app ships (`src-tauri/macos/<label>.plist`, checked by the app's tests).
pub fn bundled_plist() -> String {
    render::launchd_plist(&spec(), render::LaunchdProgram::Bundled)
}

/// The NSIS hooks the app ships (`src-tauri/windows/installer-hooks.nsh`, checked by the app's tests).
pub fn nsis_hooks() -> String {
    render::nsis_hooks(&spec())
}

/// The agent's command line (`shell-demo-agent <verb>`): the shared verbs, plus `check` (the app's start-up check:
/// rule 24, used by the update smoke) and `run` (also the default). Returns the exit code.
pub fn main_with(args: &[String], dirs: &AppDirs, version: &str) -> i32 {
    let data = dirs.data();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(PROGRAM));
    // Inside the app bundle the agent registers as the app does (SMAppService); outside, as a plist file.
    let side = if in_app_bundle(&exe) {
        Side::App
    } else {
        Side::Program
    };
    let say = |r: Result<String, String>| match r {
        Ok(m) => {
            println!("{m}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    };
    let with_manager = |f: &mut dyn FnMut(&mut Manager) -> Result<String, String>| {
        manager(data, &exe, side, version)
            .map_err(|e| e.to_string())
            .and_then(|mut m| f(&mut m))
    };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    match args.first().map(String::as_str) {
        Some("check") => say(with_manager(&mut |m| {
            Ok(format!("{:?}", m.on_start(&cancel)))
        })),
        Some("run") | None => {
            let watch = paddock_service::watch::ExeWatch::new(
                exe.clone(),
                paddock_service::watch::is_supervised(SUPERVISED_ENV),
            );
            run(data, version, &mut |d| std::thread::sleep(d), &mut || {
                watch.check(|| false) == paddock_service::watch::RestartDecision::Restart
            });
            0
        }
        _ => match Verb::parse(args) {
            Some(Verb::Install) => say(with_manager(&mut |m| {
                m.install(&cancel)
                    .map(|o| format!("{o:?}"))
                    .map_err(|e| e.to_string())
            })),
            Some(Verb::Uninstall) => say(with_manager(&mut |m| {
                m.uninstall()
                    .map(|()| "removed".into())
                    .map_err(|e| e.to_string())
            })),
            Some(Verb::Status) => say(with_manager(&mut |m| {
                serde_json::to_string_pretty(&m.status()).map_err(|e| e.to_string())
            })),
            Some(Verb::Stop) => say(request_stop(data)
                .map(|()| "asked to stop".into())
                .map_err(|e| e.to_string())),
            Some(Verb::PrintPlist) => say(Ok(bundled_plist())),
            Some(Verb::PrintUnit) => say(with_manager(&mut |m| {
                m.definition()
                    .ok_or_else(|| "no unit on this system".into())
            })),
            Some(Verb::WindowsService) => say(Err(
                "the demo agent is a per-user agent, not a Windows service".into(),
            )),
            None => say(Err(format!(
                "unknown command; use run, check, {}",
                Verb::ALL.map(Verb::word).join(", ")
            ))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heartbeat_says_whether_the_agent_answers() {
        let d = tempfile::tempdir().unwrap();
        assert!(!answers(d.path()));
        beat(d.path(), "1.2.3").unwrap();
        let b = read_heartbeat(d.path()).unwrap();
        assert_eq!((b.version.as_str(), b.pid), ("1.2.3", std::process::id()));
        assert!(answers(d.path()));
        assert!(answers_at(Some(&b), b.at + STALE_AFTER.as_secs()));
        assert!(!answers_at(Some(&b), b.at + STALE_AFTER.as_secs() + 1));
        assert!(!answers_at(None, 0));
    }

    #[test]
    fn the_agent_beats_until_asked_to_stop_or_updated() {
        let d = tempfile::tempdir().unwrap();
        let mut beats = 0;
        let data = d.path().to_path_buf();
        run(
            d.path(),
            "1.0.0",
            &mut |_| {
                beats += 1;
                if beats == 3 {
                    request_stop(&data).unwrap();
                }
            },
            &mut || false,
        );
        assert_eq!(beats, 3);
        assert!(!stop_path(d.path()).exists(), "the request is consumed");
        assert_eq!(read_heartbeat(d.path()).unwrap().version, "1.0.0");
        let mut waited = 0;
        run(d.path(), "1.0.1", &mut |_| waited += 1, &mut || true);
        assert_eq!(waited, 0, "an updated program exits at once");
    }

    #[test]
    fn the_spec_renders_the_bundled_agent() {
        assert!(bundled_plist().contains("<string>Contents/MacOS/shell-demo-agent</string>"));
        assert!(nsis_hooks().contains("shell-demo-agent.exe"));
        assert_eq!(spec().label(), LABEL);
    }
}
