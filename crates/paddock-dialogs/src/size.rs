//! The size of a dialog window as one pure state machine (ADR 0043 §3): what the window should be at any moment, decided
//! from the kind's [`SizeMode`], the latest measure the page sent, the monitor's work area, the window's visible frame
//! and its scale, and nothing else. It does no I/O: the Tauri glue feeds it events on the main thread and carries out
//! the [`Plan`] each transition returns.
//!
//! * Measures arrive as `(epoch, seq, measure)` in **logical px** (the glue has already applied the page's zoom). A lower
//!   `seq` in the same epoch changes nothing; a new epoch (a reloaded page) is accepted and restarts the order.
//! * Rust asks the OS for a **physical** inner size, rounded up once (`ceil(logical * scale)`). Every request is queued; a
//!   `Resized` equal to a queued request (within one physical px a side) takes it and every older one off the queue, so an
//!   echo of our own `set_size` is never mistaken for the user.
//! * Only [`SizeState::on_user_resize_started`] (the OS says "the user is resizing": `WM_SIZING`, live resize), or an
//!   unexplained `Resized` on a platform that has no such signal, makes the window [`SizeMode::Manual`] for the rest of
//!   the opening. A move never does.
//! * The window is shown once the measure named by `dialog_ready` has been applied and its `Resized` has arrived, or at
//!   the show deadline.

use crate::contract::{CAP_FRACTION, Shrink, Size, SizeMode};
use std::collections::VecDeque;

/// A page measure in logical px, as the glue hands it over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    /// What the content needs at the window's width: header, body at its natural height, footer.
    pub content: Size,
    /// The least it can be: the footer on one line, the body's declared minimum.
    pub min_content: Size,
    /// The body scrolls (`scrollHeight > clientHeight + 1`); logged, never used to size.
    pub overflow: bool,
}

/// The monitor and frame a plan is made for, logical px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Env {
    /// The window's scale factor (device px per logical px).
    pub scale: f64,
    /// The monitor's work area.
    pub work: Size,
    /// The visible frame (title bar and borders): outer minus inner, without the invisible resize borders.
    pub frame: Size,
}

impl Env {
    /// The most the inner size can be on this monitor: the work area less the visible frame.
    pub fn avail(&self) -> Size {
        Size {
            width: (self.work.width - self.frame.width).max(1.0),
            height: (self.work.height - self.frame.height).max(1.0),
        }
    }
}

/// A physical inner size Rust asked the OS for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub width: u32,
    pub height: u32,
}

impl Request {
    fn matches(self, width: u32, height: u32) -> bool {
        self.width.abs_diff(width) <= 1 && self.height.abs_diff(height) <= 1
    }
}

/// What the glue does after a transition. Empty means nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// Set the inner size to this (physical px); `inner` is the logical size it was rounded from.
    pub request: Option<Request>,
    /// The logical inner size asked for.
    pub inner: Option<Size>,
    /// The new OS minimum, logical px.
    pub min: Option<Size>,
    /// The mode changed to this (the page is told with `dialog://size-mode`).
    pub mode_change: Option<SizeMode>,
    /// Show the window now.
    pub show: bool,
    /// Post a refit with `run_on_main_thread` (it must run after the OS's own resize for a scale change).
    pub refit_posted: bool,
    /// Tell the page to measure again (`dialog://remeasure`).
    pub remeasure: bool,
    /// A `Resized` that matches no request and was not a user resize (logged with the queue).
    pub unexplained: bool,
    /// The measure was older than one already applied.
    pub stale: bool,
}

/// Why a window was shown by the deadline and not by its ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// No ready arrived, or no measure.
    Measure,
    /// The ready's measure was not the one applied yet.
    NotApplied,
    /// The size was asked for but no `Resized` arrived.
    NoResized,
    /// Nothing was missing: it was already shown.
    Nothing,
}

