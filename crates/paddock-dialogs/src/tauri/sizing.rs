//! The Tauri glue of the size state machine ([`crate::size`], ADR 0043 §3): every transition runs on the main thread, in
//! the order of the calls, takes the window's [`SizeState`] out of the registry, asks it what to do and carries out the
//! [`Plan`] on the window: the OS minimum, the physical inner size, the move that keeps the window on its monitor, the
//! mode event, the remeasure event and the show. One log line per transition says what was decided and why (ER-27).

use super::*;
use crate::contract::{DIALOG_MIN, Kind};
use crate::size::{Env, Measured, Missing, Plan, Request, SizeState};
use crate::window::{DialogRemeasureEvent, DialogSizeModeEvent, REMEASURE_EVENT, SIZE_MODE_EVENT, SizeModeName, zoom_of};

/// What a transition is called in the log.
#[derive(Debug, Clone, Copy)]
pub(super) enum Phase {
    Measure,
    Ready,
    Resized,
    UserResize,
    Moved,
    Scale,
    Deadline,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::Measure => "measure",
            Phase::Ready => "ready",
            Phase::Resized => "resized",
            Phase::UserResize => "user-resize",
            Phase::Moved => "moved",
            Phase::Scale => "scale",
            Phase::Deadline => "deadline",
        }
    }
}

/// The monitor and visible frame a plan is made for, logical px, read from the window now.
pub(super) fn env_of<R: Runtime>(window: &Window<R>, monitor: Option<Monitor>, scale: f64) -> Env {
    let work = monitor
        .filter(|m| m.work.is_usable())
        .map_or(Size { width: crate::contract::MAX_SIDE, height: crate::contract::MAX_SIDE }, |m| Size {
            width: m.work.width / m.scale(),
            height: m.work.height / m.scale(),
        });
    Env {
        scale,
        work,
        frame: decor_of(window, scale),
    }
}

/// A new state for a dialog window that has none yet: its kind's mode, floor and preferred height, created at the size the
/// window has now.
fn new_state<R: Runtime>(window: &Window<R>, kind: Kind, env: Env, os: &str) -> Result<SizeState, DialogError> {
    let p = kind.policy();
    let floor = p.min_size.unwrap_or(DIALOG_MIN);
    let floor = Size {
        width: floor.width.max(DIALOG_MIN.width),
        height: floor.height.max(DIALOG_MIN.height),
    };
    let inner = window.inner_size().map_err(failed)?;
    Ok(SizeState::new(
        p.size_mode,
        floor,
        p.min_size.map_or(0.0, |s| s.height),
        !crate::edge::has_resize_signal(os),
        env,
        Request {
            width: inner.width,
            height: inner.height,
        },
    ))
}

/// Runs one transition of the window's size state: `f` gets the state and the environment now and returns the plan,
/// which is then carried out and logged. Nothing happens for a window that is not an open dialog.
fn transition<R: Runtime>(
    app: &AppHandle<R>,
    window: &Window<R>,
    os: &str,
    phase: Phase,
    note: impl FnOnce(&SizeState) -> String,
    f: impl FnOnce(&mut SizeState, Env) -> Plan,
) -> Result<(), DialogError> {
    let dialogs = dialogs(app)?;
    let label = window.label();
    let Some(taken) = dialogs.take_size(label) else {
        return Err(not_a_dialog());
    };
    let kind = dialogs.get(label).ok_or_else(not_a_dialog)?.kind;
    let scale = window.scale_factor().map_err(failed)?;
    let monitor = monitor_of_window(window).map(|m| m.0);
    let env = env_of(window, monitor, scale);
    let mut state = match taken {
        Some(s) => s,
        None => new_state(window, kind, env, os)?,
    };
    let mode_before = state.mode();
    let plan = f(&mut state, env);
    let text = note(&state);
    let queue = state.pending();
    let shown = state.is_shown();
    let mode = state.mode();
    let min = state.min();
    dialogs.put_size(label, state);
    carry_out(app, window, os, &plan, monitor, env)?;
    let dialog = dialogs.get(label).map_or_else(String::new, |e| e.dialog_id);
    log::info!(
        target: TARGET,
        Dialog = dialog.as_str(), Phase = phase.name(), Mode = mode.as_str(), Queue = queue, Shown = shown,
        Scale = scale, Frame = env.frame.height, MinHeight = min.height,
        Requested = plan.inner.map_or(0.0, |s| s.height), ModeChange = (mode_before != mode),
        Unexplained = plan.unexplained, Stale = plan.stale;
        "Dialog window {}: {text}", phase.name()
    );
    if plan.unexplained {
        log::warn!(target: TARGET, Dialog = dialog.as_str(), Queue = queue; "Dialog window resized to a size this layer did not ask for");
    }
    Ok(())
}

