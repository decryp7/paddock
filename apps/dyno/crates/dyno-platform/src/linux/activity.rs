//! User activity on Linux. Idle time comes from the login manager (`loginctl`), which knows it only once the
//! desktop's own idle threshold passed: until then the session is "not idle" and the answer is 0. Wayland offers no
//! way to ask which window has the focus, so there the foreground process is unknown; on X11 it is read with `xprop`.

use super::tools::Tools;
use crate::parse::linux_tools::{session_idle, xprop_active_window, xprop_pid, SessionIdle};
use dyno_core::ports::UserActivity;
use dyno_core::process::Pid;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// `loginctl` is asked at most this often.
const CACHE: Duration = Duration::from_secs(3);
/// How long `loginctl` and `xprop` may take.
const TOOL_TIMEOUT: Duration = Duration::from_secs(2);

/// The display server, as the environment says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    /// A Wayland session: no focus query.
    Wayland,
    /// An X11 session.
    X11,
    /// No graphical session (a server, ssh).
    None,
}

impl Display {
    /// From `WAYLAND_DISPLAY` and `DISPLAY`.
    pub fn from_env() -> Display {
        let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        if set("WAYLAND_DISPLAY") {
            Display::Wayland
        } else if set("DISPLAY") {
            Display::X11
        } else {
            Display::None
        }
    }
}

/// Activity for Linux.
pub struct LinuxActivity {
    tools: Arc<dyn Tools>,
    session: String,
    display: Display,
    cached: Option<(Instant, Option<u64>)>,
}

impl LinuxActivity {
    /// Activity of `session` (a login session id, or `auto` for the caller's own).
    pub fn new(tools: Arc<dyn Tools>, session: String, display: Display) -> Self {
        LinuxActivity {
            tools,
            session,
            display,
            cached: None,
        }
    }

    /// The session from `XDG_SESSION_ID`, else `auto`; the display from the environment.
    pub fn from_env(tools: Arc<dyn Tools>) -> Self {
        let session = std::env::var("XDG_SESSION_ID")
            .ok()
            .filter(|s| !s.is_empty() && s.len() < 32 && s.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or_else(|| "auto".to_string());
        LinuxActivity::new(tools, session, Display::from_env())
    }

    fn ask(&self) -> Option<u64> {
        let out = self
            .tools
            .run(
                "loginctl",
                &["show-session", &self.session, "-p", "IdleHint", "-p", "IdleSinceHint"],
                TOOL_TIMEOUT,
            )
            .ok()
            .filter(|o| o.ok)?;
        match session_idle(&out.stdout) {
            SessionIdle::Active => Some(0),
            SessionIdle::IdleSince(us) => {
                let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_micros();
                Some(u64::try_from(now.saturating_sub(u128::from(us)) / 1_000_000).unwrap_or(u64::MAX))
            }
            SessionIdle::Unknown => None,
        }
    }
}

impl UserActivity for LinuxActivity {
    fn idle_secs(&mut self) -> Option<u64> {
        if let Some((at, v)) = self.cached {
            if at.elapsed() < CACHE {
                return v;
            }
        }
        let v = self.ask();
        self.cached = Some((Instant::now(), v));
        v
    }

