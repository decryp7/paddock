//! The pure rules of a dialog window: what the UI asks for and gets back (the wire types), request validation, the
//! window geometry by kind, the builder configuration (title-bar buttons disabled, owned or independent per OS), the
//! fit-to-content rules, and the shell's event names.
//!
//! Ported from Debrief's `dialog_windows.rs` (Debrief ADRs 0050 and 0054) without Debrief's labels, events or error type.

use crate::contract::{
    CAP_FRACTION, Contract, Kind, MAX_SIDE, Modality, Size, UNMEASURED_HEIGHT, valid_dialog_id,
};
use crate::placement::{self, Monitor, Rect, Remembered, ScreenPosition};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// To the window that opened the dialog: it answered. Payload: [`DialogResult`].
pub const RESULT_EVENT: &str = "paddock://dialog-result";
/// To the window that opened the dialog: it is gone (with or without a result). Payload: [`DialogClosed`].
pub const CLOSED_EVENT: &str = "paddock://dialog-closed";
/// To the dialog: the user asked to close it (Cmd/Ctrl+W, a window-manager close); the dialog decides and calls
/// `dialog_close`. No payload.
pub const CLOSE_REQUESTED_EVENT: &str = "paddock://dialog-close-requested";

/// More dialog windows per host window than this are refused.
pub const MAX_OPEN_PER_HOST: usize = 8;
/// The largest input a dialog is opened with, and the largest result it answers with (serialised JSON bytes).
pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;
/// The longest instance key (letters and digits).
pub const MAX_INSTANCE_LEN: usize = 16;
/// The longest window title, in characters.
pub const MAX_TITLE: usize = 200;
/// The route of the app's UI that renders a dialog (the default of [`crate::Dialogs::new`]).
pub const DEFAULT_ROUTE: &str = "index.html#/dialog";

/// What the UI asks for when opening a dialog. The kind, modality, required choice and sizes are not asked for: they
/// come from the dialog's declared contract, which Rust owns (ADR 0026 §3).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DialogRequest {
    pub dialog_id: String,
    /// Tells apart the windows of a dialog that is open once per input (a hash of the input; letters and digits).
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub instance: Option<String>,
    pub title: String,
    /// The typed input of the dialog, handed to its webview through `dialog_take_input`.
    #[serde(default)]
    #[cfg_attr(test, ts(optional, type = "unknown"))]
    pub input: Option<Value>,
    /// What the UI remembered of this dialog: its size and its offset from the parent window (never an absolute
    /// screen position). The shell decides where it goes ([`placement::place`]).
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub remembered: Option<Remembered>,
}

/// A dialog window as the opener sees it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogInfo {
    pub label: String,
    pub dialog_id: String,
    /// The window existed already and was focused.
    pub existing: bool,
}

/// What a dialog's own webview needs to render (`dialog_take_input`).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogStart {
    pub dialog_id: String,
    pub title: String,
    pub kind: Kind,
    pub modality: Modality,
    /// No Cancel: Escape and a close request leave it open.
    pub required_choice: bool,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub input: Option<Value>,
}

/// The payload of [`RESULT_EVENT`].
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogResult {
    pub label: String,
    pub dialog_id: String,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub result: Option<Value>,
}

/// The payload of [`CLOSED_EVENT`].
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogClosed {
    pub label: String,
    pub dialog_id: String,
    /// Its size and its offset from the parent window when it closed (logical px), for remembering.
    pub remembered: Option<Remembered>,
}

/// Why a dialog command failed.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DialogErrorCode {
    /// The request is malformed (a bad id, instance key or size).
    Invalid,
    /// The app has no contract for the dialog, or it takes no instance key.
    Unknown,
    /// Too large an input or result, or too many dialogs open.
    Limit,
    /// The calling window is not a dialog (or not a window of the app's scheme).
    NotADialog,
    /// The window to open the dialog for is gone.
    Closed,
    /// The dialog layer is not set up yet (no managed `Dialogs`).
    NotReady,
    /// The window system refused (creating, sizing or closing the window).
    Failed,
}

/// A failed dialog command: what the UI receives as the command's error (`code`, `message`, `retryable`).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogError {
    pub code: DialogErrorCode,
    /// For the user, as is.
    pub message: String,
    /// Whether the same call may succeed later (the layer was not ready, the window system refused).
    pub retryable: bool,
}