/// The state of one dialog window's size.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeState {
    mode: SizeMode,
    floor: Size,
    preferred_floor: f64,
    infer_user_resize: bool,
    env: Env,
    epoch: u64,
    last_seq: Option<u64>,
    last: Option<Measured>,
    ready: Option<(u64, u64)>,
    pending: VecDeque<Request>,
    current: Request,
    min: Size,
    shown: bool,
}

/// `ceil(logical * scale)` physical px, as the placement rounds it ([`crate::placement::physical_ceil`]).
pub fn physical(logical: f64, scale: f64) -> u32 {
    crate::placement::physical_ceil(logical, scale)
}

impl SizeState {
    /// A window of a kind with `mode`, never smaller than `floor`, whose height is at least `preferred_floor` when it
    /// follows its content (a viewer's 560), created at the physical inner size `created`. `infer_user_resize` is true
    /// where the OS gives no resize signal (X11, Wayland): an unexplained `Resized` then means the user.
    pub fn new(
        mode: SizeMode,
        floor: Size,
        preferred_floor: f64,
        infer_user_resize: bool,
        env: Env,
        created: Request,
    ) -> Self {
        SizeState {
            mode,
            floor,
            preferred_floor,
            infer_user_resize,
            env,
            epoch: 0,
            last_seq: None,
            last: None,
            ready: None,
            pending: VecDeque::new(),
            current: created,
            min: floor,
            shown: false,
        }
    }

    /// The mode now (a user resize turns `Height` into `Manual`).
    pub fn mode(&self) -> SizeMode {
        self.mode
    }

    /// The window has been shown.
    pub fn is_shown(&self) -> bool {
        self.shown
    }

    /// Requests not yet echoed by a `Resized`.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// The OS minimum now, logical px.
    pub fn min(&self) -> Size {
        self.min
    }

    /// The newest request still waiting for its `Resized`.
    pub fn last_request(&self) -> Option<Request> {
        self.pending.back().copied()
    }

    /// The inner size the window has or will have once every queued request is applied, logical px.
    pub fn effective(&self) -> Size {
        let r = self.pending.back().copied().unwrap_or(self.current);
        Size {
            width: f64::from(r.width) / self.env.scale,
            height: f64::from(r.height) / self.env.scale,
        }
    }

    fn min_for(&self, m: &Measured) -> Size {
        let avail = self.env.avail();
        Size {
            width: m.min_content.width.max(self.floor.width).min(avail.width),
            height: m
                .min_content
                .height
                .max(self.floor.height)
                .min(avail.height),
        }
    }

    /// The logical inner size the window should have for `m`, now.
    fn target(&self, m: &Measured, min: Size) -> Size {
        let avail = self.env.avail();
        let now = self.effective();
        let width = now.width.max(min.width).min(avail.width);
        let height = match self.mode {
            SizeMode::Manual => now.height.max(min.height).min(avail.height),
            SizeMode::Height { shrink } => {
                let cap = self.env.work.height * CAP_FRACTION - self.env.frame.height;
                let fitted = m
                    .content
                    .height
                    .ceil()
                    .max(self.preferred_floor)
                    .min(cap)
                    .max(min.height);
                let fitted = if shrink == Shrink::Never {
                    fitted.max(now.height)
                } else {
                    fitted
                };
                fitted.min(avail.height)
            }
        };
        Size { width, height }
    }

    /// Asks the OS for `size` unless it is the size the window has or will have.
    fn ask(&mut self, plan: &mut Plan, size: Size) {
        let request = Request {
            width: physical(size.width, self.env.scale),
            height: physical(size.height, self.env.scale),
        };
        let have = self.pending.back().copied().unwrap_or(self.current);
        if request != have {
            self.pending.push_back(request);
            plan.request = Some(request);
            plan.inner = Some(size);
        }
    }

    fn refit(&mut self, plan: &mut Plan) {
        if let Some(m) = self.last {
            let min = self.min_for(&m);
            if min != self.min {
                self.min = min;
                plan.min = Some(min);
            }
            let size = self.target(&m, min);
            self.ask(plan, size);
        }
    }

