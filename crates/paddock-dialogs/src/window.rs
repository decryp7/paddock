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
/// To the window that opened the dialog: its window is on screen now (shown on ready, or at the show deadline). The
/// opener's in-app scrim over a macOS parent (which the OS does not disable) starts here, not at open. Payload:
/// [`DialogShown`].
pub const SHOWN_EVENT: &str = "paddock://dialog-shown";
/// To the dialog: the user asked to close it (Cmd/Ctrl+W, a window-manager close); the dialog decides and calls
/// `dialog_close`. No payload.
pub const CLOSE_REQUESTED_EVENT: &str = "paddock://dialog-close-requested";
/// To an open dialog window: another window opened the same dialog again with new inputs (Settings at a section), so the dialog
/// points itself there. Sent by the opener's page with `emitTo` (a native event: it reaches another webview, which a
/// `BroadcastChannel` does not reliably do between the webviews of one app). Payload: `{ dialogId, inputs }`.
pub const INPUTS_EVENT: &str = "paddock://dialog-inputs";

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
    /// The opener's resolved theme (its `data-theme`): the window's background before the page paints is that theme's
    /// `--pd-surface-2`. Without it the parent window's OS theme is used.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub theme: Option<Theme>,
}

/// A colour scheme of the app (`data-theme`).
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DialogTheme"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
}

/// A dialog window as the opener sees it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogInfo {
    pub label: String,
    pub dialog_id: String,
    /// The window existed already (and was focused, if it was on screen).
    pub existing: bool,
    /// The window is not on screen yet: it is shown when its page is ready (or at the show deadline), and
    /// `paddock://dialog-shown` follows. False for an existing window that was shown already.
    pub pending: bool,
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

/// The payload of [`SHOWN_EVENT`].
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogShown {
    pub label: String,
    pub dialog_id: String,
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
    /// Asked for inside a Tauri plugin hook, where building a webview would deadlock (a wiring bug: open dialogs from
    /// commands, events or after the hook).
    InsidePluginHook,
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
    /// Run in the page before the app ([`platform_script`]).
    pub init_script: String,
}

/// The script run in a dialog page before the app: `<html data-platform>` is the OS the window was built for (`macos`,
/// `windows` or `linux`; any other OS counts as Linux), so the shell's platform-dependent layout (the footer's button
/// order) does not depend on the user agent. Set at once, or when the document element appears.
pub fn platform_script(os: &str) -> String {
    let os = match os {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    };
    format!(
        "(function(){{var set=function(){{var d=document.documentElement;if(!d)return false;d.dataset.platform='{os}';return true;}};\
if(!set()){{var o=new MutationObserver(function(){{if(set())o.disconnect();}});o.observe(document,{{childList:true}});}}}})();"
    )
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
    /// The colour scheme, for the background ([`window_surface`]).
    pub theme: Theme,
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
        background: window_surface(spec.theme),
        init_script: platform_script(spec.os),
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
    /// The page's `window.innerHeight` (CSS px), when the page sends it. A webview can be shorter than the window's
    /// inner height (the 1.1-0 spike measured 32 px on a Mac whose outer and inner sizes were equal): the fit adds the
    /// difference ([`viewport_delta`]) to the content and minimum content heights so the footer is not cut off.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub viewport_height: Option<f64>,
    /// The page's `window.devicePixelRatio` when it measured. A webview zoomed away from the window's scale factor
    /// (browser zoom, an OS text scale) reports CSS px that are not logical px: the fit converts with [`zoom_of`].
    /// Absent: CSS px are taken as logical.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub device_pixel_ratio: Option<f64>,
}

/// The least and the most a reported `devicePixelRatio` can be (anything else is a wrong report).
pub const DEVICE_PIXEL_RATIO_RANGE: std::ops::RangeInclusive<f64> = 0.25..=8.0;