impl DialogError {
    /// An error with `code` and `message`.
    pub fn new(code: DialogErrorCode, message: impl Into<String>) -> Self {
        DialogError {
            code,
            message: message.into(),
            retryable: matches!(code, DialogErrorCode::NotReady | DialogErrorCode::Failed),
        }
    }
}

impl fmt::Display for DialogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for DialogError {}

/// How a dialog window is attached to its parent natively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parenting {
    /// Owned / child / transient window: above the parent, minimised with it.
    Owned,
    /// An independent window (it can be dragged to any display).
    Independent,
}

impl Parenting {
    /// The parenting of a dialog of `modality` on the OS `os` (`std::env::consts::OS`): on macOS a child window is
    /// confined to its parent's screen and vanishes when dragged to another display, so a modeless dialog is
    /// independent there.
    pub fn of(modality: Modality, os: &str) -> Parenting {
        match (os, modality) {
            ("macos", Modality::Modeless) => Parenting::Independent,
            _ => Parenting::Owned,
        }
    }
}

/// Whether opening a modal dialog disables its parent window natively. Not on macOS, where that begins a sheet on the
/// parent and blocks the dialog's own window from becoming key (the UI's scrim blocks the parent there).
pub fn disables_parent_natively(os: &str) -> bool {
    os != "macos"
}

/// A window title without control characters, at most [`MAX_TITLE`] characters, never empty.
pub fn clean_title(title: &str) -> String {
    let t: String = title
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TITLE)
        .collect();
    match t.trim() {
        "" => "Dialog".to_string(),
        t => t.to_string(),
    }
}

/// The URL a dialog loads: the app's dialog route with the dialog id, its host and its own label.
pub fn dialog_url(route: &str, dialog_id: &str, host: &str, label: &str) -> String {
    let join = if route.contains('?') { '&' } else { '?' };
    format!("{route}{join}id={dialog_id}&host={host}&label={label}")
}

/// Validates a request: a dialog id of the scheme, an instance key of letters and digits, an input within the cap.
pub fn validate(r: &DialogRequest) -> Result<(), DialogError> {
    use DialogErrorCode::{Invalid, Limit};
    if !valid_dialog_id(&r.dialog_id) {
        let shown: String = r.dialog_id.chars().take(64).collect();
        return Err(DialogError::new(
            Invalid,
            format!("not a dialog id: {shown:?}"),
        ));
    }
    if r.instance.as_ref().is_some_and(|i| {
        i.is_empty() || i.len() > MAX_INSTANCE_LEN || !i.bytes().all(|b| b.is_ascii_alphanumeric())
    }) {
        return Err(DialogError::new(Invalid, "not an instance key"));
    }
    if r.input
        .as_ref()
        .is_some_and(|i| json_len(i) > MAX_INPUT_BYTES)
    {
        return Err(DialogError::new(
            Limit,
            format!("the dialog input is larger than {MAX_INPUT_BYTES} bytes"),
        ));
    }
    Ok(())
}

/// Counts the bytes written to it.
struct Count(usize);

impl std::io::Write for Count {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0 += b.len();
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The serialised length of `v`, without building the text.
pub fn json_len(v: &Value) -> usize {
    let mut n = Count(0);
    // Writing a `Value` into a counter cannot fail.
    let _ = serde_json::to_writer(&mut n, v);
    n.0
}

/// The window geometry of a dialog, logical px: (width, height, min width, min height). The contract's width and
/// preferred height (a measured decision or form starts at [`UNMEASURED_HEIGHT`] until its first fit), never below the
/// dialog's floor ([`Contract::floor`]: a viewer is never smaller than 680x560) nor above [`MAX_SIDE`].
pub fn geometry(contract: &Contract) -> (f64, f64, f64, f64) {
    let floor = contract.floor();
    let height = contract.height.unwrap_or(UNMEASURED_HEIGHT);
    (
        contract.width.clamp(floor.width, MAX_SIDE),
        height.clamp(floor.height, MAX_SIDE),
        floor.width,
        floor.height,
    )
}

/// Whether the window of a dialog of `kind` is fitted to its content (ADR 0026 §1: decisions, forms and viewers are;
/// a tool is user-sized).
pub fn is_fitted(kind: Kind) -> bool {
    kind.policy().fits_content
}

/// The builder configuration of a dialog window, separate from the build so a test can check it without a window.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowConfig {
    pub url: String,
    pub title: String,
    /// Logical px.
    pub inner: (f64, f64),
    /// Logical px (see [`ScreenPosition`]).
    pub position: (f64, f64),
    pub min_inner: (f64, f64),
    /// Visible when built: only for [`ShowMechanism::OffscreenThenShow`] (off-screen); otherwise created hidden and
    /// shown by the lifecycle ([`ShowMechanism::ReadyThenShow`]).
    pub visible: bool,
    /// The background before the page paints ([`window_surface`]).
    pub background: (u8, u8, u8),
    pub resizable: bool,
    /// Owned by the parent natively ([`Parenting::Owned`]).
    pub owned: bool,
    /// The native close button: none (see [`title_buttons`]).
    pub closable: bool,
    /// The native minimise button: none.
    pub minimizable: bool,
    /// The native zoom / maximise button: none.
    pub maximizable: bool,
}

