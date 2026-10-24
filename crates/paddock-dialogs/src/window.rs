//! The pure rules of a dialog window: what the UI asks for and gets back (the wire types), request validation, the
//! window geometry by kind, the builder configuration (title-bar buttons disabled, owned or independent per OS), the
//! fit-to-content rules, and the shell's event names.
//!
//! Ported from Debrief's `dialog_windows.rs` (Debrief ADRs 0050 and 0054) without Debrief's labels, events or error type.

use crate::contract::{
    Contract, DIALOG_MIN, FALLBACK_SIZE, Kind, MAX_SIDE, Modality, valid_dialog_id,
};
use crate::placement::{self, Monitor, Rect, Remembered, ScreenPosition, TITLE_BAR};
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

/// What the UI asks for when opening a dialog. The kind, modality, required choice and floor are not asked for: they
/// come from the dialog's declared contract, which Rust owns.
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
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub width: Option<f64>,
    /// None: the window is fitted to its content (a fitted form or decision).
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub height: Option<f64>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub min_width: Option<f64>,
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub min_height: Option<f64>,
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

/// A size within what a window can sensibly be; the default for a value that is not a number.
fn clamp_size(v: Option<f64>, min: f64, default: f64) -> f64 {
    match v {
        Some(v) if v.is_finite() => v.clamp(min, MAX_SIDE),
        _ => default.clamp(min, MAX_SIDE),
    }
}

/// The validated window geometry of a request, logical px: (width, height, min width, min height). The request's
/// sizes, else the kind's default, else [`FALLBACK_SIZE`]; never below the dialog's floor ([`Contract::floor`]: a viewer
/// is never smaller than 680x560) nor above [`MAX_SIDE`].
pub fn geometry(r: &DialogRequest, contract: &Contract) -> (f64, f64, f64, f64) {
    let floor = contract.floor();
    let default = contract.kind.policy().default_size.unwrap_or(FALLBACK_SIZE);
    let min_w = clamp_size(r.min_width, DIALOG_MIN.width, DIALOG_MIN.width).max(floor.width);
    let min_h = clamp_size(r.min_height, DIALOG_MIN.height, DIALOG_MIN.height).max(floor.height);
    (
        clamp_size(r.width, min_w, default.width),
        clamp_size(r.height, min_h, default.height),
        min_w,
        min_h,
    )
}

/// Whether the window of a request is fitted to its content: the kind fits content and the request names no height.
pub fn is_fitted(r: &DialogRequest, kind: Kind) -> bool {
    kind.policy().fits_content && r.height.is_none()
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
        position: at.logical,
        min_inner: (min.0.min(w), min.1.min(h)),
        resizable: true,
        owned: Parenting::of(spec.modality, spec.os) == Parenting::Owned,
    }
}

/// The window height (logical px) that fits `content` px of dialog content: at least [`DIALOG_MIN`], at most `max`
/// (the work area's height less the title bar); `None` for a value that is not a number.
pub fn fitted_height(content: f64, max: f64) -> Option<f64> {
    if !content.is_finite() || !max.is_finite() {
        return None;
    }
    Some(
        content
            .ceil()
            .clamp(DIALOG_MIN.height, max.clamp(DIALOG_MIN.height, MAX_SIDE)),
    )
}

/// The height a dialog window is set to when its content measures `content` px: fitted, but never below the dialog's
/// floor (a viewer opens at 560 and is not fitted down to its first short state) and, for a kind that only grows, never
/// below the height it has (a result being read does not shrink under the reader, even when the user made it taller).
pub fn fit_target(kind: Kind, content: f64, max: f64, floor: f64, current: f64) -> Option<f64> {
    let fitted = fitted_height(content, max)?.max(floor);
    Some(if kind.policy().grows_only {
        fitted.max(current)
    } else {
        fitted
    })
}

/// What a re-fit does to a dialog window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// The new inner height, logical px.
    pub height: f64,
    /// Where the window moves so that it stays on its monitor, if it must.
    pub move_to: Option<ScreenPosition>,
}

