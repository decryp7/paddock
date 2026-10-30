//! The tray (Windows notification area, macOS menu bar): an icon that shows the worst state of all runners, a
//! tooltip with counts, and a menu with each runner's status and quick actions. Closing the window keeps Pitcrew
//! here; quitting never stops the runners.
//!
//! Icons are the suite's (`icons/tray/`): macOS template images (the system tints them), and on Windows a light
//! and a dark variant chosen from the taskbar's theme. Each state has its own shape, not only its own colour.

use crate::events::{Actions, Latest};
use paddock_window::tauri::{NativeTray, Tray};
use paddock_window::tray::{disabled, item, submenu, TrayEntry, TrayView, MAX_ID, QUIT_ID, SHOW_ID};
use pitcrew_core::health::RunnerStatus;
use pitcrew_core::protocol::RunnerSnapshot;
use tauri::image::Image;
use tauri::{AppHandle, Manager, Runtime};

/// What the tray icon shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrayState {
    /// Every runner is fine (or stopped on purpose).
    Ok,
    /// A runner works (job, update, start, drain).
    Busy,
    /// A runner needs a look (stalled, cannot reach its server).
    Warning,
    /// A runner needs the user (crash loop, not registered), or the service is unavailable.
    Error,
}

/// The state one status contributes. Every status is mapped (the match is exhaustive).
pub fn state_of(status: RunnerStatus) -> TrayState {
    match status {
        RunnerStatus::Idle | RunnerStatus::Stopped | RunnerStatus::Paused => TrayState::Ok,
        RunnerStatus::Busy | RunnerStatus::Updating | RunnerStatus::Starting | RunnerStatus::Draining => TrayState::Busy,
        RunnerStatus::Stalled | RunnerStatus::Degraded | RunnerStatus::WaitingForLogon => TrayState::Warning,
        RunnerStatus::CrashLoop | RunnerStatus::NotRegistered => TrayState::Error,
    }
}

/// The tray state of all runners: the worst one; a low disk is a warning.
pub fn tray_state(runners: &[RunnerSnapshot]) -> TrayState {
    runners
        .iter()
        .map(|r| {
            let s = state_of(r.status);
            if r.disk.is_some_and(|d| d.low) {
                s.max(TrayState::Warning)
            } else {
                s
            }
        })
        .max()
        .unwrap_or(TrayState::Ok)
}

/// Words for a status in the tray menu.
pub fn status_words(s: RunnerStatus) -> &'static str {
    match s {
        RunnerStatus::Idle => "Idle",
        RunnerStatus::Busy => "Running a job",
        RunnerStatus::Starting => "Starting",
        RunnerStatus::Updating => "Updating",
        RunnerStatus::Stopped => "Stopped",
        RunnerStatus::Paused => "Paused (maintenance)",
        RunnerStatus::Draining => "Finishing its job",
        RunnerStatus::WaitingForLogon => "Waiting for log-on",
        RunnerStatus::Degraded => "Cannot reach the server",
        RunnerStatus::Stalled => "Stalled — restarting",
        RunnerStatus::NotRegistered => "Not registered",
        RunnerStatus::CrashLoop => "Keeps crashing",
    }
}

/// The tooltip: a summary line.
pub fn tooltip(runners: &[RunnerSnapshot], connected: bool) -> String {
    if !connected {
        return "Pitcrew — the service is not running".into();
    }
    if runners.is_empty() {
        return "Pitcrew — no runners yet".into();
    }
    let busy = runners.iter().filter(|r| r.status == RunnerStatus::Busy).count();
    let attention = runners.iter().filter(|r| r.status.needs_attention()).count();
    let mut s = format!("Pitcrew — {} runner{}", runners.len(), if runners.len() == 1 { "" } else { "s" });
    if busy > 0 {
        s.push_str(&format!(", {busy} busy"));
    }
    if attention > 0 {
        s.push_str(&format!(", {attention} need{} attention", if attention == 1 { "s" } else { "" }));
    }
    if let Some(load) = load_text(runners.iter().filter_map(|r| r.usage.as_ref())) {
        s.push_str(&format!(" — {load}"));
    }
    s
}

/// What the runners use together, for the tooltip: `CPU 12% · 4.2 GB` (`None` when none of them runs).
pub fn load_text<'a>(usages: impl Iterator<Item = &'a pitcrew_core::usage::Usage>) -> Option<String> {
    let usage = pitcrew_core::usage::Usage::sum(usages);
    if usage.processes == 0 {
        return None;
    }
    let pct = f64::from(usage.cpu_permille) / 10.0;
    let gb = usage.mem_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    Some(format!("CPU {pct:.0}% · {gb:.1} GB"))
}