/// The title-bar buttons of a dialog window: (closable, minimizable, maximizable). None, for every kind: a dialog closes
/// from its content (Close or Cancel, Escape, Cmd/Ctrl+W), and a window-manager close request reaches the dialog as its
/// own Close/Cancel. Modeless windows stay resizable and movable.
pub const fn title_buttons() -> (bool, bool, bool) {
    (false, false, false)
}

/// What a dialog window is built with: where it goes is decided by [`placement::place`] beforehand.
pub struct WindowSpec<'a> {
    pub route: &'a str,
    pub label: &'a str,
    pub host: &'a str,
    pub dialog_id: &'a str,
    pub title: &'a str,
    pub modality: Modality,
    /// The OS (`std::env::consts::OS`).
    pub os: &'a str,
    /// How the window is kept unseen until it is fitted ([`show_mechanism`]).
    pub mechanism: ShowMechanism,
    /// The parent's colour scheme, for the background ([`window_surface`]).
    pub dark: bool,
}

/// The builder configuration of a dialog at `place` with logical minimum `min`.
pub fn window_config(
    spec: &WindowSpec<'_>,
    at: &ScreenPosition,
    place: &placement::Placement,
    min: (f64, f64),
) -> WindowConfig {
    let (closable, minimizable, maximizable) = title_buttons();
    let (w, h) = place.logical_size();
    WindowConfig {
        closable,
        minimizable,
        maximizable,
        url: dialog_url(spec.route, spec.dialog_id, spec.host, spec.label),
        title: spec.title.to_string(),
        inner: (w, h),
        position: match spec.mechanism {
            ShowMechanism::ReadyThenShow => at.logical,
            ShowMechanism::OffscreenThenShow => OFFSCREEN,
        },
        visible: spec.mechanism == ShowMechanism::OffscreenThenShow,
        background: window_surface(spec.dark),
        min_inner: (min.0.min(w), min.1.min(h)),
        resizable: true,
        owned: Parenting::of(spec.modality, spec.os) == Parenting::Owned,
    }
}

/// A dialog's measure (ADR 0026 §4.2), the dialog's own inner logical size as its frame measured it, synchronously,
/// after its content and `document.fonts.ready`: `content` is what it needs at its declared width (header, body at its
/// natural height, footer), `min_content` the least it can be (the footer's buttons on one line, the body's declared
/// minimum). The window system makes `min_content` the OS minimum.
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DialogMeasureWire"))]
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Measure {
    pub content: Size,
    pub min_content: Size,
}

impl Measure {
    /// The measure if all four numbers are finite and positive and at most [`MAX_SIDE`] (a webview cannot make Rust
    /// size a window to nonsense); an `Invalid` error otherwise.
    pub fn checked(self) -> Result<Measure, DialogError> {
        let sane = |s: Size| {
            [s.width, s.height]
                .iter()
                .all(|v| v.is_finite() && *v > 0.0 && *v <= MAX_SIDE)
        };
        if sane(self.content) && sane(self.min_content) {
            Ok(self)
        } else {
            Err(DialogError::new(
                DialogErrorCode::Invalid,
                "the measure is not a size",
            ))
        }
    }
}

/// How a dialog window is kept from being seen at the wrong size (ADR 0026 §4.4). The 1.1-0 spike
/// (`docs/research/2026-10-28-dialog-hidden-windows.md`) found that nothing that waits for a frame runs in a hidden
/// window, so the UI measures synchronously and Rust decides when the window appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowMechanism {
    /// Method d: the window is created hidden and shown on `dialog_ready` (or at the show deadline).
    ReadyThenShow,
    /// Method b: the window is created visible and off-screen (unfocused), moved into place on `dialog_ready` (or at
    /// the show deadline).
    OffscreenThenShow,
}

