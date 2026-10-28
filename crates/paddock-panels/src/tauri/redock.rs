//! The adapter of [`crate::redock`]: while a pop-out window is dragged over its host it turns translucent. The cursor, the
//! button and the windows are read here; what the window looks like is decided by the pure core, and the per-OS call that
//! sets a window's opacity is in `native`.

use super::{Panels, TARGET, native};
use crate::redock::{
    Appearance, Bounds, Controller, Environment, Opacity, OpacityError, Probe, SAMPLE_MS, Sample,
    WindowOpacity, pointer_over, should_start, track,
};
use crate::wire::PanelsEvent;
use ::tauri::{AppHandle, Manager, Runtime};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::{Duration, Instant};

/// The pop-out windows being followed, each with when it last reported a move.
#[derive(Default)]
pub(crate) struct State {
    following: Mutex<HashMap<String, Instant>>,
}

impl State {
    /// Notes a move of `label`; false when no one follows it yet (the caller starts a follower).
    fn touch(&self, label: &str) -> bool {
        let mut m = self
            .following
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match m.get_mut(label) {
            Some(t) => {
                *t = Instant::now();
                true
            }
            None => false,
        }
    }

    fn begin(&self, label: &str) {
        self.following
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(label.to_string(), Instant::now());
    }

    fn since_move(&self, label: &str) -> Option<Duration> {
        self.following
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(label)
            .map(Instant::elapsed)
    }

    fn end(&self, label: &str) {
        self.following
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(label);
    }
}

/// The OS call on one window, run on the main thread (AppKit needs it, and Win32 is happiest there).
struct TauriOpacity<R: Runtime> {
    app: AppHandle<R>,
    label: String,
}

impl<R: Runtime> WindowOpacity for TauriOpacity<R> {
    fn set_opacity(&mut self, opacity: f64) -> Result<(), OpacityError> {
        let label = self.label.clone();
        let app = self.app.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.app
            .run_on_main_thread(move || {
                let result = match super::window(&app, &label) {
                    Some(w) => native::set_window_opacity(&w, opacity),
                    None => Err(OpacityError::Failed("the window is gone".into())),
                };
                let _ = tx.send(result);
            })
            .map_err(|e| OpacityError::Failed(e.to_string()))?;
        // This runs on the follower thread, never the main one, so waiting for the main thread's answer is safe.
        rx.recv_timeout(Duration::from_secs(2))
            .map_err(|e| OpacityError::Failed(e.to_string()))?
    }
}

struct Live<R: Runtime> {
    app: AppHandle<R>,
    host: String,
    label: String,
    panel: String,
}

impl<R: Runtime> Live<R> {
    fn over_host(&self) -> bool {
        let (Some(host), Ok(cursor)) = (
            super::window(&self.app, &self.host),
            self.app.cursor_position(),
        ) else {
            return false;
        };
        let (Ok(pos), Ok(size)) = (host.inner_position(), host.inner_size()) else {
            return false;
        };
        let bounds = Bounds {
            x: f64::from(pos.x),
            y: f64::from(pos.y),
            width: f64::from(size.width),
            height: f64::from(size.height),
        };
        pointer_over(bounds, cursor.x, cursor.y)
    }
}

impl<R: Runtime> Probe for Live<R> {
    fn sample(&mut self) -> Sample {
        let state = self.app.try_state::<Panels>();
        let since = state
            .as_ref()
            .and_then(|p| p.redock.since_move(&self.label));
        Sample {
            button_down: native::primary_button_down(),
            over_host: self.over_host(),
            windows_alive: since.is_some()
                && super::window(&self.app, &self.label).is_some()
                && super::window(&self.app, &self.host).is_some(),
            since_move_ms: since.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        }
    }

    fn announce(&mut self, look: Appearance) {
        super::popout::notify(
            &self.app,
            &self.host,
            &PanelsEvent::Redock {
                panel: self.panel.clone(),
                label: self.label.clone(),
                look: look.look(),
            },
        );
    }
}

/// A pop-out window moved while its host may be under it: starts following the drag (once per window).
pub(super) fn on_moved<R: Runtime>(app: &AppHandle<R>, host: &str, label: &str, panel: &str) {
    let Some(panels) = app.try_state::<Panels>() else {
        return;
    };
    if panels.redock.touch(label) {
        return;
    }
    let live = Live {
        app: app.clone(),
        host: host.to_string(),
        label: label.to_string(),
        panel: panel.to_string(),
    };
    if !should_start(native::primary_button_down(), live.over_host(), false) {
        return;
    }
    panels.redock.begin(label);
    let env = Environment {
        reduce_transparency: native::reduce_transparency(),
    };
    let opacity = panels.redock_opacity;
    let window = TauriOpacity {
        app: app.clone(),
        label: label.to_string(),
    };
    let started = std::thread::Builder::new()
        .name("paddock-redock".into())
        .spawn(move || follow(live, window, env, opacity));
    if let Err(e) = started {
        panels.redock.end(label);
        log::warn!(target: TARGET, Error = e.to_string().as_str(); "Could not follow the pop-out drag; it stays opaque");
    }
}

/// The follower thread: runs the core's loop; whatever ends it (also a panic) gives the window its opacity back.
fn follow<R: Runtime>(
    mut live: Live<R>,
    window: TauriOpacity<R>,
    env: Environment,
    opacity: Opacity,
) {
    let label = live.label.clone();
    let app = live.app.clone();
    let mut unsupported_logged = false;
    let mut ctl = Controller::new(window, env, opacity, move |e| match e {
        OpacityError::Unsupported(why) if !unsupported_logged => {
            unsupported_logged = true;
            log::warn!(target: TARGET, Reason = why.as_str(); "This platform cannot make a window translucent; the dock guides get an outline instead");
        }
        OpacityError::Unsupported(_) => {}
        OpacityError::Failed(why) => {
            log::debug!(target: TARGET, Reason = why.as_str(); "Could not set the opacity of the dragged pop-out")
        }
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        track(&mut ctl, &mut live, || {
            std::thread::sleep(Duration::from_millis(SAMPLE_MS))
        });
    }));
    // Opaque again, and the host told, whatever happened.
    drop(ctl);
    if let Some(panels) = app.try_state::<Panels>() {
        panels.redock.end(&label);
    }
    live.announce(Appearance::Opaque);
    if result.is_err() {
        log::error!(target: TARGET, Popout = label.as_str(); "The pop-out drag follower failed; the window was made opaque again");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_is_followed_once_and_forgotten_at_its_end() {
        let s = State::default();
        assert!(!s.touch("p-main-1"), "no follower yet");
        assert_eq!(s.since_move("p-main-1"), None);
        s.begin("p-main-1");
        assert!(
            s.touch("p-main-1"),
            "a follower exists: a move only refreshes the stamp"
        );
        assert!(s.since_move("p-main-1").is_some());
        assert!(!s.touch("p-main-2"), "per window");
        s.end("p-main-1");
        assert!(!s.touch("p-main-1"));
        assert_eq!(s.since_move("p-main-1"), None);
    }

    #[test]
    fn the_builder_keeps_the_opacity_in_range() {
        let default = crate::tauri::Panels::builder().build();
        assert_eq!(default.redock_opacity, Opacity::default());
        let custom = crate::tauri::Panels::builder().redock_opacity(0.7).build();
        assert_eq!(custom.redock_opacity.get(), 0.7);
        let low = crate::tauri::Panels::builder().redock_opacity(0.0).build();
        assert_eq!(low.redock_opacity.get(), crate::redock::MIN_OPACITY);
    }
}