/// The webview zoom: CSS px per logical px, `devicePixelRatio / scale_factor`. 1 when the page sent no ratio or either
/// number is not a positive finite one.
pub fn zoom_of(device_pixel_ratio: Option<f64>, scale_factor: f64) -> f64 {
    match device_pixel_ratio {
        Some(r) if r.is_finite() && r > 0.0 && scale_factor.is_finite() && scale_factor > 0.0 => {
            r / scale_factor
        }
        _ => 1.0,
    }
}

/// The webview zoom a dialog window has, with the page ratio and window scale it was read at ([`zoom_after`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomState {
    /// The page's `devicePixelRatio` last seen (`None` until the page sent one).
    pub device_pixel_ratio: Option<f64>,
    /// The window's scale factor at the last fit.
    pub scale_factor: f64,
    /// CSS px per logical px ([`zoom_of`]).
    pub zoom: f64,
}

/// The zoom after a fit at window scale `scale_factor` with a page that reports `device_pixel_ratio`. The first fit
/// (`dialog_ready`) reads it as [`zoom_of`]. Later it changes only when the page's ratio changed at an unchanged scale
/// factor (the user zoomed the page); when the scale factor changed (the window moved to another monitor) the stored
/// zoom is kept, because the ratio the page sent may still be the old monitor's and `dpr_old / scale_new` would be a
/// wrong zoom. A fit without a ratio (a refit from the logical measure) keeps the zoom too.
pub fn zoom_after(
    prev: Option<ZoomState>,
    device_pixel_ratio: Option<f64>,
    scale_factor: f64,
) -> ZoomState {
    let fresh = || ZoomState {
        device_pixel_ratio,
        scale_factor,
        zoom: zoom_of(device_pixel_ratio, scale_factor),
    };
    let Some(prev) = prev else {
        return fresh();
    };
    let same_scale = prev.scale_factor == scale_factor;
    let same_ratio = device_pixel_ratio.is_none() || device_pixel_ratio == prev.device_pixel_ratio;
    if same_scale && !same_ratio {
        return fresh();
    }
    ZoomState {
        device_pixel_ratio: device_pixel_ratio.or(prev.device_pixel_ratio),
        scale_factor,
        zoom: prev.zoom,
    }
}

/// The most a viewport delta can be, logical px (a bigger gap is a wrong report, not a webview inset).
pub const MAX_VIEWPORT_DELTA: f64 = 400.0;

/// The part of a window's inner height the page's viewport does not see: `inner_height` less the page's reported
/// `viewport_height`, clamped to `0..=`[`MAX_VIEWPORT_DELTA`] (a page that is taller than the window has no delta). 0
/// when the page did not report one.
pub fn viewport_delta(inner_height: f64, viewport_height: Option<f64>) -> f64 {
    match viewport_height {
        Some(v) if v.is_finite() && inner_height.is_finite() => {
            (inner_height - v).clamp(0.0, MAX_VIEWPORT_DELTA)
        }
        _ => 0.0,
    }
}

impl Measure {
    /// The measure in logical px: content, minimum content and viewport height, which the page reports in CSS px,
    /// multiplied by `zoom` ([`zoom_of`]). The ratio is spent, so converting again does nothing.
    pub fn logical(self, zoom: f64) -> Measure {
        let by = |s: Size| Size {
            width: s.width * zoom,
            height: s.height * zoom,
        };
        Measure {
            content: by(self.content),
            min_content: by(self.min_content),
            viewport_height: self.viewport_height.map(|v| v * zoom),
            device_pixel_ratio: None,
        }
    }

