//! The pop-out being dragged back over its host window turns translucent, so the dock guides under it show (like the
//! floating tool windows of Visual Studio and JetBrains IDEs). This is the pure core: from the drag's state and where the
//! pointer is it decides how the window looks ([`desired`]), and [`Controller`] applies that to a window through the
//! [`WindowOpacity`] port, so every way a drag ends gives the window its full opacity back (also when the controller is
//! dropped). The adapter (`tauri` feature) implements the port per OS and feeds the controller.
//!
//! The wire form is [`Look`], sent to the host as `PanelsEvent::Redock` so the dock can follow along (and draw the
//! outline guides when the user asked the system to reduce transparency).

use crate::wire::Look;

/// Opacity of the dragged window over its host when the app sets none.
pub const DEFAULT_OPACITY: f64 = 0.5;
/// Lowest opacity an app can ask for (below this the panel cannot be told from the guides).
pub const MIN_OPACITY: f64 = 0.25;
/// Highest opacity an app can ask for (above this the guides are hardly visible through it).
pub const MAX_OPACITY: f64 = 0.9;

/// How opaque the dragged window is over its host: a number in `MIN_OPACITY..=MAX_OPACITY`, clamped on construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opacity(f64);

impl Opacity {
    /// `value` clamped to the allowed range; NaN gives the default.
    pub fn new(value: f64) -> Self {
        if value.is_nan() {
            return Self::default();
        }
        Opacity(value.clamp(MIN_OPACITY, MAX_OPACITY))
    }

    /// The opacity as a fraction, 0.25 to 0.9.
    pub fn get(self) -> f64 {
        self.0
    }
}

impl Default for Opacity {
    fn default() -> Self {
        Opacity(DEFAULT_OPACITY)
    }
}

/// How a dragged pop-out window looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Appearance {
    /// Fully opaque (the normal state).
    Opaque,
    /// Translucent at this opacity, over the host window.
    Translucent(Opacity),
    /// Opaque, but the host draws a clear outline on the dock guides (the user reduced transparency in the OS).
    Outline,
}

impl Appearance {
    /// The wire form of the look the host is told about.
    pub fn look(self) -> Look {
        match self {
            Appearance::Opaque => Look::Opaque,
            Appearance::Translucent(_) => Look::Translucent,
            Appearance::Outline => Look::Outline,
        }
    }

    /// The opacity the window itself must have (1.0 unless translucent).
    pub fn window_opacity(self) -> f64 {
        match self {
            Appearance::Translucent(o) => o.get(),
            Appearance::Opaque | Appearance::Outline => 1.0,
        }
    }
}

/// Why a drag is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndCause {
    /// The button was released (a drop, or Escape, which ends the OS move).
    Released,
    /// The pop-out window closed, or its host did.
    WindowGone,
    /// The layer could not follow the drag any more (a failed probe, a panic).
    Failed,
}

/// What the layer knows about the drag now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragState {
    /// The window is being dragged; `over_host`: the pointer is over the host window.
    Active { over_host: bool },
    /// The drag is over.
    Ended(EndCause),
}

/// What the user's system asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Environment {
    /// "Reduce transparency" (macOS), transparency effects off (Windows).
    pub reduce_transparency: bool,
}

/// The look the dragged window should have: translucent only while the drag is active and over the host, and an outline
/// instead when the system reduces transparency. Every other state, in particular every end, is opaque.
pub fn desired(state: DragState, env: Environment, opacity: Opacity) -> Appearance {
    match state {
        DragState::Active { over_host: true } if env.reduce_transparency => Appearance::Outline,
        DragState::Active { over_host: true } => Appearance::Translucent(opacity),
        DragState::Active { over_host: false } | DragState::Ended(_) => Appearance::Opaque,
    }
}

/// Why the platform did not change a window's opacity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpacityError {
    /// The platform or the compositor cannot do it (no compositing on Linux, say): not worth another try.
    Unsupported(String),
    /// This try failed (the window is gone): the next one may work.
    Failed(String),
}

/// The platform call: sets a window's opacity (1.0 is fully opaque).
pub trait WindowOpacity {
    /// Applies `opacity`, a number from 0.0 to 1.0.
    fn set_opacity(&mut self, opacity: f64) -> Result<(), OpacityError>;
}

/// Applies [`desired`] looks to one window and gives it back its full opacity when dropped.
pub struct Controller<W: WindowOpacity> {
    window: W,
    env: Environment,
    opacity: Opacity,
    applied: Appearance,
    /// The window's opacity is not 1.0 (a restore is due).
    dimmed: bool,
    unsupported: bool,
    on_problem: Box<dyn FnMut(&OpacityError) + Send>,
}