    fn try_show(&mut self) -> bool {
        let ready_ok = self.ready.is_some_and(|(epoch, seq)| {
            epoch == self.epoch && self.last_seq.is_some_and(|l| l >= seq)
        });
        if !self.shown && ready_ok && self.pending.is_empty() {
            self.shown = true;
            return true;
        }
        false
    }

    /// The page sent measure `seq` of page load `epoch`.
    pub fn on_measure(&mut self, epoch: u64, seq: u64, m: Measured) -> Plan {
        let mut plan = Plan::default();
        if epoch < self.epoch || (epoch == self.epoch && self.last_seq.is_some_and(|l| seq <= l)) {
            plan.stale = true;
            return plan;
        }
        if epoch > self.epoch {
            self.epoch = epoch;
            // A ready of an older page load says nothing of this one; one that came first for this epoch stands.
            self.ready = self.ready.filter(|(e, _)| *e == epoch);
        }
        self.last_seq = Some(seq);
        self.last = Some(m);
        self.refit(&mut plan);
        plan.show = self.try_show();
        plan
    }

    /// The page says measure `seq` of `epoch` is the one to show at.
    pub fn on_ready(&mut self, epoch: u64, seq: u64) -> Plan {
        self.ready = Some((epoch, seq));
        Plan {
            show: self.try_show(),
            ..Plan::default()
        }
    }

    /// The OS reports the inner size `width` by `height` (physical px).
    pub fn on_resized(&mut self, width: u32, height: u32) -> Plan {
        let mut plan = Plan::default();
        let matched = self.pending.iter().rposition(|r| r.matches(width, height));
        match matched {
            Some(i) => {
                self.pending.drain(..=i);
            }
            None if self.current.matches(width, height) => {}
            None => {
                plan.unexplained = true;
                if self.pending.is_empty()
                    && self.infer_user_resize
                    && self.mode != SizeMode::Manual
                {
                    self.mode = SizeMode::Manual;
                    plan.mode_change = Some(SizeMode::Manual);
                }
            }
        }
        self.current = Request { width, height };
        plan.show = self.try_show();
        plan
    }

    /// The OS says the user is resizing (`WM_SIZING`, `SC_SIZE`, a live resize): the size is theirs for this opening.
    pub fn on_user_resize_started(&mut self) -> Plan {
        let mut plan = Plan::default();
        if self.mode != SizeMode::Manual {
            self.mode = SizeMode::Manual;
            plan.mode_change = Some(SizeMode::Manual);
        }
        plan
    }

    /// The window moved, perhaps to another monitor (`env` is the new one): the clamp is made again; the mode never
    /// changes.
    pub fn on_moved(&mut self, env: Env) -> Plan {
        let mut plan = Plan::default();
        if env != self.env {
            self.env = env;
            self.refit(&mut plan);
        }
        plan
    }

    /// The window's scale changed (`env` is the new one). The refit is posted, so it runs after the OS's own resize, and
    /// the page measures again.
    pub fn on_scale_changed(&mut self, env: Env) -> Plan {
        let old = self.env.scale;
        let mut plan = Plan::default();
        // What was asked is still the same logical size; only its physical size follows the new scale.
        let keep = self.effective();
        self.env = env;
        let _ = old;
        self.pending.clear();
        self.current = Request {
            width: physical(keep.width, env.scale),
            height: physical(keep.height, env.scale),
        };
        self.refit(&mut plan);
        plan.refit_posted = true;
        plan.remeasure = true;
        plan
    }