    /// The measure if all four numbers are finite and positive and at most [`MAX_SIDE`] (a webview cannot make Rust
    /// size a window to nonsense); an `Invalid` error otherwise.
    pub fn checked(self) -> Result<Measure, DialogError> {
        let sane = |s: Size| {
            [s.width, s.height]
                .iter()
                .all(|v| v.is_finite() && *v > 0.0 && *v <= MAX_SIDE)
        };
        let viewport_ok = self
            .viewport_height
            .is_none_or(|v| v.is_finite() && (1.0..=MAX_SIDE).contains(&v));
        let ratio_ok = self
            .device_pixel_ratio
            .is_none_or(|r| r.is_finite() && DEVICE_PIXEL_RATIO_RANGE.contains(&r));
        if sane(self.content) && sane(self.min_content) && viewport_ok && ratio_ok {
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
}

/// The ONE place the show mechanism is chosen per OS (`std::env::consts::OS`). The 1.1-0 spike measured method d on
/// macOS, Linux and Windows, so it is the same everywhere.
pub fn show_mechanism(os: &str) -> ShowMechanism {
    let _ = os;
    ShowMechanism::ReadyThenShow
}

/// The background of a dialog window before its page paints, RGB: the dialog shell's own surface, `--pd-surface-2` of
/// `_tokens.scss` for `theme` (a test compares these values with the file), so the window never flashes another colour.
pub const fn window_surface(theme: Theme) -> (u8, u8, u8) {
    match theme {
        Theme::Dark => (0x1b, 0x1e, 0x23),
        Theme::Light => (0xf0, 0xf2, 0xf5),
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
    /// What the window's inner height has beyond the page's viewport, logical px ([`viewport_delta`]), taken once at
    /// `dialog_ready` and reused by every later fit.
    pub viewport_delta: f64,
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
        viewport_delta: delta,
    } = *around;
    let measure = measure.checked()?;
    let monitor = monitor.filter(|m| m.work.is_usable());
    let work = monitor.map(|m| (m.work.width / m.scale(), m.work.height / m.scale()));
    let (avail_w, avail_h) = work.map_or((MAX_SIDE, MAX_SIDE), |(w, h)| {
        ((w - decor.width).max(1.0), (h - decor.height).max(1.0))
    });
    let min = Size {
        width: measure.min_content.width.min(avail_w),
        height: (measure.min_content.height + delta).min(avail_h),
    };
    let width = current.width.max(min.width);
    let height = if kind.policy().fits_content {
        let cap = work.map_or(MAX_SIDE, |(_, h)| h * CAP_FRACTION - decor.height);
        fit_target(
            kind,
            measure.content.height + delta,
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
            theme: None,
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
            viewport_height: None,
            device_pixel_ratio: None,
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
            viewport_delta: 0.0,
        }
    }

    /// The `--pd-surface-2` of the `@mixin dark` or `@mixin light` block of `_tokens.scss`, as RGB.
    fn token_surface(mixin: &str) -> (u8, u8, u8) {
        let scss = include_str!("../../../ui/projects/paddock-ui/scss/_tokens.scss");
        let block = scss
            .split(&format!("@mixin {mixin} {{"))
            .nth(1)
            .expect("the mixin exists");
        let line = block
            .lines()
            .find(|l| l.trim_start().starts_with("--pd-surface-2:"))
            .expect("the block sets --pd-surface-2");
        let hex = line.split('#').nth(1).unwrap().trim_end_matches(';').trim();
        let n = u32::from_str_radix(hex, 16).unwrap();
        ((n >> 16) as u8, (n >> 8) as u8, n as u8)
    }

    #[test]
    fn the_window_surfaces_are_the_tokens_surface_2_of_each_theme() {
        assert_eq!(window_surface(Theme::Dark), token_surface("dark"));
        assert_eq!(window_surface(Theme::Light), token_surface("light"));
    }

    #[test]
    fn a_dialog_page_is_told_its_platform_before_the_app_runs() {
        for (os, platform) in [
            ("macos", "macos"),
            ("windows", "windows"),
            ("linux", "linux"),
            ("freebsd", "linux"),
        ] {
            let script = platform_script(os);
            assert!(
                script.contains(&format!("d.dataset.platform='{platform}'")),
                "{os}: {script}"
            );
        }
    }

    #[test]
    fn the_viewport_delta_is_the_inner_height_less_the_pages_and_clamped() {
        assert_eq!(viewport_delta(332.0, Some(300.0)), 32.0);
        assert_eq!(viewport_delta(300.0, Some(332.0)), 0.0);
        assert_eq!(viewport_delta(900.0, Some(100.0)), MAX_VIEWPORT_DELTA);
        assert_eq!(viewport_delta(332.0, None), 0.0);
        assert_eq!(viewport_delta(332.0, Some(f64::NAN)), 0.0);
    }

    #[test]
    fn a_viewport_short_of_the_window_adds_its_delta_to_content_and_minimum() {
        let cur = size(400.0, 332.0);
        let m = measure(400.0, 300.0, 300.0, 150.0);
        let none = plan_fit(
            Kind::Form,
            m,
            0.0,
            &around(cur, size(0.0, 0.0), None, "macos"),
        )
        .unwrap();
        let mut a = around(cur, size(0.0, 0.0), None, "macos");
        a.viewport_delta = 32.0;
        let fit = plan_fit(Kind::Form, m, 0.0, &a).unwrap();
        assert_eq!(fit.size.height, none.size.height + 32.0);
        assert_eq!(fit.min.height, none.min.height + 32.0);
        // The delta is the caller's, taken once: a measure's own viewport height does not recompute it.
        let mut resized = m;
        resized.viewport_height = Some(100.0);
        assert_eq!(plan_fit(Kind::Form, resized, 0.0, &a).unwrap(), fit);
    }

    #[test]
    fn the_zoom_follows_the_page_ratio_at_a_fixed_scale_and_survives_a_monitor_change() {
        // First fit: dpr 2 on a 2x window is zoom 1.
        let a = zoom_after(None, Some(2.0), 2.0);
        assert_eq!(
            (a.zoom, a.device_pixel_ratio, a.scale_factor),
            (1.0, Some(2.0), 2.0)
        );
        // The user zooms the page (dpr 3 at the same scale): zoom 1.5.
        let b = zoom_after(Some(a), Some(3.0), 2.0);
        assert_eq!(b.zoom, 1.5);
        // The window moves to a 1x monitor but the page still reports the old ratio: the zoom is kept, not 3 / 1.
        let c = zoom_after(Some(b), Some(3.0), 1.0);
        assert_eq!((c.zoom, c.scale_factor), (1.5, 1.0));
        // A report with the new monitor's ratio (1.5) at the new scale keeps it too (unchanged zoom, ratio recorded).
        let d = zoom_after(Some(c), Some(1.5), 1.0);
        assert_eq!(d.zoom, 1.5);
        // Both ratio and scale changed at once: kept.
        assert_eq!(zoom_after(Some(a), Some(3.0), 1.0).zoom, 1.0);
        // Later at that scale a real page zoom is read again.
        let e = zoom_after(Some(zoom_after(Some(a), Some(3.0), 1.0)), Some(2.0), 1.0);
        assert_eq!(e.zoom, 2.0);
        // A refit without a ratio keeps the zoom and the last ratio; a first fit without one is zoom 1.
        let f = zoom_after(Some(b), None, 2.0);
        assert_eq!((f.zoom, f.device_pixel_ratio), (1.5, Some(3.0)));
        assert_eq!(zoom_after(None, None, 2.0).zoom, 1.0);
        // A bad ratio on a first fit is zoom 1.
        assert_eq!(zoom_after(None, Some(f64::NAN), 2.0).zoom, 1.0);
    }

    #[test]
    fn the_zoom_is_the_ratio_over_the_scale_and_defaults_to_one() {
        assert_eq!(zoom_of(Some(3.0), 2.0), 1.5);
        assert_eq!(zoom_of(Some(2.0), 2.0), 1.0);
        for (r, s) in [
            (None, 2.0),
            (Some(2.0), 0.0),
            (Some(2.0), f64::NAN),
            (Some(0.0), 2.0),
            (Some(f64::NAN), 2.0),
        ] {
            assert_eq!(zoom_of(r, s), 1.0, "{r:?} {s}");
        }
    }

    #[test]
    fn a_zoomed_measure_converts_css_px_to_logical_px_once() {
        let mut m = measure(400.0, 300.0, 300.0, 150.0);
        m.viewport_height = Some(280.0);
        m.device_pixel_ratio = Some(3.0);
        let l = m.logical(zoom_of(m.device_pixel_ratio, 2.0));
        assert_eq!(
            (l.content, l.min_content, l.viewport_height),
            (size(600.0, 450.0), size(450.0, 225.0), Some(420.0))
        );
        assert_eq!(l.device_pixel_ratio, None, "spent");
        assert_eq!(l.logical(1.5), l.logical(1.5));
        let plain = measure(400.0, 300.0, 300.0, 150.0);
        assert_eq!(plain.logical(1.0), plain);
    }

    #[test]
    fn a_measure_must_be_a_real_size() {
        let mut v = measure(400.0, 300.0, 300.0, 150.0);
        for bad in [Some(0.0), Some(f64::NAN), Some(MAX_SIDE + 1.0)] {
            v.viewport_height = bad;
            assert_eq!(v.checked().unwrap_err().code, DialogErrorCode::Invalid);
        }
        v.viewport_height = None;
        for bad in [0.0, 0.24, 8.01, f64::NAN, f64::INFINITY] {
            v.device_pixel_ratio = Some(bad);
            assert_eq!(v.checked().unwrap_err().code, DialogErrorCode::Invalid);
        }
        for ok in [0.25, 2.0, 8.0] {
            v.device_pixel_ratio = Some(ok);
            assert!(v.checked().is_ok(), "{ok}");
        }
        v.device_pixel_ratio = None;
        v.viewport_height = Some(300.0);
        assert!(v.checked().is_ok());
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
        assert_eq!(window_surface(Theme::Dark), (0x1b, 0x1e, 0x23));
        assert_eq!(window_surface(Theme::Light), (0xf0, 0xf2, 0xf5));
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
    fn a_short_form_is_its_content_inner_height_whatever_the_windows_frame_and_scale() {
        // Windows: the frame (title bar and borders) is outside the inner size that `set_size` takes, so the fitted inner
        // height is the content, never the content less the frame, and it is a whole number of physical px.
        for (scale, frame_px) in [(1.0, 39.0), (1.25, 48.0), (1.5, 55.0)] {
            let m = Monitor {
                work: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1040.0,
                },
                scale,
            };
            let decor = size(16.0 / scale, frame_px / scale);
            let fit = plan_fit(
                Kind::Form,
                measure(560.0, 163.0, 400.0, 120.0),
                0.0,
                &around(size(560.0, 560.0), decor, Some(m), "windows"),
            )
            .unwrap();
            assert_eq!(fit.size.height, 163.0, "scale {scale}");
            assert!(fit.resize, "scale {scale}");
            let physical = placement::physical_ceil(fit.size.height, scale);
            assert!(
                f64::from(physical) >= 163.0 * scale,
                "scale {scale}: {physical}"
            );
            assert!(
                f64::from(physical) < 163.0 * scale + 1.0,
                "scale {scale}: {physical}"
            );
            // The outer window is the inner one plus the frame; it is the frame that is added, never taken from the content.
            assert!((fit.size.height + decor.height) * scale >= 163.0 * scale + frame_px - 1e-6);
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
            viewport_delta: 0.0,
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
                theme: Theme::Dark,
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
            assert_eq!(c.background, window_surface(Theme::Dark));
            let light = window_config(
                &WindowSpec {
                    theme: Theme::Light,
                    ..spec
                },
                &at,
                &place,
                (680.0, 300.0),
            );
            assert_eq!(light.background, window_surface(Theme::Light));
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
            pending: true,
        };
        assert_eq!(serde_json::to_value(&info).unwrap()["dialogId"], "a");
    }
}