/// The icon for a state (PNG bytes built into the app).
pub fn icon_bytes(state: TrayState, dark_taskbar: bool) -> &'static [u8] {
    macro_rules! icons {
        ($($s:ident => $k:literal),*) => {
            match (state, dark_taskbar, cfg!(target_os = "macos")) {
                $(
                    (TrayState::$s, _, true) => include_bytes!(concat!("../icons/tray/macos/pitcrew-tray-", $k, "-18ptTemplate@2x.png")),
                    (TrayState::$s, true, false) => include_bytes!(concat!("../icons/tray/windows/pitcrew-tray-", $k, "-dark-32.png")),
                    (TrayState::$s, false, false) => include_bytes!(concat!("../icons/tray/windows/pitcrew-tray-", $k, "-light-32.png")),
                )*
            }
        };
    }
    icons!(Ok => "ok", Busy => "busy", Warning => "warning", Error => "error")
}

/// Whether the Windows taskbar is dark (read-only registry lookup; dark when unknown, the Windows 11 default).
pub fn dark_taskbar() -> bool {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        winreg::RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
            .and_then(|k| k.get_value::<u32, _>("SystemUsesLightTheme"))
            .map(|v| v == 0)
            .unwrap_or(true)
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// A menu item id for a runner action: `runner:<action>:<name>`.
pub fn action_id(action: &str, runner: &str) -> String {
    format!("runner:{action}:{runner}")
}

/// Parses an id of [`action_id`].
pub fn parse_action_id(id: &str) -> Option<(&str, &str)> {
    let rest = id.strip_prefix("runner:")?;
    rest.split_once(':')
}

/// The tray's menu: a summary line, each runner with its quick actions, then the shell's Open and Quit items.
pub fn menu(runners: &[RunnerSnapshot], connected: bool) -> Vec<TrayEntry> {
    let mut menu = vec![disabled(&tooltip(runners, connected)), TrayEntry::Separator];
    for r in runners {
        let mut items = Vec::new();
        if let Some(job) = &r.current_job {
            let what = job.info.repository.clone().unwrap_or_else(|| "a job".into());
            let job_name = job.info.job.clone().map(|j| format!(" \u{b7} {j}")).unwrap_or_default();
            items.push(disabled(&format!("{what}{job_name}")));
            items.push(TrayEntry::Separator);
        }
        let paused = matches!(r.status, RunnerStatus::Paused | RunnerStatus::Draining);
        let running = r.running_since.is_some();
        let actions: [(&str, &str, bool); 9] = [
            ("restart", "Restart after the current job", running),
            (
                if paused { "resume" } else { "drain" },
                if paused { "Resume" } else { "Drain for maintenance" },
                true,
            ),
            ("start", "Start", !running && !paused),
            ("stop", "Stop after the current job", running),
            ("applyUpdate", "Update now", r.update_target.is_some()),
            ("clean", "Clean up now", true),
            ("processes", "Processes\u{2026}", true),
            ("config", "Configuration\u{2026}", true),
            ("logs", "Open logs", true),
        ];
        for (action, label, enabled) in actions {
            let id = action_id(action, r.name.as_str());
            // An id the tray cannot carry would make it refuse the whole menu: leave that one action out.
            if !enabled || id.len() > MAX_ID {
                continue;
            }
            items.push(item(id, label));
        }
        menu.push(submenu(&format!("{} \u{2014} {}", r.name, status_words(r.status)), items));
    }
    if !runners.is_empty() {
        menu.push(TrayEntry::Separator);
    }
    menu.push(item(SHOW_ID, "Open Pitcrew"));
    menu.push(TrayEntry::Separator);
    menu.push(item(QUIT_ID, "Quit Pitcrew (runners keep running)"));
    menu
}

/// What the tray shows now: the worst state of all runners (the service being unreachable is an error).
pub fn view(runners: &[RunnerSnapshot], connected: bool) -> TrayView {
    let state = if connected { tray_state(runners) } else { TrayState::Error };
    TrayView::new(&tooltip(runners, connected), menu(runners, connected), state.key())
}

impl TrayState {
    /// The icon's name in the tray view.
    pub fn key(self) -> &'static str {
        match self {
            TrayState::Ok => "ok",
            TrayState::Busy => "busy",
            TrayState::Warning => "warning",
            TrayState::Error => "error",
        }
    }

    fn from_key(key: &str) -> TrayState {
        match key {
            "ok" => TrayState::Ok,
            "busy" => TrayState::Busy,
            "warning" => TrayState::Warning,
            _ => TrayState::Error,
        }
    }
}

