//! macOS 26 keeps the status items an app makes after ControlCenter is running off screen until the app has been active once
//! in the login session; a login start is hidden and never active. At a hidden start Dyno therefore activates itself once, after
//! its items exist, and gives focus back to the app that was in front. The sequence is pure over [`FocusPort`] (the real port
//! asks AppKit on the main thread, the tests use a fake); see `docs/lessons.md` and ADR 0028's amendment.

use crate::logging;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// The app that was in front before Dyno activated itself. Its identity (pid, launch time, bundle id) is kept so the port can
/// check it is still the same process before acting (rule 53: a pid is reusable). No name: logs never carry app names (rule 56).
#[derive(Clone, PartialEq, Eq)]
pub struct Front {
    /// Its process id.
    pub pid: i32,
    /// When it launched (milliseconds since the epoch), if known.
    pub launch_ms: Option<i64>,
    /// Its bundle identifier, if it has one.
    pub bundle_id: Option<String>,
}

impl std::fmt::Debug for Front {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pid {}", self.pid)
    }
}

/// How the log names the app that was in front: its pid, never its name.
pub fn describe(front: Option<&Front>) -> String {
    front.map_or_else(|| "none".to_string(), |f| format!("another app (pid {})", f.pid))
}

/// The first macOS major version the activation gate was measured on (26.6.2). Older systems are left alone: `activate()` and
/// `yieldActivationToApplication` exist from macOS 14 only, and nothing is known of the gate before 26.
pub const FIRST_OS_MAJOR: u64 = 26;

/// Whether the activation sequence runs on this macOS major version.
pub fn os_supported(major: u64) -> bool {
    major >= FIRST_OS_MAJOR
}

/// The ways to ask for the activation, in the order they are tried. macOS 14 made the first two cooperative (a background
/// launch is refused), so the ladder ends with an Apple event `activate` sent by `osascript`, which the OS honours without a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// `NSApplication.activate()`.
    Cooperative,
    /// `NSApplication.activateIgnoringOtherApps(true)` (deprecated; kept as the documented fallback).
    Forced,
    /// `osascript -e 'tell application id "<bundle id>" to activate'`, bounded by a timeout.
    AppleEvent,
}

impl Method {
    /// The order they are tried in.
    pub const LADDER: [Method; 3] = [Method::Cooperative, Method::Forced, Method::AppleEvent];

    /// How many 100 ms polls to wait for this method to take effect.
    pub const fn polls(self) -> u32 {
        match self {
            Method::Cooperative | Method::Forced => 3,
            Method::AppleEvent => 20,
        }
    }

    /// The word the log and the outcome use.
    pub const fn name(self) -> &'static str {
        match self {
            Method::Cooperative => "activate",
            Method::Forced => "activate_forced",
            Method::AppleEvent => "apple_event",
        }
    }
}

/// What the activation needs from the OS.
pub trait FocusPort {
    /// The macOS major version.
    fn os_major(&mut self) -> u64;
    /// Whether every one of Dyno's items is on screen now (`None`: the window list cannot say).
    fn items_shown(&mut self) -> Option<bool>;
    /// Whether any Dyno window is open (a window the person opened is theirs: focus is not taken back from it).
    fn dyno_window_open(&mut self) -> bool;
    /// The frontmost app, unless it is Dyno itself.
    fn frontmost(&mut self) -> Option<Front>;
    /// Whether Dyno is the active app now.
    fn is_active(&mut self) -> bool;
    /// Asks the OS to activate Dyno by `how`.
    fn activate(&mut self, how: Method) -> Result<(), String>;
    /// Makes `to` the active app again, after checking on the main thread that it is still the same process (pid, launch time, bundle id).
    fn give_back(&mut self, to: &Front) -> Result<(), String>;
    /// Waits `ms` milliseconds (a fake does not wait).
    fn wait(&mut self, ms: u64);
}

/// What [`activate_once`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing was done, and why (not a hidden start, no icons yet, already done).
    Skipped(String),
    /// Dyno was already the active app.
    AlreadyActive,
    /// Dyno became active (`how`); focus went back to `returned` unless the person had moved on or a Dyno window was open.
    Activated {
        /// Which call worked.
        how: &'static str,
        /// The app that got focus back, if any was in front.
        returned: Option<Front>,
    },
    /// Neither call made Dyno active.
    Refused,
}

/// Whether the one start activation was done (it is never done twice in a process).
#[derive(Default)]
pub struct StartFocus {
    done: AtomicBool,
}

