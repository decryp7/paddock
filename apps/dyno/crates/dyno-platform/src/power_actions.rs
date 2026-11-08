//! Sleep, hibernate and shut down by the OS's normal path, and the idle rule's exceptions (ADR 0032 section 4 and 10). The
//! mapping from an action to the OS's command and from the sleep report to the signals is pure and tested everywhere; the
//! calls themselves are one small place per OS. Nothing here ever forces: no `EWX_FORCE`, no inhibitor override, no
//! "really shut down" path. Tests never use [`OsPowerActions`] (ER-14): they use the fakes in [`crate::fake`].

use crate::extras::sleep_report;
use crate::issues::OsRunner;
#[cfg(any(not(windows), test))]
use crate::issues::ToolRunner;
use dyno_core::extras::sleep::{Prevents, SleepReport};
use dyno_core::metrics::Reading;
use dyno_core::ports::{IdleSignals, IdleSignalsReading, PlatformError, PowerActions};
use dyno_core::power::idle::IdleAction;
#[cfg(any(not(windows), test))]
use std::time::Duration;

/// How long the OS's own tool gets to take the request.
#[cfg(not(windows))]
const ASK_TIMEOUT: Duration = Duration::from_secs(20);

/// The program and arguments that ask `os` ("macos", "linux") for `action` the normal way.
pub fn command_for(os: &str, action: IdleAction) -> Result<(&'static str, &'static [&'static str]), PlatformError> {
    match (os, action) {
        ("macos", IdleAction::Sleep) => Ok(("/usr/bin/pmset", &["sleepnow"])),
        // The loginwindow's own event: apps are asked to quit and can cancel (may need the Automation permission).
        ("macos", IdleAction::ShutDown) => Ok(("/usr/bin/osascript", &["-e", "tell application \"System Events\" to shut down"])),
        ("macos", IdleAction::Hibernate) => Err(PlatformError::Unsupported(
            "macOS decides by itself when to hibernate; Dyno cannot ask for it.".into(),
        )),
        // logind: every block inhibitor is respected, and the desktop's own are checked by the session.
        ("linux", IdleAction::Sleep) => Ok(("systemctl", &["suspend"])),
        ("linux", IdleAction::Hibernate) => Ok(("systemctl", &["hibernate"])),
        ("linux", IdleAction::ShutDown) => Ok(("systemctl", &["poweroff"])),
        _ => Err(PlatformError::Unsupported("Dyno cannot ask this system to do that.".into())),
    }
}

/// Words that mean the blocker is playing audio (coreaudiod on macOS, "Playing audio" in logind, the AUDIO section of powercfg).
fn is_audio(name: &str, reason: &str) -> bool {
    let (n, r) = (name.to_ascii_lowercase(), reason.to_ascii_lowercase());
    n.contains("coreaudio") || r.contains("audio") || n == "audio"
}

/// The OS's own bookkeeping, which is not a program asking the computer to stay awake: macOS `powerd` holds "Prevent sleep while
/// display is on" whenever the display is on (measured in a macOS 26 VM, 2026-11-04), and would block every idle rule.
fn is_system_noise(name: &str, reason: &str) -> bool {
    name.eq_ignore_ascii_case("powerd") || reason.to_ascii_lowercase().contains("prevent sleep while display is on")
}

/// The signals a sleep report gives: audio playing and the other programs that asked the computer to stay awake. Dyno's own
/// hold (by pid, or named Dyno) is left out. Whatever the report could not read is "cannot tell" with its reason.
pub fn signals_from_report(report: &SleepReport, own_pid: u32) -> (Reading<bool>, Reading<Vec<String>>) {
    if let Some(why) = &report.blockers_unavailable {
        return (Reading::Unavailable(why.clone()), Reading::Unavailable(why.clone()));
    }
    let mut audio = false;
    let mut others: Vec<String> = Vec::new();
    for b in &report.blockers {
        if b.pid == Some(own_pid)
            || b.name.eq_ignore_ascii_case("dyno")
            || b.name.to_ascii_lowercase().starts_with("dyno-")
            || is_system_noise(&b.name, &b.reason)
        {
            continue;
        }
        if is_audio(&b.name, &b.reason) {
            audio = true;
        } else if matches!(b.prevents, Prevents::Sleep | Prevents::DisplaySleep) && !others.contains(&b.name) {
            others.push(b.name.clone());
        }
    }
    (Reading::Available(audio), Reading::Available(others))
}

/// Asks the real OS.
pub struct OsPowerActions;

impl PowerActions for OsPowerActions {
    fn can(&self, action: IdleAction) -> Reading<()> {
        #[cfg(windows)]
        {
            let _ = action;
            Reading::Available(())
        }
        #[cfg(not(windows))]
        match command_for(std::env::consts::OS, action) {
            Ok(_) => Reading::Available(()),
            Err(e) => Reading::Unavailable(e.to_string()),
        }
    }

    fn request(&self, action: IdleAction) -> Result<(), PlatformError> {
        #[cfg(windows)]
        {
            crate::windows::power::request(action)
        }
        #[cfg(not(windows))]
        {
            let (program, args) = command_for(std::env::consts::OS, action)?;
            let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
            OsRunner.run(program, &args, ASK_TIMEOUT).map(|_| ())
        }
    }
}

/// Reads the real OS's exceptions.
pub struct OsIdleSignals;

impl IdleSignals for OsIdleSignals {
    fn read(&self) -> IdleSignalsReading {
        let report = sleep_report(&OsRunner, std::env::consts::OS);
        let (from_report, stay_awake) = signals_from_report(&report, std::process::id());
        #[cfg(windows)]
        let (audio, full_screen) = {
            // Audio needs no administrator on Windows (WASAPI); the stay-awake list does, and stays "cannot tell" without the helper.
            let _ = from_report;
            (crate::windows::power::audio_playing(), crate::windows::power::full_screen())
        };
        #[cfg(not(windows))]
        let (audio, full_screen) = (
            from_report,
            full_screen_of(&OsRunner, std::env::consts::OS, &|k| std::env::var(k).ok()),
        );
        IdleSignalsReading {
            audio,
            full_screen,
            stay_awake,
        }
    }
}

/// A JavaScript for Automation script that says whether an on-screen app window fills a whole screen (a full-screen app or a
/// borderless game): normal and maximised windows leave the menu bar, so only a full-screen window matches a screen's size.
#[cfg(any(not(windows), test))]
const MAC_FULL_SCREEN_JXA: &str = "ObjC.import('CoreGraphics'); ObjC.import('AppKit'); \
var list = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements, $.kCGNullWindowID))); \
var screens = $.NSScreen.screens; var sizes = []; \
for (var i = 0; i < screens.count; i++) { var f = screens.objectAtIndex(i).frame; sizes.push(Math.round(f.size.width) + 'x' + Math.round(f.size.height)); } \
var full = false; \
for (var j = 0; j < list.length; j++) { var w = list[j]; if (w.kCGWindowLayer !== 0) continue; var b = w.kCGWindowBounds; \
if (sizes.indexOf(Math.round(b.Width) + 'x' + Math.round(b.Height)) >= 0 && ['Window Server', 'Dock', 'Finder'].indexOf(w.kCGWindowOwnerName) < 0) { full = true; break; } } \
full ? 'full-screen' : 'not-full-screen'";

/// What the JXA script printed: `Some(true)` for a full-screen window, `None` for anything else the script did not say.
pub fn parse_mac_full_screen(out: &str) -> Option<bool> {
    match out.trim() {
        "full-screen" => Some(true),
        "not-full-screen" => Some(false),
        _ => None,
    }
}

/// The window id `xprop -root _NET_ACTIVE_WINDOW` prints (`_NET_ACTIVE_WINDOW(WINDOW): window id # 0x4a00007`), if it names one.
pub fn parse_active_window(out: &str) -> Option<String> {
    let id = out.split('#').nth(1)?.trim();
    (id.starts_with("0x") && id.len() > 2 && id != "0x0" && id.chars().skip(2).all(|c| c.is_ascii_hexdigit())).then(|| id.to_string())
}

/// Whether `xprop -id <id> _NET_WM_STATE` lists the full-screen state.
pub fn parse_wm_state_full_screen(out: &str) -> bool {
    out.contains("_NET_WM_STATE_FULLSCREEN")
}

/// Whether a full-screen app shows, where the OS lets Dyno ask: macOS by the window list, Linux on X11 by the active window's
/// state; Wayland and anything else is an honest "cannot tell". `env` reads the environment (a test passes its own).
#[cfg(any(not(windows), test))]
pub fn full_screen_of(runner: &dyn ToolRunner, os: &str, env: &dyn Fn(&str) -> Option<String>) -> Reading<bool> {
    const QUICK: Duration = Duration::from_secs(8);
    let cannot = |why: &str| Reading::Unavailable(why.to_string());
    match os {
        "macos" => match runner.run(
            "/usr/bin/osascript",
            &[
                "-l".to_string(),
                "JavaScript".to_string(),
                "-e".to_string(),
                MAC_FULL_SCREEN_JXA.to_string(),
            ],
            QUICK,
        ) {
            Ok(out) => parse_mac_full_screen(&out).map_or_else(
                || cannot("macOS gave an answer Dyno could not read when asked whether an app is full screen."),
                Reading::Available,
            ),
            Err(e) => cannot(&format!("macOS would not say whether an app is full screen ({e}).")),
        },
        "linux" => {
            if env("XDG_SESSION_TYPE").is_some_and(|t| t.eq_ignore_ascii_case("wayland"))
                || (env("DISPLAY").is_none() && env("WAYLAND_DISPLAY").is_some())
            {
                return cannot("Wayland does not let a program ask whether another app is full screen.");
            }
            if env("DISPLAY").is_none() {
                return cannot("Dyno has no X display to ask whether an app is full screen.");
            }
            let id = match runner.run("xprop", &["-root".to_string(), "_NET_ACTIVE_WINDOW".to_string()], QUICK) {
                Ok(out) => match parse_active_window(&out) {
                    Some(id) => id,
                    None => return Reading::Available(false),
                },
                Err(e) => {
                    return cannot(&format!(
                        "xprop (from x11-utils) is needed to tell whether an app is full screen ({e})."
                    ))
                }
            };
            match runner.run("xprop", &["-id".to_string(), id, "_NET_WM_STATE".to_string()], QUICK) {
                Ok(out) => Reading::Available(parse_wm_state_full_screen(&out)),
                Err(e) => cannot(&format!("xprop would not read the active window ({e}).")),
            }
        }
        _ => cannot("This system gives Dyno no full-screen state."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issues::testing::Scripted;
    use dyno_core::extras::sleep::SleepBlocker;

    fn blocker(name: &str, pid: Option<u32>, prevents: Prevents, reason: &str) -> SleepBlocker {
        SleepBlocker {
            name: name.into(),
            pid,
            prevents,
            reason: reason.into(),
            held_for: None,
        }
    }

    #[test]
    fn each_os_asks_the_normal_way_and_never_forces() {
        let c = |os, a| {
            command_for(os, a)
                .map(|(p, a)| format!("{p} {}", a.join(" ")))
                .map_err(|e| e.to_string())
        };
        assert_eq!(c("macos", IdleAction::Sleep).unwrap(), "/usr/bin/pmset sleepnow");
        assert!(c("macos", IdleAction::ShutDown).unwrap().contains("System Events"));
        assert!(c("macos", IdleAction::Hibernate).unwrap_err().contains("by itself"));
        assert_eq!(c("linux", IdleAction::Sleep).unwrap(), "systemctl suspend");
        assert_eq!(c("linux", IdleAction::Hibernate).unwrap(), "systemctl hibernate");
        assert_eq!(c("linux", IdleAction::ShutDown).unwrap(), "systemctl poweroff");
        assert!(c("freebsd", IdleAction::Sleep).is_err());
        for os in ["macos", "linux"] {
            for a in [IdleAction::Sleep, IdleAction::Hibernate, IdleAction::ShutDown] {
                if let Ok((p, args)) = command_for(os, a) {
                    let all = format!("{p} {}", args.join(" "));
                    assert!(
                        !all.contains("--force") && !all.contains("-f ") && !all.contains("ignore-inhibitors") && !all.contains("now -h"),
                        "{all}"
                    );
                }
            }
        }
    }

    #[test]
    fn signals_split_audio_from_other_stay_awake_requests_and_leave_dynos_own_out() {
        let report = SleepReport {
            blockers: vec![
                blocker(
                    "coreaudiod",
                    Some(11),
                    Prevents::Sleep,
                    "com.apple.audio.context.preventuseridlesleep",
                ),
                blocker("Zoom", Some(12), Prevents::DisplaySleep, "call"),
                blocker("Zoom", Some(12), Prevents::Sleep, "call"),
                blocker("Dyno", Some(99), Prevents::Sleep, "keeping awake for 1 h, you asked"),
                blocker("dyno-agent", Some(5), Prevents::Sleep, "x"),
                blocker("someone", Some(99), Prevents::Sleep, "own pid"),
                blocker("Firefox", Some(13), Prevents::Sleep, "Playing audio"),
                blocker("Backup", None, Prevents::Other, "network"),
                blocker(
                    "powerd",
                    Some(99_999),
                    Prevents::Sleep,
                    "Powerd - Prevent sleep while display is on",
                ),
                blocker("x", Some(3), Prevents::Sleep, "Prevent sleep while display is on"),
            ],
            ..Default::default()
        };
        let (audio, others) = signals_from_report(&report, 99);
        assert_eq!(audio, Reading::Available(true));
        assert_eq!(others, Reading::Available(vec!["Zoom".to_string()]));
        let quiet = signals_from_report(&SleepReport::default(), 1);
        assert_eq!(quiet, (Reading::Available(false), Reading::Available(vec![])));
    }

    #[test]
    fn an_unreadable_report_makes_both_signals_cannot_tell_with_the_reason() {
        let report = SleepReport {
            blockers_unavailable: Some("Windows only lets an administrator list what keeps it awake".into()),
            ..Default::default()
        };
        let (audio, stay) = signals_from_report(&report, 1);
        assert!(matches!(audio, Reading::Unavailable(ref r) if r.contains("administrator")));
        assert!(matches!(stay, Reading::Unavailable(ref r) if r.contains("administrator")));
    }

    fn ok(text: &str) -> Result<String, PlatformError> {
        Ok(text.to_string())
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn the_mac_script_answers_are_read_and_anything_else_is_cannot_tell() {
        assert_eq!(parse_mac_full_screen("full-screen\n"), Some(true));
        assert_eq!(parse_mac_full_screen(" not-full-screen "), Some(false));
        assert_eq!(parse_mac_full_screen(""), None);
        assert_eq!(parse_mac_full_screen("execution error: not allowed"), None);
        assert_eq!(
            full_screen_of(&Scripted::new(vec![ok("full-screen\n")]), "macos", &no_env),
            Reading::Available(true)
        );
        assert_eq!(
            full_screen_of(&Scripted::new(vec![ok("not-full-screen\n")]), "macos", &no_env),
            Reading::Available(false)
        );
        assert!(
            matches!(full_screen_of(&Scripted::new(vec![ok("surprise")]), "macos", &no_env), Reading::Unavailable(r) if r.contains("could not read"))
        );
        let refused = Scripted::new(vec![Err(PlatformError::AccessDenied("not authorised".into()))]);
        assert!(matches!(full_screen_of(&refused, "macos", &no_env), Reading::Unavailable(r) if r.contains("not authorised")));
    }

    #[test]
    fn x11_reads_the_active_windows_state_and_wayland_is_an_honest_cannot_tell() {
        assert_eq!(
            parse_active_window("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x4a00007\n"),
            Some("0x4a00007".into())
        );
        for none in [
            "_NET_ACTIVE_WINDOW(WINDOW): window id # 0x0",
            "_NET_ACTIVE_WINDOW:  not found.",
            "",
            "# 0xzz",
        ] {
            assert_eq!(parse_active_window(none), None, "{none}");
        }
        assert!(parse_wm_state_full_screen(
            "_NET_WM_STATE(ATOM) = _NET_WM_STATE_FOCUSED, _NET_WM_STATE_FULLSCREEN"
        ));
        assert!(!parse_wm_state_full_screen("_NET_WM_STATE(ATOM) = _NET_WM_STATE_FOCUSED"));
        let x11 = |k: &str| (k == "DISPLAY").then(|| ":0".to_string());
        let on = Scripted::new(vec![
            ok("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x1\n"),
            ok("_NET_WM_STATE(ATOM) = _NET_WM_STATE_FULLSCREEN\n"),
        ]);
        assert_eq!(full_screen_of(&on, "linux", &x11), Reading::Available(true));
        let off = Scripted::new(vec![
            ok("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x1\n"),
            ok("_NET_WM_STATE(ATOM) = \n"),
        ]);
        assert_eq!(full_screen_of(&off, "linux", &x11), Reading::Available(false));
        let none = Scripted::new(vec![ok("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x0\n")]);
        assert_eq!(
            full_screen_of(&none, "linux", &x11),
            Reading::Available(false),
            "no active window: nothing is full screen"
        );
        let wayland = |k: &str| match k {
            "XDG_SESSION_TYPE" => Some("wayland".to_string()),
            "DISPLAY" => Some(":0".to_string()),
            _ => None,
        };
        assert!(matches!(full_screen_of(&Scripted::new(vec![]), "linux", &wayland), Reading::Unavailable(r) if r.contains("Wayland")));
        assert!(matches!(full_screen_of(&Scripted::new(vec![]), "linux", &no_env), Reading::Unavailable(r) if r.contains("no X display")));
        let missing = Scripted::new(vec![Err(PlatformError::Unsupported("xprop: not found".into()))]);
        assert!(matches!(full_screen_of(&missing, "linux", &x11), Reading::Unavailable(r) if r.contains("x11-utils")));
        let second = Scripted::new(vec![
            ok("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x1\n"),
            Err(PlatformError::Timeout("slow".into())),
        ]);
        assert!(matches!(full_screen_of(&second, "linux", &x11), Reading::Unavailable(r) if r.contains("would not read")));
        assert!(matches!(
            full_screen_of(&Scripted::new(vec![]), "freebsd", &no_env),
            Reading::Unavailable(_)
        ));
    }
}