/// The ONE place the show mechanism is chosen per OS (`std::env::consts::OS`). Measured on macOS and Linux: method d.
/// Windows is pending; if it disagrees, return [`ShowMechanism::OffscreenThenShow`] for `"windows"` here and nowhere
/// else.
pub fn show_mechanism(os: &str) -> ShowMechanism {
    let _ = os;
    ShowMechanism::ReadyThenShow
}

/// Where an off-screen window waits (logical px, far outside any display arrangement).
pub const OFFSCREEN: (f64, f64) = (-30_000.0, -30_000.0);

/// The background of a dialog window before its page paints, RGB: the dialog shell's own surface (`--pd-surface-2` of
/// `_tokens.scss`, dark `#1b1e23`, light `#f0f2f5`), so the window never flashes another colour. The scheme is the parent
/// window's theme, since the request carries none.
pub const fn window_surface(dark: bool) -> (u8, u8, u8) {
    if dark {
        (0x1b, 0x1e, 0x23)
    } else {
        (0xf0, 0xf2, 0xf5)
    }
}

/// The height a dialog window is set to when its content measures `content` px: the preferred height (the content, at
/// least `floor`, the viewer's preferred height) limited to `cap`, then never below `min` (the minimum content), and,
/// for a kind that only grows, never below the height it has (a result being read does not shrink under the reader,
/// even when the user made it taller). `None` for a value that is not a number.
pub fn fit_target(
    kind: Kind,
    content: f64,
    cap: f64,
    min: f64,
    floor: f64,
    current: f64,
) -> Option<f64> {
    if ![content, cap, min, floor, current]
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    let fitted = content.ceil().max(floor).min(cap).max(min);
    Some(if kind.policy().grows_only {
        fitted.max(current)
    } else {
        fitted
    })
}

/// What a fit does to a dialog window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// The new inner size, logical px.
    pub size: Size,
    /// The new OS minimum (the minimum content), logical px.
    pub min: Size,
    /// The size differs from the current one (by half a pixel or more).
    pub resize: bool,
    /// Where the window moves so that it stays on its monitor, if it must.
    pub move_to: Option<ScreenPosition>,
}

/// What a fit knows of the window it sizes.
#[derive(Debug, Clone, Copy)]
pub struct Around<'a> {
    /// The window's inner size now, logical px.
    pub current: Size,
    /// Its own decorations: outer minus inner, logical px.
    pub decor: Size,
    /// Its outer rectangle, physical px.
    pub outer: Option<Rect>,
    /// The monitor it is on.
    pub monitor: Option<Monitor>,
    /// The OS (`std::env::consts::OS`).
    pub os: &'a str,
}