/// What a plan does to the window.
fn carry_out<R: Runtime>(
    app: &AppHandle<R>,
    window: &Window<R>,
    os: &str,
    plan: &Plan,
    monitor: Option<Monitor>,
    env: Env,
) -> Result<(), DialogError> {
    if let Some(min) = plan.min {
        window
            .set_min_size(Some(PhysicalSize::new(
                placement::physical_ceil(min.width, env.scale),
                placement::physical_ceil(min.height, env.scale),
            )))
            .map_err(failed)?;
    }
    if let Some(r) = plan.request {
        window
            .set_size(PhysicalSize::new(r.width, r.height))
            .map_err(failed)?;
        if let (Some(outer), Some(inner)) = (outer_rect(window), plan.inner)
            && let Some((x, y)) =
                placement::keep_on_monitor(outer, inner.height + env.frame.height, monitor)
        {
            let place = placement::Placement {
                x,
                y,
                width: 0.0,
                height: 0.0,
                scale: monitor.map_or(1.0, |m| m.scale()),
                remembered: false,
            };
            set_position(window, &placement::screen_position(&place, os)).map_err(failed)?;
        }
    }
    if let Some(mode) = plan.mode_change {
        let name = match mode {
            crate::SizeMode::Manual => SizeModeName::Manual,
            crate::SizeMode::Height { .. } => SizeModeName::Height,
        };
        let _ = app.emit_to(window.label(), SIZE_MODE_EVENT, DialogSizeModeEvent { mode: name });
    }
    if plan.remeasure {
        let _ = app.emit_to(window.label(), REMEASURE_EVENT, DialogRemeasureEvent { scale: env.scale });
    }
    if plan.show {
        show(app, window.label(), os, false);
    }
    Ok(())
}

/// The page's measure in logical px: the webview zoom taken out, read fresh with every measure (the page measures again
/// after every scale change), and what the window's inner height has beyond the page's viewport added to both heights.
pub(super) fn measured_of<R: Runtime>(window: &Window<R>, measure: Measure, scale: f64) -> Result<Measured, DialogError> {
    let l = measure.checked()?.logical(zoom_of(measure.device_pixel_ratio, scale));
    let inner = window.inner_size().map_err(failed)?.to_logical::<f64>(scale);
    let delta = viewport_delta(inner.height, l.viewport_height);
    let with = |s: Size| Size {
        width: s.width,
        height: s.height + delta,
    };
    Ok(Measured {
        content: with(l.content),
        min_content: with(l.min_content),
        overflow: l.overflow,
    })
}

/// A measure (`dialog_measure`): validated here, so the caller hears of nonsense, then posted to the main thread.
pub(super) fn measure<R: Runtime>(
    app: &AppHandle<R>,
    window: &Window<R>,
    epoch: u64,
    seq: u64,
    measure: Measure,
    os: &str,
) -> Result<(), DialogError> {
    measure.checked()?;
    let (a, w, os) = (app.clone(), window.clone(), os.to_string());
    on_main_thread(app, move || {
        if let Err(e) = measure_now(&a, &w, epoch, seq, measure, &os) {
            log::warn!(target: TARGET, Window = w.label(); "Dialog measure failed: {e}");
        }
    });
    Ok(())
}

