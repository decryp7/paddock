//! The pure rules of a dialog window: what the UI asks for and gets back (the wire types), request validation, the
//! window geometry by kind, the builder configuration (title-bar buttons disabled, owned or independent per OS), the
//! fit-to-content rules, and the shell's event names.
//!
//! Ported from Debrief's `dialog_windows.rs` (Debrief ADRs 0050 and 0054) without Debrief's labels, events or error type.

use crate::contract::{
    Contract, Kind, MAX_SIDE, Modality, Size, UNMEASURED_HEIGHT, valid_dialog_id,
};
use crate::placement::{self, Remembered, ScreenPosition};
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
/// To a dialog window: its size mode changed (a user resize made it `manual`, ADR 0043 §3). Payload: [`DialogSizeModeEvent`].
pub const SIZE_MODE_EVENT: &str = "paddock://dialog-size-mode";
/// To a dialog window: measure again (its scale changed and text may re-wrap, ADR 0043 §4). Payload: [`DialogRemeasureEvent`].
pub const REMEASURE_EVENT: &str = "paddock://dialog-remeasure";

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
    /// This page load of the window (1 for the first, raised by every load, also a reload): every measure the page sends
    /// carries it, so a reloaded page's `seq = 1` is not dropped as stale (ADR 0043 §2).
    #[cfg_attr(test, ts(type = "number"))]
    pub epoch: u64,
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
    /// The body scrolls (`scrollHeight > clientHeight + 1`): logged, so a scrollbar beside one line is seen in a user's
    /// bundle; never used to size (ADR 0043 §2).
    #[serde(default)]
    pub overflow: bool,
}

/// What the page sends with `dialog_ready` (ADR 0043 §2): measure `seq` of page load `epoch` is the one to show at. It
/// carries no size; the measure was sent with `dialog_measure`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DialogReadyWire {
    #[cfg_attr(test, ts(type = "number"))]
    pub epoch: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub seq: u64,
}

/// How a dialog window follows its content now, on the wire (ADR 0043 §1).
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DialogSizeModeName"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SizeModeName {
    /// The window keeps the size it was given.
    Manual,
    /// The height follows the content.
    Height,
}

/// The payload of [`SIZE_MODE_EVENT`]: the window's mode changed (a user resize made it `manual`); the page sets
/// `data-size-mode`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogSizeModeEvent {
    pub mode: SizeModeName,
}

/// The payload of [`REMEASURE_EVENT`]: the window's scale changed; the page measures again (text may re-wrap).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogRemeasureEvent {
    /// The window's new scale factor.
    pub scale: f64,
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
            overflow: false,
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
            overflow: false,
        }
    }

    fn size(width: f64, height: f64) -> Size {
        Size { width, height }
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
            epoch: 3,
        };
        assert_eq!(
            serde_json::to_value(&start).unwrap(),
            json!({ "dialogId": "a", "title": "t", "kind": "decision", "modality": "modal", "requiredChoice": true, "input": null, "epoch": 3 })
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