/// The fit of a dialog of `kind` (preferred-height floor `floor`, [`Kind::policy`]'s viewer height; 0 for the others)
/// whose frame measured `measure`, now `current` (inner, logical px) with decorations `decor` (the window's own outer
/// minus inner, logical px) inside the outer rectangle `outer` (physical px) on `monitor`.
///
/// * The cap is [`CAP_FRACTION`] of the monitor's work area minus the window's own decorations; it limits the preferred
///   size only and never pushes the window below the minimum content. The minimum itself stays on the work area.
/// * A decision, form or viewer takes the content height; a decision and a form shrink and grow, a viewer only grows.
///   A tool is user-sized: it keeps its size (but never below the minimum).
/// * The width is the window's (the frame is laid out at its declared or remembered width) but never below the minimum
///   content width.
/// * A window that would hang off the bottom of its monitor moves up on it.
pub fn plan_fit(
    kind: Kind,
    measure: Measure,
    floor: f64,
    around: &Around<'_>,
) -> Result<Fit, DialogError> {
    let Around {
        current,
        decor,
        outer,
        monitor,
        os,
    } = *around;
    let measure = measure.checked()?;
    let monitor = monitor.filter(|m| m.work.is_usable());
    let work = monitor.map(|m| (m.work.width / m.scale(), m.work.height / m.scale()));
    let (avail_w, avail_h) = work.map_or((MAX_SIDE, MAX_SIDE), |(w, h)| {
        ((w - decor.width).max(1.0), (h - decor.height).max(1.0))
    });
    let min = Size {
        width: measure.min_content.width.min(avail_w),
        height: measure.min_content.height.min(avail_h),
    };
    let width = current.width.max(min.width);
    let height = if kind.policy().fits_content {
        let cap = work.map_or(MAX_SIDE, |(_, h)| h * CAP_FRACTION - decor.height);
        fit_target(
            kind,
            measure.content.height,
            cap,
            min.height,
            floor,
            current.height,
        )
        // Finite by `checked` above.
        .unwrap_or(current.height)
    } else {
        current.height.max(min.height)
    };
    let size = Size { width, height };
    let resize =
        (size.width - current.width).abs() >= 0.5 || (size.height - current.height).abs() >= 0.5;
    let move_to = outer
        .and_then(|o| placement::keep_on_monitor(o, size.height + decor.height, monitor))
        .map(|(x, y)| {
            let scale = monitor.map_or(1.0, |m| m.scale());
            let p = placement::Placement {
                x,
                y,
                width: 0.0,
                height: 0.0,
                scale,
                remembered: false,
            };
            placement::screen_position(&p, os)
        });
    Ok(Fit {
        size,
        min,
        resize,
        move_to,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{Contracts, DIALOG_MIN, VIEWER_MIN};
    use serde_json::json;

    crate::dialogs! {
        static TABLE;
        "a" => Form(Modeless),
        "about" => Viewer,
        "settings" => Tool,
        "huge" => Tool { width: Some(Px(4096)), height: Some(4096) },
        "ask" => Decision,
    }

    fn contracts() -> Contracts {
        Contracts::from_tables(&[&TABLE]).unwrap()
    }

    fn contract(id: &str) -> Contract {
        contracts().get(id).unwrap().clone()
    }

    fn request(id: &str) -> DialogRequest {
        DialogRequest {
            dialog_id: id.into(),
            instance: None,
            title: "Set up".into(),
            input: Some(json!({ "modelId": "m1" })),
            remembered: None,
        }
    }

    fn measure(cw: f64, ch: f64, mw: f64, mh: f64) -> Measure {
        Measure {
            content: Size {
                width: cw,
                height: ch,
            },
            min_content: Size {
                width: mw,
                height: mh,
            },
        }
    }

    fn size(width: f64, height: f64) -> Size {
        Size { width, height }
    }

    fn around(current: Size, decor: Size, monitor: Option<Monitor>, os: &str) -> Around<'_> {
        Around {
            current,
            decor,
            outer: None,
            monitor,
            os,
        }
    }

    #[test]
    fn a_measure_must_be_a_real_size() {
        assert!(measure(400.0, 300.0, 300.0, 150.0).checked().is_ok());
        for bad in [
            measure(f64::NAN, 300.0, 300.0, 150.0),
            measure(400.0, 0.0, 300.0, 150.0),
            measure(400.0, 300.0, -1.0, 150.0),
            measure(400.0, 300.0, 300.0, f64::INFINITY),
            measure(400.0, MAX_SIDE + 1.0, 300.0, 150.0),
        ] {
            assert_eq!(bad.checked().unwrap_err().code, DialogErrorCode::Invalid);
        }
        let m: Measure = serde_json::from_value(
            json!({ "content": { "width": 400, "height": 300 }, "minContent": { "width": 300, "height": 150 } }),
        )
        .unwrap();
        assert_eq!(m, measure(400.0, 300.0, 300.0, 150.0));
        assert!(
            serde_json::from_value::<Measure>(json!({ "content": { "width": 1, "height": 1 }, "minContent": { "width": 1, "height": 1 }, "x": 1 })).is_err()
        );
    }

    #[test]
    fn the_show_mechanism_is_chosen_per_os_in_one_place() {
        for os in ["macos", "windows", "linux"] {
            assert_eq!(show_mechanism(os), ShowMechanism::ReadyThenShow, "{os}");
        }
        assert_eq!(window_surface(true), (0x1b, 0x1e, 0x23));
        assert_eq!(window_surface(false), (0xf0, 0xf2, 0xf5));
    }

    #[test]
    fn a_viewer_is_never_fitted_below_its_floor_or_shrunk_and_a_decision_fits_exactly() {
        use Kind::{Decision, Form, Viewer};
        // fit_target(kind, content, cap, min, floor, current)
        // A viewer opened at 560 with a short first state stays 560; it grows with the content; it never shrinks.
        assert_eq!(
            fit_target(Viewer, 300.0, 900.0, 100.0, 560.0, 560.0),
            Some(560.0)
        );
        assert_eq!(
            fit_target(Viewer, 700.0, 900.0, 100.0, 560.0, 560.0),
            Some(700.0)
        );
        assert_eq!(
            fit_target(Viewer, 600.0, 900.0, 100.0, 560.0, 800.0),
            Some(800.0),
            "the user made it taller"
        );
        assert_eq!(
            fit_target(Viewer, 5000.0, 700.0, 100.0, 560.0, 560.0),
            Some(700.0),
            "up to the cap"
        );
        // A decision is exactly its content; so is a form, which may shrink after a step.
        assert_eq!(
            fit_target(Decision, 250.2, 900.0, 100.0, 0.0, 520.0),
            Some(251.0)
        );
        assert_eq!(
            fit_target(Form, 300.0, 900.0, 100.0, 0.0, 520.0),
            Some(300.0)
        );
        // The cap never pushes below the minimum content.
        assert_eq!(
            fit_target(Form, 900.0, 300.0, 400.0, 0.0, 520.0),
            Some(400.0)
        );
        assert_eq!(
            fit_target(Decision, 20.0, 900.0, 150.0, 0.0, 520.0),
            Some(150.0)
        );
        for bad in [f64::NAN, f64::INFINITY] {
            assert_eq!(fit_target(Decision, bad, 900.0, 100.0, 0.0, 520.0), None);
            assert_eq!(fit_target(Decision, 300.0, bad, 100.0, 0.0, 520.0), None);
        }
    }

    #[test]
    fn the_cap_is_three_quarters_of_the_work_area_less_the_windows_own_decorations() {
        // A 1920x1080 display at 1x: cap = 0.75 * 1080 - 28 = 782.
        let m = Monitor {
            work: Rect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            scale: 1.0,
        };
        let decor = size(0.0, 28.0);
        let cur = size(560.0, 520.0);
        let fit = plan_fit(
            Kind::Form,
            measure(560.0, 4000.0, 400.0, 150.0),
            0.0,
            &around(cur, decor, Some(m), "linux"),
        )
        .unwrap();
        assert_eq!(fit.size, size(560.0, 0.75 * 1080.0 - 28.0));
        assert!(fit.resize);
        assert_eq!(fit.min, size(400.0, 150.0));
        // Never below the minimum content, even when that is taller than the cap.
        let fit = plan_fit(
            Kind::Form,
            measure(560.0, 4000.0, 400.0, 900.0),
            0.0,
            &around(cur, decor, Some(m), "linux"),
        )
        .unwrap();
        assert_eq!(fit.size.height, 900.0);
        // The minimum itself stays on the work area less the decorations.
        let fit = plan_fit(
            Kind::Form,
            measure(560.0, 300.0, 4000.0, 3000.0),
            0.0,
            &around(cur, decor, Some(m), "linux"),
        )
        .unwrap();
        assert_eq!(
            (fit.min, fit.size.height),
            (size(1920.0, 1080.0 - 28.0), 1080.0 - 28.0)
        );
        // A decision shrinks to its content and a form follows it; a viewer only grows; a tool keeps the user's size.
        let content = measure(560.0, 250.0, 300.0, 150.0);
        let at = |kind, floor| {
            plan_fit(kind, content, floor, &around(cur, decor, Some(m), "linux")).unwrap()
        };
        assert_eq!(at(Kind::Decision, 0.0).size, size(560.0, 250.0));
        assert_eq!(at(Kind::Viewer, 560.0).size, size(560.0, 560.0));
        assert!(
            at(Kind::Viewer, 560.0).resize,
            "grown from 520 to its preferred 560"
        );
        let tool = at(Kind::Tool, 0.0);
        assert_eq!((tool.size, tool.resize), (cur, false));
        // The width is the window's, but never below the minimum content width.
        let wide_min = measure(560.0, 250.0, 700.0, 150.0);
        assert_eq!(
            plan_fit(
                Kind::Decision,
                wide_min,
                0.0,
                &around(size(400.0, 300.0), decor, Some(m), "linux")
            )
            .unwrap()
            .size
            .width,
            700.0
        );
        // No monitor known: only the largest side bounds it.
        let free = plan_fit(
            Kind::Viewer,
            measure(720.0, 900.0, 300.0, 150.0),
            560.0,
            &around(size(720.0, 560.0), decor, None, "linux"),
        )
        .unwrap();
        assert_eq!((free.size.height, free.move_to), (900.0, None));
        let e = plan_fit(
            Kind::Form,
            measure(f64::NAN, 1.0, 1.0, 1.0),
            0.0,
            &around(cur, decor, None, "linux"),
        )
        .unwrap_err();
        assert_eq!(e.code, DialogErrorCode::Invalid);
    }

    #[test]
    fn a_refit_moves_the_window_up_on_its_own_monitor() {
        // A Retina display left of the primary (negative x), 2x.
        let m = Monitor {
            work: Rect {
                x: -3024.0,
                y: 0.0,
                width: 3024.0,
                height: 1964.0,
            },
            scale: 2.0,
        };
        let outer = Rect {
            x: -2000.0,
            y: 1500.0,
            width: 1200.0,
            height: 500.0,
        };
        let decor = size(0.0, 32.0);
        let a = |os| Around {
            current: size(600.0, 220.0),
            decor,
            outer: Some(outer),
            monitor: Some(m),
            os,
        };
        let fit = plan_fit(
            Kind::Form,
            measure(600.0, 400.0, 300.0, 150.0),
            0.0,
            &a("macos"),
        )
        .unwrap();
        assert_eq!(fit.size.height, 400.0);
        let to = fit.move_to.unwrap();
        // 400 + 32 logical = 864 physical; it ends at the work area's bottom, in points on macOS.
        assert_eq!(to.logical, (-1000.0, (1964.0 - 864.0) / 2.0));
        assert_eq!(to.physical, None);
        let fit = plan_fit(
            Kind::Form,
            measure(600.0, 400.0, 300.0, 150.0),
            0.0,
            &a("windows"),
        )
        .unwrap();
        assert_eq!(fit.move_to.unwrap().physical, Some((-2000, 1100)));
    }

    #[test]
    fn macos_modeless_dialogs_are_independent_and_macos_never_disables_the_parent_natively() {
        use Modality::*;
        assert_eq!(Parenting::of(Modeless, "macos"), Parenting::Independent);
        assert_eq!(Parenting::of(Modal, "macos"), Parenting::Owned);
        for os in ["windows", "linux"] {
            assert_eq!(Parenting::of(Modeless, os), Parenting::Owned);
            assert!(disables_parent_natively(os));
        }
        assert!(!disables_parent_natively("macos"));
    }

    #[test]
    fn requests_are_validated_and_sizes_bounded() {
        assert!(validate(&request("a")).is_ok());
        for bad in ["", "../x", "a b", "x?y", "A", &"x".repeat(65)] {
            let e = validate(&request(bad)).unwrap_err();
            assert_eq!(e.code, DialogErrorCode::Invalid, "{bad}");
        }
        let mut big = request("a");
        big.input = Some(Value::String("x".repeat(MAX_INPUT_BYTES + 1)));
        let e = validate(&big).unwrap_err();
        assert!(e.to_string().contains("larger") && e.code == DialogErrorCode::Limit);
        for bad in ["", "a b", "../x", &"x".repeat(17)] {
            let mut r = request("a");
            r.instance = Some(bad.to_string());
            assert!(validate(&r).is_err(), "{bad}");
        }
        let mut r = request("a");
        r.instance = Some("abc1".into());
        assert!(validate(&r).is_ok());

        // The contract decides the size: a measured form starts at its width and the unmeasured height, a viewer at its
        // preferred size (never below its floor), a tool at its own.
        assert_eq!(
            geometry(&contract("a")),
            (
                560.0,
                UNMEASURED_HEIGHT,
                DIALOG_MIN.width,
                DIALOG_MIN.height
            )
        );
        assert_eq!(geometry(&contract("ask")).0, 400.0);
        assert_eq!(
            geometry(&contract("about")),
            (720.0, 560.0, VIEWER_MIN.width, VIEWER_MIN.height)
        );
        assert_eq!(
            geometry(&contract("settings")),
            (960.0, 680.0, 320.0, 200.0)
        );
        assert_eq!(geometry(&contract("huge")).1, MAX_SIDE);
        // The webview cannot size the window either.
        for field in ["width", "height", "minWidth", "minHeight"] {
            let mut v = json!({ "dialogId": "a", "title": "t" });
            v[field] = json!(400);
            assert!(
                serde_json::from_value::<DialogRequest>(v).is_err(),
                "{field}"
            );
        }

        // A remembered place is an offset from the parent; absolute bounds are refused.
        assert!(
            serde_json::from_value::<DialogRequest>(json!({ "dialogId": "a", "title": "t",
                "bounds": { "x": 1, "y": 1, "width": 400, "height": 300 } }))
            .is_err()
        );
        let r: DialogRequest = serde_json::from_value(json!({ "dialogId": "a", "title": "t",
            "remembered": { "dx": 1, "dy": 2, "width": 400, "height": 300 } }))
        .unwrap();
        assert_eq!(r.remembered.map(|m| (m.dx, m.dy)), Some((1.0, 2.0)));
        // The webview cannot choose its own kind, modality or required choice: those come from the contract.
        for field in ["kind", "modality", "requiredChoice"] {
            let mut v = json!({ "dialogId": "a", "title": "t" });
            v[field] = json!("modal");
            assert!(
                serde_json::from_value::<DialogRequest>(v).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn decisions_forms_and_viewers_are_fitted_and_a_tool_is_user_sized() {
        assert!(is_fitted(Kind::Form) && is_fitted(Kind::Decision) && is_fitted(Kind::Viewer));
        assert!(!is_fitted(Kind::Tool));
    }

    #[test]
    fn no_dialog_window_has_title_bar_buttons_and_every_one_stays_resizable() {
        let place = placement::Placement {
            x: 20.0,
            y: 40.0,
            width: 1000.0,
            height: 800.0,
            scale: 2.0,
            remembered: false,
        };
        for (os, modality, owned) in [
            ("macos", Modality::Modal, true),
            ("macos", Modality::Modeless, false),
            ("windows", Modality::Modeless, true),
            ("linux", Modality::Modal, true),
        ] {
            let spec = WindowSpec {
                route: DEFAULT_ROUTE,
                label: "d-main-1",
                host: "main",
                dialog_id: "ai-setup",
                title: "Set up",
                modality,
                os,
                mechanism: ShowMechanism::ReadyThenShow,
                dark: true,
            };
            let at = placement::screen_position(&place, os);
            let c = window_config(&spec, &at, &place, (680.0, 300.0));
            assert_eq!(
                (c.closable, c.minimizable, c.maximizable),
                (false, false, false)
            );
            assert!(c.resizable);
            assert_eq!(c.owned, owned, "{os} {modality:?}");
            assert_eq!(
                c.url,
                "index.html#/dialog?id=ai-setup&host=main&label=d-main-1"
            );
            assert_eq!((c.inner, c.position), ((500.0, 400.0), (10.0, 20.0)));
            // The minimum never exceeds the window (a dialog shrunk to a small work area).
            assert_eq!(c.min_inner, (500.0, 300.0));
            assert_eq!(c.title, "Set up");
            // Created unseen, on the dialog shell's own surface.
            assert!(!c.visible);
            assert_eq!(c.background, window_surface(true));
            // The off-screen mechanism waits far from every display and is visible.
            let off = window_config(
                &WindowSpec {
                    mechanism: ShowMechanism::OffscreenThenShow,
                    dark: false,
                    ..spec
                },
                &at,
                &place,
                (680.0, 300.0),
            );
            assert!(off.visible && off.position == OFFSCREEN);
            assert_eq!(off.background, window_surface(false));
        }
    }

    #[test]
    fn urls_and_titles_are_clean() {
        assert_eq!(
            dialog_url("app.html?x=1", "a", "w-2", "d-w-2-3"),
            "app.html?x=1&id=a&host=w-2&label=d-w-2-3"
        );
        assert_eq!(clean_title("  Export\u{7}  "), "Export");
        assert_eq!(clean_title("\n\t "), "Dialog");
        assert_eq!(clean_title(&"x".repeat(500)).chars().count(), MAX_TITLE);
    }

    #[test]
    fn errors_say_whether_a_retry_can_help_and_serialise_as_the_command_error() {
        let e = DialogError::new(DialogErrorCode::NotReady, "not ready");
        assert!(e.retryable);
        assert!(!DialogError::new(DialogErrorCode::Unknown, "x").retryable);
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({ "code": "notReady", "message": "not ready", "retryable": true })
        );
        assert_eq!(e.to_string(), "not ready");
        let _: &dyn std::error::Error = &e;
        assert_eq!(json_len(&json!({ "a": [1, 2] })), r#"{"a":[1,2]}"#.len());
        assert!(std::io::Write::flush(&mut Count(0)).is_ok());
    }

    #[test]
    fn wire_types_are_camel_case() {
        let start = DialogStart {
            dialog_id: "a".into(),
            title: "t".into(),
            kind: Kind::Decision,
            modality: Modality::Modal,
            required_choice: true,
            input: None,
        };
        assert_eq!(
            serde_json::to_value(&start).unwrap(),
            json!({ "dialogId": "a", "title": "t", "kind": "decision", "modality": "modal", "requiredChoice": true, "input": null })
        );
        let info = DialogInfo {
            label: "d-main-1".into(),
            dialog_id: "a".into(),
            existing: false,
        };
        assert_eq!(serde_json::to_value(&info).unwrap()["dialogId"], "a");
    }
}
