//! macOS 26 keeps the status items an app makes after ControlCenter is running off screen until the app has been active once
//! in the login session; a login start is hidden and never active. At a hidden start Dyno therefore activates itself once, after
//! its items exist, and gives focus back to the app that was in front. The sequence is pure over [`FocusPort`] (the real port
//! asks AppKit on the main thread, the tests use a fake); see `docs/lessons.md` and ADR 0028's amendment.

use crate::logging;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// The app that was in front before Dyno activated itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Front {
    /// Its process id.
    pub pid: i32,
    /// Its name, for the log.
    pub name: String,
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
    /// The frontmost app, unless it is Dyno itself.
    fn frontmost(&mut self) -> Option<Front>;
    /// Whether Dyno is the active app now.
    fn is_active(&mut self) -> bool;
    /// Asks the OS to activate Dyno by `how`.
    fn activate(&mut self, how: Method) -> Result<(), String>;
    /// Makes `to` the active app again.
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
    /// Dyno became active (`how`) and focus went back to `returned`.
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

/// Polls to see whether an activation took (the cooperative one is asynchronous), at this interval and count.
const POLL_MS: u64 = 100;
const POLLS: u32 = 10;
/// How long Dyno stays active before focus goes back (the menu bar must see the activation).
const HOLD_MS: u64 = 300;

fn became_active<P: FocusPort>(port: &mut P) -> bool {
    for _ in 0..POLLS {
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
    let started = Instant::now();
    let before = port.frontmost();
    let front = before
        .as_ref()
        .map_or_else(|| "none".to_string(), |f| format!("{} (pid {})", f.name, f.pid));
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
        let t = Instant::now();
        let called = port.activate(method);
        let took = became_active(port);
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
    let returned = before;
    if let Some(to) = &returned {
        let r = port.give_back(to);
        logging::info(&format!(
            "tray activation: focus given back to {} (pid {}): {}; Dyno was in front {} ms",
            to.name,
            to.pid,
            r.as_ref().map_or_else(|e| format!("failed ({e})"), |()| "ok".to_string()),
            started.elapsed().as_millis()
        ));
    } else {
        logging::info(&format!(
            "tray activation: no other app was in front; Dyno stays active ({} ms)",
            started.elapsed().as_millis()
        ));
    }
    Outcome::Activated { how, returned }
}
