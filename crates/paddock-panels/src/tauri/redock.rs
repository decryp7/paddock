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
    /// `PanelsBuilder::windows_translucency`.
    windows_translucency: bool,
}

impl<R: Runtime> WindowOpacity for TauriOpacity<R> {
    fn set_opacity(&mut self, opacity: f64) -> Result<(), OpacityError> {
        if opacity < 1.0 {
            native::translucency_allowed(self.windows_translucency)?;
        }
        let label = self.label.clone();
        let app = self.app.clone();
        on_main(&self.app, move || match super::window(&app, &label) {
            // The mock runtime has no native window (its handles point nowhere): nothing is called (rule 7).
            Some(_) if cfg!(any(test, feature = "mock-runtime")) => Ok(()),
            Some(w) => native::set_window_opacity(&w, opacity),
            None => Err(OpacityError::Failed("the window is gone".into())),
        })
        .and_then(|r| r)
    }
}

/// Runs `f` on the main thread and waits for its answer. Only the follower thread calls this, never the main one.
fn on_main<R: Runtime, T: Send + 'static>(
    app: &AppHandle<R>,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, OpacityError> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|e| OpacityError::Failed(e.to_string()))?;
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|e| OpacityError::Failed(e.to_string()))
}

struct Live<R: Runtime> {
    app: AppHandle<R>,
    host: String,
    label: String,
    panel: String,
}

impl<R: Runtime> Live<R> {
    /// Everything one sample needs, read in one pass on the main thread: whether the windows exist, the pointer is over the
    /// host, and the button.
    fn look(app: &AppHandle<R>, host: &str, label: &str) -> (bool, bool, Option<bool>) {
        let popout = super::window(app, label);
        let host = super::window(app, host);
        let (Some(popout), Some(host)) = (popout, host) else {
            return (false, false, None);
        };
        let over = app.cursor_position().is_ok_and(|cursor| {
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
        });
        (true, over, native::button_down_for(&popout))
    }
}

impl<R: Runtime> Probe for Live<R> {
    fn sample(&mut self) -> Sample {
        let since = self
            .app
            .try_state::<Panels>()
            .and_then(|p| p.redock.since_move(&self.label));
        let (app, host, label) = (self.app.clone(), self.host.clone(), self.label.clone());
        let (alive, over_host, button_down) =
            on_main(&self.app, move || Self::look(&app, &host, &label))
                .unwrap_or((false, false, None));
        Sample {
            button_down,
            over_host,
            windows_alive: alive && since.is_some(),
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
    // A window event runs on the main thread: read the world directly.
    start(app, host, label, panel, Live::look(app, host, label));
}

/// Starts the follower for `world` (windows alive, pointer over the host, button) when it is a drag over the host.
fn start<R: Runtime>(
    app: &AppHandle<R>,
    host: &str,
    label: &str,
    panel: &str,
    world: (bool, bool, Option<bool>),
) {
    let Some(panels) = app.try_state::<Panels>() else {
        return;
    };
    let (alive, over_host, button) = world;
    if !alive || !should_start(button, over_host) {
        return;
    }
    panels.redock.begin(label);
    let live = Live {
        app: app.clone(),
        host: host.to_string(),
        label: label.to_string(),
        panel: panel.to_string(),
    };
    let env = Environment {
        reduce_transparency: native::reduce_transparency(),
    };
    let window = TauriOpacity {
        app: app.clone(),
        label: label.to_string(),
        windows_translucency: panels.redock_windows,
    };
    let opacity = panels.redock_opacity;
    let (end_app, end_label) = (app.clone(), label.to_string());
    let started = std::thread::Builder::new()
        .name("paddock-redock".into())
        .spawn(move || {
            follow(live, window, env, opacity, pause, move || {
                if let Some(panels) = end_app.try_state::<Panels>() {
                    panels.redock.end(&end_label);
                }
            });
        });
    if let Err(e) = started {
        panels.redock.end(label);
        log::warn!(target: TARGET, Error = e.to_string().as_str(); "Could not follow the pop-out drag; it stays opaque");
    }
}

/// Logs what went wrong with a window's opacity; a platform that cannot do it once per drag, not per sample.
fn log_problem(unsupported_logged: &mut bool, e: &OpacityError) {
    match e {
        OpacityError::Unsupported(why) if !*unsupported_logged => {
            *unsupported_logged = true;
            log::warn!(target: TARGET, Reason = why.as_str(); "This platform cannot make a window translucent; the dock guides get an outline instead");
        }
        OpacityError::Unsupported(_) => {}
        OpacityError::Failed(why) => {
            log::debug!(target: TARGET, Reason = why.as_str(); "Could not set the opacity of the dragged pop-out");
        }
    }
}

fn pause() {
    std::thread::sleep(Duration::from_millis(SAMPLE_MS));
}

/// The follower: runs the core's loop; whatever ends it (also a panic of the probe) gives the window its opacity back,
/// then tells the host the window is opaque (unless it heard so already), and only then forgets the window (`done`), so a new drag cannot start a second
/// follower before the host heard the end of the first.
fn follow<W, P>(
    mut probe: P,
    window: W,
    env: Environment,
    opacity: Opacity,
    pause: impl FnMut(),
    done: impl FnOnce(),
) where
    W: WindowOpacity,
    P: Probe,
{
    let mut unsupported_logged = false;
    let mut ctl = Controller::new(window, env, opacity, move |e| {
        log_problem(&mut unsupported_logged, e);
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        track(&mut ctl, &mut probe, pause);
    }));
    // Opaque again (the controller's drop guard), then the host hears it, then the window is forgotten.
    let last = ctl.applied();
    drop(ctl);
    if last != Appearance::Opaque {
        probe.announce(Appearance::Opaque);
    }
    done();
    if result.is_err() {
        log::error!(target: TARGET, "The pop-out drag follower failed; the window was made opaque again");
    }
}

#[cfg(test)]
mod follow_tests {
    use super::*;
    use crate::wire::Look;
    use std::sync::{Arc, Mutex};