/// The re-fit of a dialog of `kind` (floor height `floor`) whose content measures `content` px, now `current` px tall
/// inside the outer rectangle `outer` (physical px) on `monitor`: `Ok(None)` when it already has that height (within
/// half a pixel), an error for a content height that is not a number. The height is bounded by the work area of the
/// dialog's own monitor, and a window that would hang off its bottom moves up on the same monitor.
pub fn plan_fit(
    kind: Kind,
    content: f64,
    floor: f64,
    current: f64,
    outer: Option<Rect>,
    monitor: Option<Monitor>,
    os: &str,
) -> Result<Option<Fit>, DialogError> {
    let monitor = monitor.filter(|m| m.work.is_usable());
    let max = monitor.map_or(MAX_SIDE, |m| m.work.height / m.scale() - TITLE_BAR);
    let height = fit_target(kind, content, max, floor, current).ok_or_else(|| {
        DialogError::new(
            DialogErrorCode::Invalid,
            "the content height is not a number",
        )
    })?;
    if (height - current).abs() < 0.5 {
        return Ok(None);
    }
    let move_to = outer
        .and_then(|o| placement::keep_on_monitor(o, height, monitor))
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
    Ok(Some(Fit { height, move_to }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{Contracts, VIEWER_MIN};
    use serde_json::json;

    fn contracts() -> Contracts {
        Contracts::parse(
            r#"{ "version": 1, "dialogs": {
                "a": { "kind": "form", "modality": "modeless" },
                "about": { "kind": "viewer", "modality": "modeless", "minSize": { "width": 680, "height": 560 } },
                "settings": { "kind": "tool", "modality": "modeless" },
                "ask": { "kind": "decision", "modality": "modal" } } }"#,
        )
        .unwrap()
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
            width: Some(760.0),
            height: None,
            min_width: None,
            min_height: None,
            remembered: None,
        }
    }

    #[test]
    fn a_dialog_window_is_fitted_to_its_content_within_bounds() {
        assert_eq!(
            fitted_height(180.2, 900.0),
            Some(DIALOG_MIN.height.max(181.0))
        );
        assert_eq!(fitted_height(250.2, 900.0), Some(251.0));
        assert_eq!(fitted_height(50.0, 900.0), Some(DIALOG_MIN.height));
        assert_eq!(fitted_height(5000.0, 700.0), Some(700.0));
        assert_eq!(fitted_height(5000.0, 1e9), Some(MAX_SIDE));
        assert_eq!(fitted_height(f64::NAN, 700.0), None);
        assert_eq!(fitted_height(300.0, f64::INFINITY), None);
    }

    #[test]
    fn a_viewer_is_never_fitted_below_its_floor_or_shrunk_and_a_decision_fits_exactly() {
        use Kind::{Decision, Form, Viewer};
        // A viewer opened at 560 with a short first state stays 560; it grows with the content; it never shrinks.
        assert_eq!(fit_target(Viewer, 300.0, 900.0, 560.0, 560.0), Some(560.0));
        assert_eq!(fit_target(Viewer, 700.0, 900.0, 560.0, 560.0), Some(700.0));
        assert_eq!(
            fit_target(Viewer, 600.0, 900.0, 560.0, 800.0),
            Some(800.0),
            "the user made it taller"
        );
        assert_eq!(
            fit_target(Viewer, 5000.0, 700.0, 560.0, 560.0),
            Some(700.0),
            "up to the screen"
        );
        // A decision is exactly its content; so is a fitted form, which may shrink after a step.
        assert_eq!(
            fit_target(Decision, 251.0, 900.0, 200.0, 520.0),
            Some(251.0)
        );
        assert_eq!(fit_target(Form, 300.0, 900.0, 200.0, 520.0), Some(300.0));
        assert_eq!(fit_target(Decision, f64::NAN, 900.0, 200.0, 520.0), None);
    }

    #[test]
    fn a_refit_is_bounded_by_the_dialogs_own_monitor_and_moves_up_on_it() {
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
        let fit = plan_fit(
            Kind::Form,
            400.0,
            200.0,
            220.0,
            Some(outer),
            Some(m),
            "macos",
        )
        .unwrap()
        .unwrap();
        assert_eq!(fit.height, 400.0);
        let to = fit.move_to.unwrap();
        // 400 + 32 logical = 864 physical; it ends at the work area's bottom, in points on macOS.
        assert_eq!(to.logical, (-1000.0, (1964.0 - 864.0) / 2.0));
        assert_eq!(to.physical, None);
        let fit = plan_fit(
            Kind::Form,
            400.0,
            200.0,
            220.0,
            Some(outer),
            Some(m),
            "windows",
        )
        .unwrap()
        .unwrap();
        assert_eq!(fit.move_to.unwrap().physical, Some((-2000, 1100)));
        // Content taller than the display: bounded by its work area (in its own logical px).
        let tall = plan_fit(Kind::Form, 5000.0, 200.0, 220.0, None, Some(m), "linux")
            .unwrap()
            .unwrap();
        assert_eq!(
            (tall.height, tall.move_to),
            (1964.0 / 2.0 - TITLE_BAR, None)
        );
        // Already that tall: nothing to do; a value that is not a number is an error.
        assert_eq!(
            plan_fit(Kind::Form, 220.2, 200.0, 221.0, None, None, "linux"),
            Ok(None)
        );
        let e = plan_fit(Kind::Form, f64::NAN, 200.0, 221.0, None, None, "linux").unwrap_err();
        assert_eq!(e.code, DialogErrorCode::Invalid);
        // No monitor known: bounded by the largest side, never moved.
        let free = plan_fit(
            Kind::Viewer,
            900.0,
            560.0,
            560.0,
            Some(outer),
            None,
            "linux",
        )
        .unwrap()
        .unwrap();
        assert_eq!((free.height, free.move_to), (900.0, None));
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

        let mut r = request("a");
        r.width = Some(f64::NAN);
        r.height = Some(1e9);
        r.min_width = Some(10.0);
        assert_eq!(
            geometry(&r, &contract("a")),
            (
                FALLBACK_SIZE.width,
                MAX_SIDE,
                DIALOG_MIN.width,
                DIALOG_MIN.height
            )
        );
        // The kind's floor wins over a smaller request: a viewer is never below 680x560.
        let mut small = request("about");
        small.width = Some(400.0);
        small.height = Some(300.0);
        small.min_width = Some(320.0);
        small.min_height = Some(200.0);
        assert_eq!(
            geometry(&small, &contract("about")),
            (680.0, 560.0, 680.0, 560.0)
        );
        // The kind's default size when the request names none.
        let mut bare = request("settings");
        bare.width = None;
        assert_eq!(
            geometry(&bare, &contract("settings")),
            (960.0, 680.0, 320.0, 200.0)
        );
        let mut bare = request("about");
        bare.width = None;
        assert_eq!(geometry(&bare, &contract("about")).0, VIEWER_MIN.width);

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
    fn only_kinds_that_fit_content_and_requests_without_a_height_are_fitted() {
        let r = request("a");
        assert!(
            is_fitted(&r, Kind::Form)
                && is_fitted(&r, Kind::Decision)
                && is_fitted(&r, Kind::Viewer)
        );
        assert!(!is_fitted(&r, Kind::Tool));
        let mut fixed = request("a");
        fixed.height = Some(400.0);
        assert!(!is_fitted(&fixed, Kind::Form));
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
