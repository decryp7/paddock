//! The shell's window label scheme (plan §5.1): `main`, other top-level windows `w-<n>`, dialogs `d-<host>-<n>` and
//! pop-out panels `p-<host>-<n>`, where a host is `main` or a `w-<n>`, and the tray popovers `tray` and `tray-<item>`
//! (plan S12). Every Paddock app uses it, so the command guard, the dialog and pop-out layers and the capabilities
//! agree on which webview is which.

use serde::Serialize;
use std::fmt;

/// The label of the first window.
pub const MAIN_LABEL: &str = "main";
/// The prefix of the other top-level windows (`w-2`).
pub const WINDOW_PREFIX: &str = "w-";
/// The prefix of dialog windows (`d-main-1`).
pub const DIALOG_PREFIX: &str = "d-";
/// The prefix of pop-out panel windows (`p-main-1`).
pub const POPOUT_PREFIX: &str = "p-";
/// The label of the tray popover (the one opened by a click on the tray icon).
pub const TRAY_LABEL: &str = "tray";
/// The prefix of an app's other tray popovers (`tray-cpu`, one per tray item).
pub const TRAY_PREFIX: &str = "tray-";
/// The longest item id of a `tray-<item>` popover.
pub const MAX_TRAY_ITEM: usize = 32;

/// The class of a window for the command table.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowClass {
    /// `main` and `w-<n>`.
    Main,
    /// `p-<host>-<n>`.
    Popout,
    /// `d-<host>-<n>`.
    Dialog,
    /// `tray` and `tray-<item>`: a tray popover. Not part of [`Windows::WINDOW`](crate::Windows::WINDOW): a popover may
    /// call only the commands that name `TRAY` (least privilege for an always-on-top surface).
    Tray,
}

impl WindowClass {
    /// Every class, in a fixed order.
    pub const ALL: [WindowClass; 4] = [
        WindowClass::Main,
        WindowClass::Popout,
        WindowClass::Dialog,
        WindowClass::Tray,
    ];

    /// The class's name on the wire and in generated TypeScript.
    pub const fn as_str(self) -> &'static str {
        match self {
            WindowClass::Main => "main",
            WindowClass::Popout => "popout",
            WindowClass::Dialog => "dialog",
            WindowClass::Tray => "tray",
        }
    }
}

/// A parsed window label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Label<'a> {
    /// `main`.
    Main,
    /// `w-<n>`.
    Window(u32),
    /// `d-<host>-<n>`: dialog `n` of the top-level window `host`.
    Dialog { host: &'a str, n: u32 },
    /// `p-<host>-<n>`: pop-out `n` of the top-level window `host`.
    Popout { host: &'a str, n: u32 },
    /// `tray` (`item: None`) or `tray-<item>`: a tray popover. It belongs to no window.
    Tray { item: Option<&'a str> },
}

/// A tray item id: lowercase ASCII letters, digits and dashes, 1 to [`MAX_TRAY_ITEM`] characters, starting with a letter
/// or digit (it becomes part of a window label and of a route).
pub fn valid_tray_item(id: &str) -> bool {
    (1..=MAX_TRAY_ITEM).contains(&id.len())
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A decimal number of ASCII digits only (no sign, no spaces).
fn number(s: &str) -> Option<u32> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// `<host>-<n>` where the host is a top-level window label.
fn hosted(rest: &str) -> Option<(&str, u32)> {
    let (host, n) = rest.rsplit_once('-')?;
    let n = number(n)?;
    matches!(Label::parse(host)?, Label::Main | Label::Window(_)).then_some((host, n))
}

impl<'a> Label<'a> {
    /// The label `label`, or `None` when it is not one of the scheme's (another webview: a page, a plugin's view).
    pub fn parse(label: &'a str) -> Option<Label<'a>> {
        if label == MAIN_LABEL {
            Some(Label::Main)
        } else if let Some(n) = label.strip_prefix(WINDOW_PREFIX) {
            number(n).map(Label::Window)
        } else if let Some(rest) = label.strip_prefix(DIALOG_PREFIX) {
            hosted(rest).map(|(host, n)| Label::Dialog { host, n })
        } else if let Some(rest) = label.strip_prefix(POPOUT_PREFIX) {
            hosted(rest).map(|(host, n)| Label::Popout { host, n })
        } else if label == TRAY_LABEL {
            Some(Label::Tray { item: None })
        } else if let Some(item) = label.strip_prefix(TRAY_PREFIX) {
            valid_tray_item(item).then_some(Label::Tray { item: Some(item) })
        } else {
            None
        }
    }

    pub fn class(&self) -> WindowClass {
        match self {
            Label::Main | Label::Window(_) => WindowClass::Main,
            Label::Dialog { .. } => WindowClass::Dialog,
            Label::Popout { .. } => WindowClass::Popout,
            Label::Tray { .. } => WindowClass::Tray,
        }
    }

    /// The top-level window this one works for: a dialog's or pop-out's host; `None` for a top-level window itself and
    /// for a tray popover.
    pub fn host(&self) -> Option<&'a str> {
        match self {
            Label::Main | Label::Window(_) | Label::Tray { .. } => None,
            Label::Dialog { host, .. } | Label::Popout { host, .. } => Some(host),
        }
    }
}

impl fmt::Display for Label<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Label::Main => f.write_str(MAIN_LABEL),
            Label::Window(n) => write!(f, "{WINDOW_PREFIX}{n}"),
            Label::Dialog { host, n } => write!(f, "{DIALOG_PREFIX}{host}-{n}"),
            Label::Popout { host, n } => write!(f, "{POPOUT_PREFIX}{host}-{n}"),
            Label::Tray { item: None } => f.write_str(TRAY_LABEL),
            Label::Tray { item: Some(item) } => write!(f, "{TRAY_PREFIX}{item}"),
        }
    }
}

/// The window class of the webview labelled `label`; `None` for a webview outside the scheme, which may call nothing
/// of a table.
pub fn class_of(label: &str) -> Option<WindowClass> {
    Label::parse(label).map(|l| l.class())
}

/// The top-level window whose state a label works on: a dialog's or pop-out's host, else the label itself.
pub fn resolve(label: &str) -> &str {
    Label::parse(label).and_then(|l| l.host()).unwrap_or(label)
}