impl<W: WindowOpacity> Controller<W> {
    /// Controls `window`; `on_problem` hears of errors (the adapter logs them: an unsupported platform once).
    pub fn new(
        window: W,
        env: Environment,
        opacity: Opacity,
        on_problem: impl FnMut(&OpacityError) + Send + 'static,
    ) -> Self {
        Controller {
            window,
            env,
            opacity,
            applied: Appearance::Opaque,
            dimmed: false,
            unsupported: false,
            on_problem: Box::new(on_problem),
        }
    }

    /// The look now.
    pub fn applied(&self) -> Appearance {
        self.applied
    }

    /// Moves the window to the look `state` calls for. Returns the new look when it changed. A platform that cannot dim
    /// windows keeps the window opaque and gives the outline instead, so the user still sees the guides.
    pub fn update(&mut self, state: DragState) -> Option<Appearance> {
        let mut want = desired(state, self.env, self.opacity);
        if self.unsupported && matches!(want, Appearance::Translucent(_)) {
            want = Appearance::Outline;
        }
        if want == self.applied {
            return None;
        }
        if matches!(want, Appearance::Translucent(_)) || self.dimmed {
            self.apply(want.window_opacity());
        }
        // An unsupported platform answered while applying.
        if self.unsupported && matches!(want, Appearance::Translucent(_)) {
            want = Appearance::Outline;
        }
        if want == self.applied {
            return None;
        }
        self.applied = want;
        Some(want)
    }

    /// Ends the drag: the window is opaque again.
    pub fn end(&mut self, cause: EndCause) -> Option<Appearance> {
        self.update(DragState::Ended(cause))
    }

    fn apply(&mut self, opacity: f64) {
        if self.unsupported {
            return;
        }
        match self.window.set_opacity(opacity) {
            Ok(()) => self.dimmed = opacity < 1.0,
            Err(e) => {
                (self.on_problem)(&e);
                match e {
                    OpacityError::Unsupported(_) => self.unsupported = true,
                    // A restore that failed is tried again by the drop guard.
                    OpacityError::Failed(_) => {}
                }
            }
        }
    }
}

impl<W: WindowOpacity> Drop for Controller<W> {
    fn drop(&mut self) {
        if self.dimmed {
            self.apply(1.0);
        }
    }
}

/// How often the drag is sampled while a pop-out window is over its host (about 30 Hz).
pub const SAMPLE_MS: u64 = 33;
/// Without a readable mouse button (Linux), the drag is over when the window has not moved for this long.
pub const SETTLE_MS: u64 = 500;
/// Samples with the button up in a row that end a drag (one stray reading is not an end).
const UP_SAMPLES: u32 = 2;
/// A drag is followed for at most this many samples (10 minutes): nothing unbounded, a stuck watcher gives the window back.
pub const MAX_SAMPLES: u64 = 10 * 60 * 1000 / SAMPLE_MS;

/// An axis-aligned rectangle in physical screen px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Is the pointer over `host`? False for anything not finite.
pub fn pointer_over(host: Bounds, x: f64, y: f64) -> bool {
    [host.x, host.y, host.width, host.height, x, y]
        .iter()
        .all(|v| v.is_finite())
        && x >= host.x
        && y >= host.y
        && x < host.x + host.width
        && y < host.y + host.height
}

/// Should a window move start the follow-up: the primary button is held (or cannot be read) and the pointer is over the
/// host, and none is running for this window.
pub fn should_start(button_down: Option<bool>, over_host: bool, tracking: bool) -> bool {
    !tracking && over_host && button_down != Some(false)
}

/// One look at the world while a pop-out window is dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// The primary mouse button; `None` when the platform cannot tell.
    pub button_down: Option<bool>,
    pub over_host: bool,
    /// The pop-out window and its host still exist.
    pub windows_alive: bool,
    /// Time since the window last reported a move.
    pub since_move_ms: u64,
}

/// Turns samples into the drag's state (debounces the button, settles where the button cannot be read).
#[derive(Debug, Default)]
pub struct Tracker {
    up: u32,
    samples: u64,
}