    /// Everything the follower does, in order.
    type Log = Arc<Mutex<Vec<String>>>;

    struct FakeWindow(Log);

    impl WindowOpacity for FakeWindow {
        fn set_opacity(&mut self, opacity: f64) -> Result<(), OpacityError> {
            self.0.lock().unwrap().push(format!("opacity {opacity}"));
            Ok(())
        }
    }

    struct Script {
        samples: std::vec::IntoIter<Option<Sample>>,
        log: Log,
    }

    impl Probe for Script {
        fn sample(&mut self) -> Sample {
            match self.samples.next().expect("the script ran out") {
                Some(s) => s,
                None => panic!("the probe failed"),
            }
        }
        fn announce(&mut self, look: Appearance) {
            let look: Look = look.look();
            self.log.lock().unwrap().push(format!("announce {look:?}"));
        }
    }

    fn s(button: bool, over: bool) -> Option<Sample> {
        Some(Sample {
            button_down: Some(button),
            over_host: over,
            windows_alive: true,
            since_move_ms: 0,
        })
    }

    fn gone() -> Option<Sample> {
        Some(Sample {
            button_down: Some(true),
            over_host: true,
            windows_alive: false,
            since_move_ms: 0,
        })
    }

    fn run(samples: Vec<Option<Sample>>, env: Environment) -> Vec<String> {
        let log: Log = Arc::default();
        let probe = Script {
            samples: samples.into_iter(),
            log: log.clone(),
        };
        let done_log = log.clone();
        follow(
            probe,
            FakeWindow(log.clone()),
            env,
            Opacity::default(),
            || {},
            move || {
                done_log.lock().unwrap().push("done".into());
            },
        );
        std::mem::take(&mut *log.lock().unwrap())
    }

    #[test]
    fn a_drop_dims_then_restores_announces_in_order_and_forgets_last() {
        let log = run(
            vec![s(true, true), s(true, true), s(false, true), s(false, true)],
            Environment::default(),
        );
        assert_eq!(
            log,
            [
                "opacity 0.5",
                "announce Translucent",
                "opacity 1",
                "announce Opaque",
                "done"
            ]
        );
    }

    #[test]
    fn a_closing_window_restores_and_ends() {
        let log = run(vec![s(true, true), gone()], Environment::default());
        assert_eq!(
            log,
            [
                "opacity 0.5",
                "announce Translucent",
                "opacity 1",
                "announce Opaque",
                "done"
            ]
        );
    }

    #[test]
    fn a_panicking_probe_restores_announces_once_and_forgets_the_window() {
        let log = run(vec![s(true, true), None], Environment::default());
        assert_eq!(
            log,
            [
                "opacity 0.5",
                "announce Translucent",
                "opacity 1",
                "announce Opaque",
                "done"
            ]
        );
    }

    #[test]
    fn a_probe_that_fails_before_anything_dimmed_only_forgets_the_window() {
        let log = run(vec![None], Environment::default());
        assert_eq!(log, ["done"]);
    }

    #[test]
    fn reduced_transparency_outlines_without_touching_the_window() {
        let log = run(
            vec![s(true, true), s(false, true), s(false, true)],
            Environment {
                reduce_transparency: true,
            },
        );
        assert_eq!(log, ["announce Outline", "announce Opaque", "done"]);
    }

    #[test]
    fn leaving_the_host_is_announced_opaque_once() {
        let log = run(
            vec![
                s(true, true),
                s(true, false),
                s(false, false),
                s(false, false),
            ],
            Environment::default(),
        );
        assert_eq!(
            log,
            [
                "opacity 0.5",
                "announce Translucent",
                "opacity 1",
                "announce Opaque",
                "done"
            ]
        );
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
        assert!(!default.redock_windows, "Windows stays off until verified");
        assert!(
            crate::tauri::Panels::builder()
                .windows_translucency(true)
                .build()
                .redock_windows
        );
    }
}

#[cfg(test)]
mod mock_tests {
    use super::*;
    use crate::redock::{Appearance, Probe, WindowOpacity};
    use crate::tauri::tests::{app, app_with, recorder, window};
    use crate::wire::PANELS_EVENT;
    use ::tauri::Manager;
    use ::tauri::test::MockRuntime;
    use serde_json::json;
    use std::time::Instant;

    fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::yield_now();
        }
    }

    fn live(app: &::tauri::App<MockRuntime>) -> Live<MockRuntime> {
        Live {
            app: app.handle().clone(),
            host: "main".into(),
            label: "p-main-1".into(),
            panel: "pn.code".into(),
        }
    }

    #[test]
    fn the_opacity_call_reports_a_missing_window_and_never_panics_on_a_present_one() {
        let app = app();
        let mut gone = TauriOpacity {
            app: app.handle().clone(),
            label: "p-main-9".into(),
            windows_translucency: true,
        };
        assert!(matches!(
            gone.set_opacity(0.5),
            Err(OpacityError::Failed(_))
        ));
        let _ = window(&app, "p-main-1");
        let mut present = TauriOpacity {
            app: app.handle().clone(),
            label: "p-main-1".into(),
            windows_translucency: true,
        };
        assert_eq!(present.set_opacity(0.5), Ok(()));
        assert_eq!(present.set_opacity(1.0), Ok(()));
    }

    #[test]
    fn problems_are_logged_without_failing() {
        let mut logged = false;
        log_problem(
            &mut logged,
            &OpacityError::Unsupported("no compositor".into()),
        );
        assert!(logged);
        log_problem(
            &mut logged,
            &OpacityError::Unsupported("no compositor".into()),
        );
        log_problem(&mut logged, &OpacityError::Failed("busy".into()));
    }

    #[test]
    fn windows_stay_opaque_until_the_app_enables_translucency() {
        let app = app();
        let _ = window(&app, "p-main-1");
        let mut off = TauriOpacity {
            app: app.handle().clone(),
            label: "p-main-1".into(),
            windows_translucency: false,
        };
        let r = off.set_opacity(0.5);
        if cfg!(windows) {
            assert!(matches!(r, Err(OpacityError::Unsupported(_))));
        }
        // Restoring is never gated.
        let _ = off.set_opacity(1.0);
    }

    #[test]
    fn a_sample_knows_whether_the_windows_and_the_follower_are_alive() {
        let app = app();
        let mut l = live(&app);
        assert!(!l.sample().windows_alive, "no pop-out window");
        let _ = window(&app, "p-main-1");
        assert!(!l.sample().windows_alive, "no follower registered");
        app.state::<Panels>().redock.begin("p-main-1");
        let s = l.sample();
        assert!(s.windows_alive);
        assert!(s.since_move_ms < 10_000);
        let mut other_host = Live {
            host: "w-9".into(),
            ..live(&app)
        };
        assert!(!other_host.sample().windows_alive, "no host window");
    }

    #[test]
    fn announcing_tells_the_host_the_look() {
        let app = app();
        let seen = recorder(&window(&app, "main"), PANELS_EVENT);
        let mut l = live(&app);
        l.announce(Appearance::Outline);
        l.announce(Appearance::Opaque);
        wait_for("both events", || seen.lock().unwrap().len() == 2);
        let seen = seen.lock().unwrap().clone();
        assert_eq!(
            seen[0],
            json!({ "kind": "redock", "panel": "pn.code", "label": "p-main-1", "look": "outline" })
        );
        assert_eq!(seen[1]["look"], "opaque");
    }

    #[test]
    fn a_drag_that_is_not_over_the_host_with_the_button_held_starts_nothing() {
        let app = app();
        let h = app.handle();
        let _ = window(&app, "p-main-1");
        for world in [
            (false, true, Some(true)),
            (true, false, Some(true)),
            (true, true, Some(false)),
        ] {
            start(h, "main", "p-main-1", "pn.code", world);
            assert!(
                app.state::<Panels>()
                    .redock
                    .since_move("p-main-1")
                    .is_none(),
                "{world:?}"
            );
        }
        // No managed state: nothing to do.
        let bare = app_with(None);
        start(
            bare.handle(),
            "main",
            "p-main-1",
            "pn.code",
            (true, true, Some(true)),
        );
        on_moved(bare.handle(), "main", "p-main-1", "pn.code");
    }

    #[test]
    fn a_drag_over_the_host_is_followed_until_its_window_goes_and_then_forgotten() {
        let app = app();
        let h = app.handle();
        let _ = window(&app, "p-main-1");
        let seen = recorder(&window(&app, "main"), PANELS_EVENT);
        start(h, "main", "p-main-1", "pn.code", (true, true, Some(true)));
        assert!(
            app.state::<Panels>()
                .redock
                .since_move("p-main-1")
                .is_some(),
            "followed"
        );
        // A second move of the same window only refreshes the stamp (no second follower).
        on_moved(h, "main", "p-main-1", "pn.code");
        // The window goes: the follower ends, announces nothing it had not announced, and forgets the window.
        window(&app, "p-main-1").destroy().unwrap();
        wait_for("the follower to end", || {
            app.state::<Panels>()
                .redock
                .since_move("p-main-1")
                .is_none()
        });
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .all(|e| e["kind"] != "redock" || e["look"] == "opaque")
        );
    }
}