/// Polls to see whether an activation took, at this interval. The two AppKit calls are refused at once or not at all for a
/// background launch (measured), so they get a short look; the Apple event gets a longer one.
const POLL_MS: u64 = 100;
/// How long Dyno stays active before focus goes back (the menu bar must see the activation).
const HOLD_MS: u64 = 300;
/// How often, and how far apart, the items are looked at before deciding to activate (about 2 s in all).
const LOOKS: u32 = 8;
const LOOK_MS: u64 = 250;

fn became_active<P: FocusPort>(port: &mut P, polls: u32) -> bool {
    for _ in 0..polls {
        if port.is_active() {
            return true;
        }
        port.wait(POLL_MS);
    }
    port.is_active()
}

/// Activates Dyno once. Only at a hidden start (`hidden`), only once the icons exist (`icons_exist`), and only the first time in
/// the process. Logs every attempt and its result (ER-27). Runs off the main thread; the port hops to it per call.
pub fn activate_once<P: FocusPort>(port: &mut P, focus: &StartFocus, hidden: bool, icons_exist: bool) -> Outcome {
    if !hidden {
        return Outcome::Skipped("not a hidden start".to_string());
    }
    if !icons_exist {
        return Outcome::Skipped("the icons do not exist yet".to_string());
    }
    if focus.done.swap(true, Ordering::SeqCst) {
        return Outcome::Skipped("already done in this process".to_string());
    }
    let major = port.os_major();
    if !os_supported(major) {
        logging::info(&format!(
            "tray activation: macOS {major} is older than {FIRST_OS_MAJOR}, the only version the gate was measured on; no activation"
        ));
        return Outcome::Skipped(format!("macOS {major} is not {FIRST_OS_MAJOR} or later"));
    }
    // Items that ControlCenter adopted at its own start are shown within moments; only parked ones need the activation.
    let looked = Instant::now();
    for look in 0..LOOKS {
        match port.items_shown() {
            Some(true) => {
                logging::info(&format!(
                    "tray activation: the items are already on screen after {} ms; no activation, focus untouched",
                    looked.elapsed().as_millis()
                ));
                return Outcome::Skipped("the items are already shown".to_string());
            }
            Some(false) => {}
            None => {
                logging::info("tray activation: the window list cannot say whether the items are shown; activating to be safe");
                break;
            }
        }
        if look + 1 < LOOKS {
            port.wait(LOOK_MS);
        }
    }
    let started = Instant::now();
    let mut before = port.frontmost();
    let front = describe(before.as_ref());
    if port.is_active() {
        logging::info(&format!(
            "tray activation: Dyno is already active (frontmost was {front}); nothing to do"
        ));
        return Outcome::AlreadyActive;
    }
    logging::info(&format!(
        "tray activation: hidden start, Dyno not active; frontmost app {front}; activating once so macOS shows the menu-bar items"
    ));
    let mut how = None;
    for method in Method::LADDER {
        let name = method.name();
        if method == Method::AppleEvent {
            // The person may have switched apps while the first rungs were refused: the one in front now is the one to return to.
            if let Some(now) = port.frontmost() {
                before = Some(now);
            }
        }
        let t = Instant::now();
        let called = port.activate(method);
        let took = became_active(port, method.polls());
        logging::info(&format!(
            "tray activation: attempt {name}: call {}, active afterwards {took}, {} ms",
            called.as_ref().map_or_else(|e| format!("failed ({e})"), |()| "ok".to_string()),
            t.elapsed().as_millis()
        ));
        if took {
            how = Some(name);
            break;
        }
    }
    let Some(how) = how else {
        logging::info("tray activation: neither call made Dyno active; the items may stay off screen until Dyno is started by hand");
        return Outcome::Refused;
    };
    port.wait(HOLD_MS);
    let mut returned = None;
    if !port.is_active() {
        logging::info("tray activation: Dyno is no longer active (the person switched apps); focus is left where it is");
    } else if port.dyno_window_open() {
        logging::info("tray activation: a Dyno window is open; focus stays with Dyno");
    } else if let Some(to) = before {
        let r = port.give_back(&to);
        logging::info(&format!(
            "tray activation: focus given back to {}: {}; Dyno was in front {} ms",
            describe(Some(&to)),
            r.as_ref().map_or_else(|e| format!("failed ({e})"), |()| "ok".to_string()),
            started.elapsed().as_millis()
        ));
        returned = Some(to);
    } else {
        logging::info(&format!(
            "tray activation: no other app was in front; Dyno stays active ({} ms)",
            started.elapsed().as_millis()
        ));
    }
    Outcome::Activated { how, returned }
}