    /// The show deadline passed: the window is shown at whatever size it has, and what was missing is said.
    pub fn on_show_deadline(&mut self) -> (Plan, Missing) {
        if self.shown {
            return (Plan::default(), Missing::Nothing);
        }
        let missing = match (self.ready, self.last_seq) {
            (None, _) | (_, None) => Missing::Measure,
            (Some((e, s)), Some(l)) if e != self.epoch || l < s => Missing::NotApplied,
            _ => Missing::NoResized,
        };
        self.shown = true;
        (
            Plan {
                show: true,
                ..Plan::default()
            },
            missing,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Kind;
    use proptest::prelude::*;

    fn size(width: f64, height: f64) -> Size {
        Size { width, height }
    }

    fn env(scale: f64) -> Env {
        Env {
            scale,
            work: size(1920.0, 1080.0),
            frame: size(16.0, 39.0),
        }
    }

    fn state(kind: Kind, scale: f64, infer: bool) -> SizeState {
        let p = kind.policy();
        let floor = size(320.0, 200.0);
        let e = env(scale);
        SizeState::new(
            p.size_mode,
            floor,
            p.default_height.unwrap_or(0.0),
            infer,
            e,
            Request {
                width: physical(400.0, scale),
                height: physical(520.0, scale),
            },
        )
    }

    fn measured(content_h: f64) -> Measured {
        Measured {
            content: size(400.0, content_h),
            min_content: size(320.0, 120.0),
            overflow: false,
        }
    }

    fn apply(s: &mut SizeState, plan: &Plan) -> Plan {
        match plan.request {
            Some(r) => s.on_resized(r.width, r.height),
            None => Plan::default(),
        }
    }

    #[test]
    fn a_form_follows_its_content_up_and_down_and_is_shown_after_its_resize() {
        let mut s = state(Kind::Form, 1.5, false);
        let plan = s.on_measure(1, 1, measured(300.0));
        assert_eq!(plan.inner.unwrap().height, 300.0);
        assert_eq!(plan.request.unwrap().height, 450);
        assert!(!plan.show);
        assert!(!s.on_ready(1, 1).show, "held until the Resized arrives");
        let shown = apply(&mut s, &plan);
        assert!(shown.show && s.is_shown());
        let bigger = s.on_measure(1, 2, measured(700.0));
        assert_eq!(bigger.inner.unwrap().height, 700.0);
        apply(&mut s, &bigger);
        let smaller = s.on_measure(1, 3, measured(250.0));
        assert_eq!(smaller.inner.unwrap().height, 250.0);
    }

    #[test]
    fn a_viewer_opens_at_its_floor_and_only_grows() {
        let mut s = state(Kind::Viewer, 1.0, false);
        let a = s.on_measure(1, 1, measured(100.0));
        assert_eq!(a.inner.unwrap().height, 560.0, "the preferred floor");
        apply(&mut s, &a);
        let b = s.on_measure(1, 2, measured(700.0));
        assert_eq!(b.inner.unwrap().height, 700.0);
        apply(&mut s, &b);
        let c = s.on_measure(1, 3, measured(300.0));
        assert!(c.request.is_none(), "never shorter: {c:?}");
    }

    #[test]
    fn the_cap_limits_the_preferred_height_but_never_the_minimum() {
        let mut s = state(Kind::Form, 1.0, false);
        let cap = 1080.0 * 0.75 - 39.0;
        let plan = s.on_measure(1, 1, measured(5000.0));
        assert_eq!(plan.inner.unwrap().height, cap);
        let mut s = state(Kind::Form, 1.0, false);
        let m = Measured {
            min_content: size(320.0, 900.0),
            ..measured(5000.0)
        };
        assert_eq!(s.on_measure(1, 1, m).inner.unwrap().height, 900.0);
        assert_eq!(s.min().height, 900.0);
    }

    #[test]
    fn a_tool_keeps_its_size_and_only_the_minimum_raises_it() {
        let mut s = state(Kind::Tool, 1.0, false);
        let a = s.on_measure(1, 1, measured(100.0));
        assert!(a.request.is_none(), "{a:?}");
        assert_eq!(s.min().height, 200.0, "the floor holds");
        let m = Measured {
            min_content: size(320.0, 800.0),
            ..measured(100.0)
        };
        let b = s.on_measure(1, 2, m);
        assert_eq!(b.inner.unwrap().height, 800.0);
    }

    #[test]
    fn a_user_resize_makes_a_height_window_manual_and_later_content_leaves_it() {
        let mut s = state(Kind::Form, 1.0, false);
        let p = s.on_measure(1, 1, measured(300.0));
        apply(&mut s, &p);
        let plan = s.on_user_resize_started();
        assert_eq!(plan.mode_change, Some(SizeMode::Manual));
        assert_eq!(s.on_user_resize_started().mode_change, None);
        s.on_resized(500, 650);
        let later = s.on_measure(1, 2, measured(900.0));
        assert!(later.request.is_none(), "{later:?}");
    }

    #[test]
    fn a_move_never_changes_the_mode_and_a_matching_resize_is_not_the_user() {
        let mut s = state(Kind::Form, 1.0, true);
        let p = s.on_measure(1, 1, measured(300.0));
        let after = apply(&mut s, &p);
        assert!(!after.unexplained && after.mode_change.is_none());
        let moved = s.on_moved(Env {
            work: size(1280.0, 800.0),
            ..env(1.0)
        });
        assert!(moved.mode_change.is_none());
        assert_eq!(s.mode(), Kind::Form.policy().size_mode);
    }

    #[test]
    fn without_an_os_signal_an_unexplained_resize_with_nothing_pending_is_the_user() {
        let mut s = state(Kind::Form, 1.0, true);
        let p = s.on_measure(1, 1, measured(300.0));
        apply(&mut s, &p);
        let user = s.on_resized(640, 480);
        assert!(user.unexplained);
        assert_eq!(user.mode_change, Some(SizeMode::Manual));
        // With a request still queued it is only logged.
        let mut s = state(Kind::Form, 1.0, true);
        s.on_measure(1, 1, measured(300.0));
        let early = s.on_resized(640, 480);
        assert!(early.unexplained && early.mode_change.is_none());
        // Where the OS has a signal the comparison is not used.
        let mut s = state(Kind::Form, 1.0, false);
        let plain = s.on_resized(640, 480);
        assert!(plain.unexplained && plain.mode_change.is_none());
    }

    #[test]
    fn a_resized_equal_to_the_size_the_window_already_has_is_neither_unexplained_nor_the_user() {
        // A hidden GTK window gets no configure for a size asked of it, so the glue reads the size back and reports it
        // as the echo; the configure that comes with the map then repeats that size.
        let mut s = state(Kind::Form, 1.0, true);
        let p = s.on_measure(1, 1, measured(300.0));
        let r = p.request.unwrap();
        s.on_resized(r.width, r.height);
        let again = s.on_resized(r.width, r.height);
        assert!(!again.unexplained && again.mode_change.is_none());
        assert_eq!(s.mode(), Kind::Form.policy().size_mode);
    }

    #[test]
    fn an_older_seq_changes_nothing_and_a_new_epoch_is_accepted() {
        let mut s = state(Kind::Form, 1.0, false);
        let p = s.on_measure(1, 5, measured(300.0));
        apply(&mut s, &p);
        let before = s.clone();
        for (e, q) in [(1, 5), (1, 4), (0, 9)] {
            assert!(s.on_measure(e, q, measured(900.0)).stale);
            assert_eq!(s, before);
        }
        let reload = s.on_measure(2, 1, measured(500.0));
        assert!(!reload.stale);
        assert_eq!(reload.inner.unwrap().height, 500.0);
        // A ready of the old epoch does not show a window of the new one.
        let mut s = state(Kind::Form, 1.0, false);
        s.on_ready(1, 1);
        let p = s.on_measure(2, 1, measured(300.0));
        apply(&mut s, &p);
        assert!(!s.is_shown());
    }

    #[test]
    fn the_show_deadline_says_what_was_missing() {
        let mut s = state(Kind::Form, 1.0, false);
        assert_eq!(s.on_show_deadline().1, Missing::Measure);
        assert!(s.is_shown());
        assert_eq!(s.on_show_deadline().1, Missing::Nothing);
        let mut s = state(Kind::Form, 1.0, false);
        s.on_ready(1, 3);
        s.on_measure(1, 1, measured(300.0));
        assert_eq!(s.on_show_deadline().1, Missing::NotApplied);
        let mut s = state(Kind::Form, 1.0, false);
        s.on_measure(1, 1, measured(300.0));
        s.on_ready(1, 1);
        assert_eq!(s.on_show_deadline().1, Missing::NoResized);
    }

    #[test]
    fn a_scale_change_posts_one_refit_and_one_remeasure_at_the_new_scale() {
        let mut s = state(Kind::Form, 1.0, false);
        let p = s.on_measure(1, 1, measured(300.0));
        apply(&mut s, &p);
        let plan = s.on_scale_changed(env(1.5));
        assert!(plan.refit_posted && plan.remeasure);
        assert!(plan.request.is_none(), "the same logical size: {plan:?}");
        assert_eq!(s.effective().height, 300.0);
        // The page's next measure re-wraps taller at the new DPI.
        let next = s.on_measure(1, 2, measured(340.0));
        assert_eq!(next.request.unwrap().height, 510);
    }

    #[test]
    fn a_monitor_with_a_smaller_work_area_clamps_the_window() {
        let mut s = state(Kind::Form, 1.0, false);
        let p = s.on_measure(1, 1, measured(700.0));
        apply(&mut s, &p);
        let plan = s.on_moved(Env {
            work: size(1024.0, 500.0),
            ..env(1.0)
        });
        assert_eq!(plan.inner.unwrap().height, 500.0 * 0.75 - 39.0);
    }

    #[derive(Debug, Clone)]
    enum Op {
        Measure {
            seq_step: i8,
            content: f64,
            min_h: f64,
        },
        Ready,
        OsResize,
        Foreign(u32, u32),
        UserStart,
        Moved(f64, f64),
        Scale(f64),
        Deadline,
        Reload,
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (-2i8..4, 50.0..4000.0f64, 50.0..900.0f64).prop_map(|(seq_step, content, min_h)| {
                Op::Measure {
                    seq_step,
                    content,
                    min_h,
                }
            }),
            Just(Op::Ready),
            Just(Op::OsResize),
            (200u32..3000, 200u32..3000).prop_map(|(w, h)| Op::Foreign(w, h)),
            Just(Op::UserStart),
            (800.0..7680.0f64, 600.0..4320.0f64).prop_map(|(w, h)| Op::Moved(w, h)),
            prop_oneof![
                Just(1.0),
                Just(1.25),
                Just(1.5),
                Just(1.75),
                Just(2.0),
                Just(3.0)
            ]
            .prop_map(Op::Scale),
            Just(Op::Deadline),
            Just(Op::Reload),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(400))]

        /// ADR 0043 check 2: whatever order events come in, the plans keep the window inside its limits, only a user
        /// resize makes it manual, and the window is shown only after its ready.
        #[test]
        fn any_interleaving_keeps_the_size_rules(
            kind in prop_oneof![Just(Kind::Decision), Just(Kind::Form), Just(Kind::Viewer), Just(Kind::Tool)],
            scale in prop_oneof![Just(1.0), Just(1.25), Just(1.5), Just(1.75), Just(2.0), Just(3.0)],
            work in (800.0..7680.0f64, 600.0..4320.0f64),
            frame in (0.0..60.0f64, 0.0..60.0f64),
            infer in any::<bool>(),
            ops in proptest::collection::vec(op(), 1..40),
        ) {
            let e = Env { scale, work: size(work.0, work.1), frame: size(frame.0, frame.1) };
            let p = kind.policy();
            let mut s = SizeState::new(p.size_mode, size(320.0, 200.0), p.default_height.unwrap_or(0.0), infer, e,
                Request { width: physical(400.0, scale), height: physical(520.0, scale) });
            let (mut epoch, mut seq) = (1u64, 0u64);
            let mut env_now = e;
            let mut os: VecDeque<Request> = VecDeque::new();
            let mut ready_sent: Option<(u64, u64)> = None;
            let mut manual_expected = p.size_mode == SizeMode::Manual;
            for o in ops {
                let before_mode = s.mode();
                let eff_before = s.effective();
                let mut user_signal = false;
                let measured_now = if let Op::Measure { content, .. } = &o { Some(*content) } else { None };
                let plan = match o {
                    Op::Measure { seq_step, content, min_h } => {
                        let next = (seq as i64 + i64::from(seq_step)).max(0) as u64;
                        let stale_expected = next <= seq && seq > 0;
                        let m = Measured { content: size(400.0, content), min_content: size(320.0, min_h), overflow: false };
                        let snapshot = s.clone();
                        let plan = s.on_measure(epoch, next, m);
                        if stale_expected {
                            prop_assert!(plan.stale);
                            prop_assert_eq!(&s, &snapshot);
                        } else {
                            seq = next;
                        }
                        plan
                    }
                    Op::Ready => { ready_sent = Some((epoch, seq)); s.on_ready(epoch, seq) }
                    Op::OsResize => match os.pop_front() {
                        Some(r) => s.on_resized(r.width, r.height),
                        None => Plan::default(),
                    },
                    Op::Foreign(w, h) => {
                        // The OS echoes older requests first; a foreign size only counts when nothing is queued.
                        user_signal = infer && s.pending() == 0 && !os.iter().any(|r| r.matches(w, h));
                        s.on_resized(w, h)
                    }
                    Op::UserStart => { user_signal = true; s.on_user_resize_started() }
                    Op::Moved(w, h) => {
                        env_now = Env { work: size(w, h), ..env_now };
                        s.on_moved(env_now)
                    }
                    Op::Scale(sc) => {
                        env_now = Env { scale: sc, ..env_now };
                        os.clear();
                        let plan = s.on_scale_changed(env_now);
                        prop_assert!(plan.refit_posted && plan.remeasure);
                        plan
                    }
                    Op::Deadline => { let (plan, _) = s.on_show_deadline(); plan }
                    Op::Reload => { epoch += 1; seq = 0; ready_sent = None; Plan::default() }
                };
                if let Some(r) = plan.request {
                    os.push_back(r);
                    let inner = plan.inner.unwrap();
                    let avail = env_now.avail();
                    let min = s.min();
                    prop_assert!(inner.width >= min.width - 1e-6 && inner.height >= min.height - 1e-6, "below min: {inner:?} {min:?}");
                    prop_assert!(inner.width <= avail.width + 1e-6 && inner.height <= avail.height + 1e-6, "above avail: {inner:?} {avail:?}");
                    let back = f64::from(r.height) / env_now.scale;
                    prop_assert!(back >= inner.height - 1e-6 && back < inner.height + 1.0 / env_now.scale + 1e-6);
                }
                if let (Some(content), false) = (measured_now, plan.stale) {
                    let min = s.min();
                    let avail = env_now.avail();
                    let eff = s.effective();
                    if before_mode == SizeMode::Manual {
                        // A manual window is only ever raised to a new minimum (or lowered onto a smaller work area).
                        prop_assert!(eff.height <= eff_before.height.max(min.height) + 1.0 / env_now.scale + 1e-6, "manual grew: {eff:?} {eff_before:?}");
                    } else if s.mode() == (SizeMode::Height { shrink: Shrink::Always }) {
                        let cap = env_now.work.height * CAP_FRACTION - env_now.frame.height;
                        let want = content.ceil();
                        if want >= min.height && want <= cap && want <= avail.height {
                            prop_assert!((eff.height - want).abs() <= 1.0 / env_now.scale + 1e-6, "not the content: {eff:?} want {want}");
                        }
                    }
                }
                if before_mode != s.mode() {
                    prop_assert!(user_signal, "mode changed without a user resize: {:?}", plan);
                    manual_expected = true;
                }
                if manual_expected {
                    prop_assert_eq!(s.mode(), SizeMode::Manual);
                }
                if plan.show {
                    prop_assert!(s.is_shown());
                }
            }
            let _ = ready_sent;
        }
    }
}