impl Tracker {
    pub fn next(&mut self, s: Sample) -> DragState {
        self.samples += 1;
        if !s.windows_alive {
            return DragState::Ended(EndCause::WindowGone);
        }
        if self.samples > MAX_SAMPLES {
            return DragState::Ended(EndCause::Failed);
        }
        match s.button_down {
            Some(false) => {
                self.up += 1;
                if self.up >= UP_SAMPLES {
                    return DragState::Ended(EndCause::Released);
                }
            }
            Some(true) => self.up = 0,
            None if s.since_move_ms > SETTLE_MS => return DragState::Ended(EndCause::Released),
            None => {}
        }
        DragState::Active {
            over_host: s.over_host,
        }
    }
}

/// What [`track`] reads and tells.
pub trait Probe {
    /// Looks at the drag now.
    fn sample(&mut self) -> Sample;
    /// The look changed (the host is told so its dock can follow).
    fn announce(&mut self, look: Appearance);
}

/// Follows one drag until it ends: samples, moves the window to the look called for, announces changes, and ends with the
/// window opaque (the controller restores it when dropped too). `pause` waits [`SAMPLE_MS`].
pub fn track<W: WindowOpacity, P: Probe>(
    ctl: &mut Controller<W>,
    probe: &mut P,
    mut pause: impl FnMut(),
) {
    let mut tracker = Tracker::default();
    loop {
        let state = tracker.next(probe.sample());
        if let Some(look) = ctl.update(state) {
            probe.announce(look);
        }
        // Already opaque by `update`; a failed restore is retried by the controller's drop guard.
        if matches!(state, DragState::Ended(_)) {
            return;
        }
        pause();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Fake {
        calls: Arc<Mutex<Vec<f64>>>,
        fail: Arc<Mutex<Option<OpacityError>>>,
    }

    impl Fake {
        fn calls(&self) -> Vec<f64> {
            self.calls.lock().unwrap().clone()
        }
        fn failing(&self, e: Option<OpacityError>) {
            *self.fail.lock().unwrap() = e;
        }
    }

    impl WindowOpacity for Fake {
        fn set_opacity(&mut self, opacity: f64) -> Result<(), OpacityError> {
            if let Some(e) = self.fail.lock().unwrap().clone() {
                return Err(e);
            }
            self.calls.lock().unwrap().push(opacity);
            Ok(())
        }
    }

    const OVER: DragState = DragState::Active { over_host: true };
    const AWAY: DragState = DragState::Active { over_host: false };

    fn ctl(fake: &Fake, env: Environment) -> (Controller<Fake>, Arc<Mutex<Vec<OpacityError>>>) {
        let problems = Arc::new(Mutex::new(Vec::new()));
        let p = problems.clone();
        let c = Controller::new(fake.clone(), env, Opacity::default(), move |e| {
            p.lock().unwrap().push(e.clone())
        });
        (c, problems)
    }

    #[test]
    fn opacity_is_clamped_and_defaults() {
        assert_eq!(Opacity::new(0.0).get(), MIN_OPACITY);
        assert_eq!(Opacity::new(1.0).get(), MAX_OPACITY);
        assert_eq!(Opacity::new(0.6).get(), 0.6);
        assert_eq!(Opacity::new(f64::NAN).get(), DEFAULT_OPACITY);
        assert_eq!(Opacity::new(f64::INFINITY).get(), MAX_OPACITY);
        assert_eq!(Opacity::default().get(), DEFAULT_OPACITY);
    }

    #[test]
    fn desired_is_translucent_only_over_the_host_while_dragging() {
        let o = Opacity::default();
        let env = Environment::default();
        assert_eq!(desired(OVER, env, o), Appearance::Translucent(o));
        assert_eq!(desired(AWAY, env, o), Appearance::Opaque);
        for cause in [EndCause::Released, EndCause::WindowGone, EndCause::Failed] {
            assert_eq!(desired(DragState::Ended(cause), env, o), Appearance::Opaque);
        }
    }

    #[test]
    fn reduced_transparency_gives_an_outline_not_a_dimmed_window() {
        let env = Environment {
            reduce_transparency: true,
        };
        assert_eq!(desired(OVER, env, Opacity::default()), Appearance::Outline);
        assert_eq!(desired(AWAY, env, Opacity::default()), Appearance::Opaque);
        let fake = Fake::default();
        let (mut c, _) = ctl(&fake, env);
        assert_eq!(c.update(OVER), Some(Appearance::Outline));
        assert!(fake.calls().is_empty());
        assert_eq!(c.end(EndCause::Released), Some(Appearance::Opaque));
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn looks_and_window_opacity() {
        assert_eq!(Appearance::Opaque.look(), Look::Opaque);
        assert_eq!(
            Appearance::Translucent(Opacity::default()).look(),
            Look::Translucent
        );
        assert_eq!(Appearance::Outline.look(), Look::Outline);
        assert_eq!(Appearance::Outline.window_opacity(), 1.0);
        assert_eq!(
            Appearance::Translucent(Opacity::new(0.4)).window_opacity(),
            0.4
        );
    }

    #[test]
    fn enters_and_leaves_the_host() {
        let fake = Fake::default();
        let (mut c, _) = ctl(&fake, Environment::default());
        assert_eq!(c.update(AWAY), None);
        assert!(
            fake.calls().is_empty(),
            "nothing to do while it is not over the host"
        );
        assert_eq!(
            c.update(OVER),
            Some(Appearance::Translucent(Opacity::default()))
        );
        assert_eq!(c.update(OVER), None, "no repeated call");
        assert_eq!(c.update(AWAY), Some(Appearance::Opaque));
        assert_eq!(fake.calls(), [0.5, 1.0]);
    }

    #[test]
    fn every_end_path_restores() {
        for cause in [EndCause::Released, EndCause::WindowGone, EndCause::Failed] {
            let fake = Fake::default();
            let (mut c, _) = ctl(&fake, Environment::default());
            c.update(OVER);
            assert_eq!(c.end(cause), Some(Appearance::Opaque));
            assert_eq!(fake.calls().last(), Some(&1.0), "{cause:?}");
            drop(c);
            assert_eq!(fake.calls(), [0.5, 1.0], "{cause:?}: restored once");
        }
    }

    #[test]
    fn dropping_a_dimmed_controller_restores() {
        let fake = Fake::default();
        let (mut c, _) = ctl(&fake, Environment::default());
        c.update(OVER);
        drop(c);
        assert_eq!(fake.calls(), [0.5, 1.0]);
    }

    #[test]
    fn dropping_during_a_panic_restores() {
        let fake = Fake::default();
        let f = fake.clone();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let (mut c, _) = ctl(&f, Environment::default());
            c.update(OVER);
            panic!("tracker bug");
        }));
        assert!(r.is_err());
        assert_eq!(fake.calls(), [0.5, 1.0]);
    }

    #[test]
    fn a_never_dimmed_controller_makes_no_call_on_drop() {
        let fake = Fake::default();
        let (c, _) = ctl(&fake, Environment::default());
        drop(c);
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn unsupported_platform_stays_opaque_with_an_outline_and_reports_once() {
        let fake = Fake::default();
        fake.failing(Some(OpacityError::Unsupported("no compositor".into())));
        let (mut c, problems) = ctl(&fake, Environment::default());
        assert_eq!(c.update(OVER), Some(Appearance::Outline));
        assert_eq!(c.update(AWAY), Some(Appearance::Opaque));
        assert_eq!(c.update(OVER), Some(Appearance::Outline));
        assert_eq!(c.end(EndCause::Released), Some(Appearance::Opaque));
        drop(c);
        assert_eq!(problems.lock().unwrap().len(), 1, "logged once");
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_failed_try_is_reported_and_the_next_one_works() {
        let fake = Fake::default();
        fake.failing(Some(OpacityError::Failed("window busy".into())));
        let (mut c, problems) = ctl(&fake, Environment::default());
        // The call failed: the look is reported as asked (the window may still change), the failure is heard.
        c.update(OVER);
        assert_eq!(problems.lock().unwrap().len(), 1);
        fake.failing(None);
        c.update(AWAY);
        c.update(OVER);
        assert_eq!(fake.calls(), [0.5]);
        drop(c);
        assert_eq!(fake.calls(), [0.5, 1.0]);
    }

    #[test]
    fn a_failed_restore_is_tried_again_on_drop() {
        let fake = Fake::default();
        let (mut c, problems) = ctl(&fake, Environment::default());
        c.update(OVER);
        fake.failing(Some(OpacityError::Failed("gone".into())));
        c.end(EndCause::WindowGone);
        assert_eq!(problems.lock().unwrap().len(), 1);
        fake.failing(None);
        drop(c);
        assert_eq!(fake.calls(), [0.5, 1.0]);
    }

    struct Script {
        samples: std::vec::IntoIter<Sample>,
        looks: Vec<Look>,
    }

    impl Probe for Script {
        fn sample(&mut self) -> Sample {
            self.samples.next().expect("the script ended the drag")
        }
        fn announce(&mut self, look: Appearance) {
            self.looks.push(look.look());
        }
    }

    fn sample(button: Option<bool>, over: bool) -> Sample {
        Sample {
            button_down: button,
            over_host: over,
            windows_alive: true,
            since_move_ms: 0,
        }
    }

    fn run(samples: Vec<Sample>, env: Environment) -> (Vec<Look>, Vec<f64>) {
        let fake = Fake::default();
        let (mut c, _) = ctl(&fake, env);
        let mut probe = Script {
            samples: samples.into_iter(),
            looks: vec![],
        };
        track(&mut c, &mut probe, || {});
        drop(c);
        (probe.looks, fake.calls())
    }

    #[test]
    fn a_drag_over_the_host_and_a_drop_dims_then_restores() {
        let (looks, calls) = run(
            vec![
                sample(Some(true), false),
                sample(Some(true), true),
                sample(Some(true), true),
                sample(Some(false), true),
                sample(Some(false), true),
            ],
            Environment::default(),
        );
        assert_eq!(looks, [Look::Translucent, Look::Opaque]);
        assert_eq!(calls, [0.5, 1.0]);
    }

    #[test]
    fn leaving_the_host_and_coming_back() {
        let (looks, calls) = run(
            vec![
                sample(Some(true), true),
                sample(Some(true), false),
                sample(Some(true), true),
                sample(Some(false), false),
                sample(Some(false), false),
            ],
            Environment::default(),
        );
        assert_eq!(
            looks,
            [
                Look::Translucent,
                Look::Opaque,
                Look::Translucent,
                Look::Opaque
            ]
        );
        assert_eq!(calls, [0.5, 1.0, 0.5, 1.0]);
    }

    #[test]
    fn one_stray_button_reading_does_not_end_the_drag() {
        let (looks, _) = run(
            vec![
                sample(Some(true), true),
                sample(Some(false), true),
                sample(Some(true), true),
                sample(Some(false), true),
                sample(Some(false), true),
            ],
            Environment::default(),
        );
        assert_eq!(looks, [Look::Translucent, Look::Opaque]);
    }

    #[test]
    fn a_closing_window_restores_and_ends() {
        let gone = Sample {
            windows_alive: false,
            ..sample(Some(true), true)
        };
        let (looks, calls) = run(vec![sample(Some(true), true), gone], Environment::default());
        assert_eq!(looks, [Look::Translucent, Look::Opaque]);
        assert_eq!(calls, [0.5, 1.0]);
    }

    #[test]
    fn without_a_readable_button_the_drag_settles_when_the_window_stops_moving() {
        let idle = Sample {
            since_move_ms: SETTLE_MS + 1,
            ..sample(None, true)
        };
        let (looks, calls) = run(
            vec![sample(None, true), sample(None, true), idle],
            Environment::default(),
        );
        assert_eq!(looks, [Look::Translucent, Look::Opaque]);
        assert_eq!(calls, [0.5, 1.0]);
    }

    #[test]
    fn a_stuck_drag_is_given_up() {
        let mut t = Tracker::default();
        let s = sample(Some(true), true);
        for _ in 0..MAX_SAMPLES {
            assert_eq!(t.next(s), DragState::Active { over_host: true });
        }
        assert_eq!(t.next(s), DragState::Ended(EndCause::Failed));
    }

    #[test]
    fn reduced_transparency_tracks_with_an_outline_only() {
        let (looks, calls) = run(
            vec![
                sample(Some(true), true),
                sample(Some(false), true),
                sample(Some(false), true),
            ],
            Environment {
                reduce_transparency: true,
            },
        );
        assert_eq!(looks, [Look::Outline, Look::Opaque]);
        assert!(calls.is_empty());
    }

    #[test]
    fn pointer_over_the_host() {
        let host = Bounds {
            x: 100.0,
            y: 50.0,
            width: 400.0,
            height: 300.0,
        };
        assert!(pointer_over(host, 100.0, 50.0));
        assert!(pointer_over(host, 499.9, 349.9));
        assert!(!pointer_over(host, 500.0, 100.0));
        assert!(!pointer_over(host, 99.0, 100.0));
        assert!(!pointer_over(host, f64::NAN, 100.0));
        assert!(!pointer_over(
            Bounds {
                width: f64::INFINITY,
                ..host
            },
            200.0,
            100.0
        ));
    }

    #[test]
    fn a_follow_up_starts_once_over_the_host_with_the_button_held_or_unreadable() {
        assert!(should_start(Some(true), true, false));
        assert!(should_start(None, true, false));
        assert!(
            !should_start(Some(false), true, false),
            "moved without the button: snapping, not a drag"
        );
        assert!(!should_start(Some(true), false, false));
        assert!(!should_start(Some(true), true, true));
    }
}