    fn foreground_pid(&mut self) -> Option<Pid> {
        if self.display != Display::X11 || !self.tools.exists("xprop") {
            return None;
        }
        let root = self
            .tools
            .run("xprop", &["-root", "_NET_ACTIVE_WINDOW"], TOOL_TIMEOUT)
            .ok()
            .filter(|o| o.ok)?;
        let window = xprop_active_window(&root.stdout)?;
        let prop = self
            .tools
            .run("xprop", &["-id", &window, "_NET_WM_PID"], TOOL_TIMEOUT)
            .ok()
            .filter(|o| o.ok)?;
        xprop_pid(&prop.stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tools::fake::FakeTools;
    use super::super::tools::ToolOutput;
    use super::*;

    fn idle_since_secs_ago(secs: u64) -> String {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as u64;
        format!("IdleHint=yes\nIdleSinceHint={}\n", now - secs * 1_000_000)
    }

    #[test]
    fn idle_time_follows_the_login_managers_hint_and_is_cached() {
        let tools = Arc::new(FakeTools::with("loginctl", &idle_since_secs_ago(90)));
        let mut a = LinuxActivity::new(tools.clone(), "c2".into(), Display::None);
        let idle = a.idle_secs().unwrap();
        assert!((90..=92).contains(&idle), "{idle}");
        a.idle_secs();
        assert_eq!(tools.calls(), ["loginctl show-session c2 -p IdleHint -p IdleSinceHint"]);
        let active = Arc::new(FakeTools::with("loginctl", "IdleHint=no\nIdleSinceHint=0\n"));
        assert_eq!(LinuxActivity::new(active, "auto".into(), Display::None).idle_secs(), Some(0));
    }

    #[test]
    fn unknown_when_the_session_cannot_say() {
        assert_eq!(
            LinuxActivity::new(Arc::new(FakeTools::default()), "auto".into(), Display::None).idle_secs(),
            None
        );
        let failing = Arc::new(FakeTools::default());
        failing.answer(
            "loginctl",
            Ok(ToolOutput {
                ok: false,
                stdout: String::new(),
                stderr: "no session".into(),
            }),
        );
        assert_eq!(LinuxActivity::new(failing, "auto".into(), Display::None).idle_secs(), None);
        let blank = Arc::new(FakeTools::with("loginctl", "IdleHint=\n"));
        assert_eq!(LinuxActivity::new(blank, "auto".into(), Display::None).idle_secs(), None);
    }

    #[test]
    fn the_foreground_process_is_known_on_x11_only() {
        let tools = Arc::new(FakeTools::default());
        tools.answer(
            "xprop",
            Ok(ToolOutput {
                ok: true,
                stdout: "_NET_ACTIVE_WINDOW(WINDOW): window id # 0x4a00007\n".into(),
                stderr: String::new(),
            }),
        );
        let mut a = LinuxActivity::new(tools.clone(), "auto".into(), Display::X11);
        // The same fake answers both calls; the second one is not a pid, so nothing is claimed.
        assert_eq!(a.foreground_pid(), None);
        assert_eq!(tools.calls().len(), 2);
        let wayland = Arc::new(FakeTools::with("xprop", "x"));
        assert_eq!(
            LinuxActivity::new(wayland.clone(), "auto".into(), Display::Wayland).foreground_pid(),
            None
        );
        assert!(wayland.calls().is_empty(), "no focus query is attempted on Wayland");
        assert_eq!(
            LinuxActivity::new(Arc::new(FakeTools::default()), "auto".into(), Display::X11).foreground_pid(),
            None
        );
    }

    #[test]
    fn x11_pid_is_read_from_the_active_window() {
        struct Scripted;
        impl Tools for Scripted {
            fn run(&self, _p: &str, args: &[&str], _t: Duration) -> Result<ToolOutput, dyno_core::ports::PlatformError> {
                let stdout = if args.contains(&"-root") {
                    "_NET_ACTIVE_WINDOW(WINDOW): window id # 0x4a00007\n"
                } else {
                    "_NET_WM_PID(CARDINAL) = 4242\n"
                };
                Ok(ToolOutput {
                    ok: true,
                    stdout: stdout.into(),
                    stderr: String::new(),
                })
            }
            fn exists(&self, _p: &str) -> bool {
                true
            }
        }
        assert_eq!(
            LinuxActivity::new(Arc::new(Scripted), "auto".into(), Display::X11).foreground_pid(),
            Some(4242)
        );
    }

    #[test]
    fn the_environment_is_read_without_panicking() {
        let _ = Display::from_env();
        let _ = LinuxActivity::from_env(Arc::new(FakeTools::default()));
    }
}
