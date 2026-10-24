//! The table's runtime checks, as a pure function of the command, the calling webview's label, the dialog grant and the
//! request size: a pop-out gets a smaller set than its host window, a webview outside the label scheme none, a dialog
//! only what its own contract declares (ADR 0009, Debrief ADR 0054), and an oversized request is refused before its
//! arguments are parsed. The Tauri adapter ([`crate::tauri::Guard`]) runs it on every call.

use crate::labels::{WindowClass, class_of};
use crate::table::Table;
use serde::Serialize;

/// What one dialog window may call: given by the dialog layer (`paddock-dialogs`: the dialog's id and its declared
/// contract). The guard asks it only for commands the table opens to dialogs at all.
pub trait DialogGrant {
    /// The id of the dialog the window shows (for messages and logs).
    fn dialog(&self) -> &str;
    /// Whether the dialog's contract (its own commands plus the base ones) covers `command`.
    fn allows(&self, command: &str) -> bool;
}

/// Why a call was refused (camelCase on the wire, the shape of an app's command error: `code`, `message`, `retryable`).
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "RefusalCode"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RefusalCode {
    /// The calling window (or dialog) may not call the command.
    Denied,
    /// The request is larger than the command's limit.
    Limit,
}

/// A refused call: what the UI receives as the command's error, and what is logged.
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "CommandRefusal"))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    pub code: RefusalCode,
    /// For the user, as is. A UI recognises a denial by `code` (the wording "`<command>` is not available in " is kept
    /// stable for UIs that match on it).
    pub message: String,
    /// Always false: the same call is refused again.
    retryable: bool,
    /// The refused command.
    pub command: String,
    /// The dialog whose window called it, when the caller is a dialog the dialog layer knows.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dialog: Option<String>,
}

impl Refusal {
    fn new(code: RefusalCode, command: &str, dialog: Option<&str>, message: String) -> Refusal {
        Refusal {
            code,
            message,
            retryable: false,
            command: command.to_string(),
            dialog: dialog.map(str::to_string),
        }
    }

    /// Whether the call is a dialog calling what its contract does not declare: a bug in the app (log it as an error
    /// naming the dialog and the command); any other refusal is a warning.
    pub fn breaches_contract(&self) -> bool {
        self.code == RefusalCode::Denied && self.dialog.is_some()
    }

    /// The log line for the refusal.
    pub fn log_message(&self) -> String {
        match &self.dialog {
            Some(id) if self.breaches_contract() => {
                format!(
                    "The {id} dialog called {}, which its contract does not declare: {}",
                    self.command, self.message
                )
            }
            _ => format!("A command was refused: {}", self.message),
        }
    }
}

/// Whether the webview `label` may call `command` with a request of `size` bytes; why not.
///
/// - Plugin commands (`plugin:...`) pass: the capabilities govern them. Unknown commands pass: Tauri answers that the
///   command does not exist.
/// - The label's class must be one the entry names; a webview outside the label scheme may call nothing.
/// - A dialog window may call only what its `grant` allows; a dialog window without a grant (unknown to the dialog layer)
///   may call nothing.
/// - The request may be at most the entry's limit.
pub fn check(
    table: &Table,
    command: &str,
    label: &str,
    grant: Option<&dyn DialogGrant>,
    size: usize,
) -> Result<(), Refusal> {
    if command.starts_with("plugin:") {
        return Ok(());
    }
    let Some(spec) = table.spec(command) else {
        return Ok(());
    };
    let class = class_of(label);
    let dialog = grant.map(|g| g.dialog());
    if !class.is_some_and(|c| spec.windows().contains(c)) {
        return Err(Refusal::new(
            RefusalCode::Denied,
            command,
            None,
            format!("{command} is not available in this window"),
        ));
    }
    if class == Some(WindowClass::Dialog) && !grant.is_some_and(|g| g.allows(command)) {
        let message = match dialog {
            Some(id) => format!(
                "{command} is not available in the {id} dialog (its contract does not declare it)"
            ),
            None => format!("{command} is not available in an unknown dialog"),
        };
        return Err(Refusal::new(RefusalCode::Denied, command, dialog, message));
    }
    if size > spec.limit() {
        return Err(Refusal::new(
            RefusalCode::Limit,
            command,
            dialog.filter(|_| class == Some(WindowClass::Dialog)),
            format!(
                "the request to {command} is too large ({size} bytes; at most {})",
                spec.limit()
            ),
        ));
    }
    Ok(())
}