pub(super) fn measure_now<R: Runtime>(
    app: &AppHandle<R>,
    window: &Window<R>,
    epoch: u64,
    seq: u64,
    measure: Measure,
    os: &str,
) -> Result<(), DialogError> {
    let scale = window.scale_factor().map_err(failed)?;
    let m = measured_of(window, measure, scale)?;
    transition(
        app,
        window,
        os,
        Phase::Measure,
        |_| format!("epoch {epoch} seq {seq}, content {}x{}, min {}x{}, overflow {}", m.content.width, m.content.height, m.min_content.width, m.min_content.height, m.overflow),
        |s, _| s.on_measure(epoch, seq, m),
    )
}

/// The page says measure `seq` of `epoch` is the one to show at (`dialog_ready`).
pub(super) fn ready<R: Runtime>(app: &AppHandle<R>, window: &Window<R>, epoch: u64, seq: u64, os: &str) -> Result<(), DialogError> {
    dialogs(app)?.get(window.label()).ok_or_else(not_a_dialog)?;
    let (a, w, os) = (app.clone(), window.clone(), os.to_string());
    on_main_thread(app, move || {
        let r = transition(&a, &w, &os, Phase::Ready, |_| format!("epoch {epoch} seq {seq}"), |s, _| s.on_ready(epoch, seq));
        if let Err(e) = r {
            log::warn!(target: TARGET, Window = w.label(); "Dialog ready failed: {e}");
        }
    });
    Ok(())
}

/// The window system reported the inner size `size`: the echo of a request, or something else.
pub(super) fn resized<R: Runtime>(window: &Window<R>, size: PhysicalSize<u32>) {
    let app = window.app_handle();
    let _ = transition(app, window, std::env::consts::OS, Phase::Resized, |s| format!("{}x{} physical, pending {}", size.width, size.height, s.pending()), |s, _| s.on_resized(size.width, size.height));
}

/// The window system says the user is resizing (an edge module's signal): the size is theirs for this opening.
pub fn user_resize_started<R: Runtime>(window: &Window<R>) {
    let app = window.app_handle();
    let _ = transition(app, window, std::env::consts::OS, Phase::UserResize, |_| "the OS says the user is resizing".into(), |s, _| s.on_user_resize_started());
}

/// The window moved (perhaps to another monitor): the clamp is made again; the mode never changes.
pub(super) fn moved<R: Runtime>(window: &Window<R>) {
    let app = window.app_handle();
    let _ = transition(app, window, std::env::consts::OS, Phase::Moved, |_| String::new(), |s, env| s.on_moved(env));
}

/// The scale changed: runs on the main thread after the window system's own resize (see `scale_changed`).
pub(super) fn scale_now<R: Runtime>(window: &Window<R>) {
    let app = window.app_handle();
    let _ = transition(app, window, std::env::consts::OS, Phase::Scale, |s| format!("refit at the new scale, pending {}", s.pending()), |s, env| s.on_scale_changed(env));
}

/// The show deadline passed for `label`: says what was missing, then the window is shown.
pub(super) fn deadline<R: Runtime>(app: &AppHandle<R>, label: &str) {
    let Some(dialogs) = app.try_state::<Dialogs>() else {
        return;
    };
    let Some(Some(mut state)) = dialogs.take_size(label) else {
        return;
    };
    let (_, missing) = state.on_show_deadline();
    dialogs.put_size(label, state);
    if missing != Missing::Nothing {
        let dialog = dialogs.get(label).map_or_else(String::new, |e| e.dialog_id);
        log::warn!(target: TARGET, Dialog = dialog.as_str(), Phase = Phase::Deadline.name(); "Dialog window shown at the show deadline; missing: {missing:?}");
    }
}