/// The tray over the shell's window layer: its model reads what the event thread keeps current in [`Latest`], and a left
/// click shows the window (menu on macOS). Returns whether the icon exists (Linux without an app indicator: closing the
/// last window then quits, which the window layer decides).
pub fn create<R: Runtime>(app: &AppHandle<R>) -> bool {
    let native = NativeTray::new(|key| Image::from_bytes(icon_bytes(TrayState::from_key(key), dark_taskbar())).ok());
    let tray = Tray::new(native, |app: &AppHandle<R>| {
        let latest = app.state::<Latest>();
        let connected = matches!(latest.connection(), crate::dto::ConnectionState::Connected { .. });
        let runners = if connected {
            latest.status().map(|s| s.runners).unwrap_or_default()
        } else {
            Vec::new()
        };
        view(&runners, connected)
    })
    .on_action(|app, id| {
        let Some((action, runner)) = parse_action_id(id) else { return };
        if crate::events::page_route(action, runner).is_some() {
            let _ = paddock_window::tauri::reveal(app, "main");
        }
        app.state::<Actions>().run(app, action, runner);
    });
    paddock_window::tauri::create_tray(app, tray)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [RunnerStatus; 12] = [
        RunnerStatus::Stopped,
        RunnerStatus::Paused,
        RunnerStatus::WaitingForLogon,
        RunnerStatus::Draining,
        RunnerStatus::NotRegistered,
        RunnerStatus::CrashLoop,
        RunnerStatus::Updating,
        RunnerStatus::Busy,
        RunnerStatus::Stalled,
        RunnerStatus::Starting,
        RunnerStatus::Degraded,
        RunnerStatus::Idle,
    ];

    #[test]
    fn every_status_has_an_icon_and_words() {
        for s in ALL {
            let state = state_of(s);
            for dark in [false, true] {
                let png = icon_bytes(state, dark);
                assert_eq!(&png[1..4], b"PNG", "{s:?}");
            }
            assert!(!status_words(s).is_empty());
            // Statuses that need attention never show as fine.
            if s.needs_attention() {
                assert!(state >= TrayState::Warning, "{s:?}");
            }
        }
        assert_eq!(state_of(RunnerStatus::CrashLoop), TrayState::Error);
        assert_eq!(state_of(RunnerStatus::Busy), TrayState::Busy);
        assert_eq!(state_of(RunnerStatus::Stalled), TrayState::Warning);
        assert_eq!(tray_state(&[]), TrayState::Ok);
    }

    #[test]
    fn menu_ids_round_trip_and_tooltips_count() {
        let id = action_id("drain", "build-pc");
        assert_eq!(parse_action_id(&id), Some(("drain", "build-pc")));
        assert_eq!(parse_action_id("open"), None);
        assert_eq!(tooltip(&[], false), "Pitcrew — the service is not running");
        assert_eq!(tooltip(&[], true), "Pitcrew — no runners yet");
    }

    #[test]
    fn the_tooltip_adds_what_the_runners_use() {
        use pitcrew_core::usage::Usage;
        assert_eq!(load_text([].iter()), None, "nothing runs");
        let a = Usage::tick(120, 2 * 1024 * 1024 * 1024, 4);
        let b = Usage::tick(60, 1024 * 1024 * 1024 + 512 * 1024 * 1024, 2);
        assert_eq!(load_text([a, b].iter()).as_deref(), Some("CPU 18% · 3.5 GB"));
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;

    #[test]
    fn without_the_service_the_tray_shows_an_error_and_only_open_and_quit() {
        let v = view(&[], false);
        assert_eq!(v.icon, "error");
        assert!(paddock_window::tray::validate(&v.menu).is_ok());
        assert!(v.menu.iter().any(|e| matches!(e, TrayEntry::Item { id, .. } if id == SHOW_ID)));
        assert!(v.menu.iter().any(|e| matches!(e, TrayEntry::Item { id, .. } if id == QUIT_ID)));
    }

    #[test]
    fn icon_keys_round_trip() {
        for s in [TrayState::Ok, TrayState::Busy, TrayState::Warning, TrayState::Error] {
            assert_eq!(TrayState::from_key(s.key()), s);
        }
    }
}
