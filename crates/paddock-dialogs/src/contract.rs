//! The declared contract of every dialog (ADR 0009): its kind, modality, minimum size and the IPC commands its window
//! may call. The app's TypeScript registry (`defineDialog`, plan S6) is the source; a JSON copy generated from it is
//! parsed here once (parse, don't validate) and answers "may dialog X call command Y?" for the command guard of
//! `paddock-commands` through [`Grant`].
//!
//! Ported from Debrief's `dialog_contracts.rs` (Debrief ADR 0054) and the kind policy of its `dialog-kind.ts`.

use paddock_commands::DialogGrant;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// The commands every dialog window may call whatever it shows (taking its input, answering, sizing itself, and, as every
/// window, posting a notice to the engine's centre and reading its host's list: ADR 0010, rule 34): the kind adds them,
/// a contract never lists them. An app adds its own base set in the contract file (`base`).
pub const BASE_COMMANDS: [&str; 5] = [
    "dialog_take_input",
    "dialog_close",
    "dialog_fit_height",
    "notifications_post",
    "notifications_list",
];
/// The longest dialog id and command name.
pub const MAX_NAME_LEN: usize = 64;
/// The largest contract file accepted.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
/// The most dialogs one contract file declares.
pub const MAX_DIALOGS: usize = 1024;
/// The most commands one dialog (or the base set) declares.
pub const MAX_COMMANDS: usize = 512;
/// The least a dialog window can be, logical px.
pub const DIALOG_MIN: Size = Size {
    width: 320.0,
    height: 200.0,
};
/// The viewer floor: results open at about 680x560 and only grow.
pub const VIEWER_MIN: Size = Size {
    width: 680.0,
    height: 560.0,
};
/// A tool's default size.
pub const TOOL_DEFAULT: Size = Size {
    width: 960.0,
    height: 680.0,
};
/// The size of a dialog that neither declares one nor has one from its kind.
pub const FALLBACK_SIZE: Size = Size {
    width: 640.0,
    height: 520.0,
};
/// The largest window side a dialog may ask for, logical px.
pub const MAX_SIDE: f64 = 4096.0;

/// The four kinds of dialog: the kind decides modality limits, size, buttons and keys (ADR 0009 §1).
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DialogKind"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Modal, fits its content, OK/Cancel.
    Decision,
    /// Modal or modeless, a comfortable size, pinned footer.
    Form,
    /// Modeless, opens at 680x560, grows and never shrinks, Close.
    Viewer,
    /// Modeless, large, remembers its size.
    Tool,
}

/// What a kind fixes, independent of any one dialog.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KindPolicy {
    /// The smallest window of the kind (the viewer floor), or none (then [`DIALOG_MIN`]).
    pub min_size: Option<Size>,
    /// The size the window opens at when the dialog asks for none (`None`: the request or [`FALLBACK_SIZE`] decides).
    pub default_size: Option<Size>,
    /// The window is fitted to its content (`dialog_fit_height`) unless the dialog names a height.
    pub fits_content: bool,
    /// A re-fit never makes the window shorter than it is (a result being read does not shrink under the reader).
    pub grows_only: bool,
}

impl Kind {
    /// Every kind, in a fixed order.
    pub const ALL: [Kind; 4] = [Kind::Decision, Kind::Form, Kind::Viewer, Kind::Tool];

    /// Whether a dialog of this kind may be `modality`: a decision is always modal, a viewer and a tool never, a form
    /// either.
    pub fn allows(self, modality: Modality) -> bool {
        match self {
            Kind::Decision => modality == Modality::Modal,
            Kind::Form => true,
            Kind::Viewer | Kind::Tool => modality == Modality::Modeless,
        }
    }

    /// The kind's size rules.
    pub const fn policy(self) -> KindPolicy {
        match self {
            Kind::Decision | Kind::Form => KindPolicy {
                min_size: None,
                default_size: None,
                fits_content: true,
                grows_only: false,
            },
            Kind::Viewer => KindPolicy {
                min_size: Some(VIEWER_MIN),
                default_size: Some(VIEWER_MIN),
                fits_content: true,
                grows_only: true,
            },
            Kind::Tool => KindPolicy {
                min_size: None,
                default_size: Some(TOOL_DEFAULT),
                fits_content: false,
                grows_only: false,
            },
        }
    }

    /// The kind's name on the wire.
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Decision => "decision",
            Kind::Form => "form",
            Kind::Viewer => "viewer",
            Kind::Tool => "tool",
        }
    }
}

/// Whether a dialog blocks the window that opened it.
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DialogModality"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    /// The user answers before going on; the parent is blocked.
    Modal,
    /// A window kept beside the work; the parent stays usable.
    Modeless,
}

/// A window size in logical px.
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DialogSize"))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    fn is_valid(&self) -> bool {
        [self.width, self.height]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0 && *v <= MAX_SIDE)
    }
}

/// One dialog's contract.
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DialogContract"))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Contract {
    pub kind: Kind,
    pub modality: Modality,
    /// A modal without a Cancel: Escape and a close request leave it open.
    #[serde(default)]
    #[cfg_attr(test, ts(as = "Option<bool>", optional))]
    pub required_choice: bool,
    /// One window per distinct input (the request names an instance key).
    #[serde(default)]
    #[cfg_attr(test, ts(as = "Option<bool>", optional))]
    pub instanced: bool,
    /// The smallest window of the dialog; the kind's floor applies when it is smaller or absent.
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub min_size: Option<Size>,
    /// The commands its window may call beyond the base ones; a trailing `*` is a prefix.
    #[serde(default)]
    #[cfg_attr(test, ts(as = "Option<Vec<String>>", optional))]
    pub commands: Vec<String>,
}

impl Contract {
    /// The smallest window of the dialog: its declared minimum, never below the kind's floor or [`DIALOG_MIN`].
    pub fn floor(&self) -> Size {
        let kind = self.kind.policy().min_size.unwrap_or(DIALOG_MIN);
        let own = self.min_size.unwrap_or(DIALOG_MIN);
        Size {
            width: own.width.max(kind.width),
            height: own.height.max(kind.height),
        }
    }
}

/// The contract file as the app's registry writes it.
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DialogContractFile"))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContractFile {
    /// Always 1.
    pub version: u32,
    /// The app's commands every dialog window may call (besides [`BASE_COMMANDS`]).
    #[serde(default)]
    #[cfg_attr(test, ts(as = "Option<Vec<String>>", optional))]
    pub base: Vec<String>,
    /// Every dialog by id.
    pub dialogs: BTreeMap<String, Contract>,
}

/// Why a contract file was refused: the message names the dialog and what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError(String);

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dialog contracts: {}", self.0)
    }
}

impl std::error::Error for ContractError {}

fn covers(declared: impl IntoIterator<Item = impl AsRef<str>>, command: &str) -> bool {
    declared
        .into_iter()
        .any(|d| match d.as_ref().strip_suffix('*') {
            Some(prefix) => command.starts_with(prefix),
            None => d.as_ref() == command,
        })
}

/// A command name, or a `prefix*`: `lower_snake_case`, 1 to 64 bytes.
pub fn valid_command(n: &str) -> bool {
    let n = n.strip_suffix('*').unwrap_or(n);
    !n.is_empty()
        && n.len() <= MAX_NAME_LEN
        && n.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A dialog id: lower-case letters, digits, `-` and `.` (the shell's namespaced contribution ids, `pw.create-app`),
/// 1 to 64 bytes.
pub fn valid_dialog_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_NAME_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}

/// All contracts of an app plus the commands every dialog window may call.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Contracts {
    base: Vec<String>,
    dialogs: BTreeMap<String, Contract>,
}

impl Contracts {
    /// Parses a contract file; fails on a file over [`MAX_FILE_BYTES`], a version it does not know, a kind/modality
    /// combination the kind forbids, a required choice that is not modal, a malformed id, command name or minimum
    /// size, or more dialogs or commands than the limits.
    pub fn parse(json: &str) -> Result<Self, ContractError> {
        if json.len() > MAX_FILE_BYTES {
            return Err(ContractError(format!(
                "the file is larger than {MAX_FILE_BYTES} bytes"
            )));
        }
        let file: ContractFile =
            serde_json::from_str(json).map_err(|e| ContractError(e.to_string()))?;
        Self::from_file(file)
    }

    /// Validates a contract file already read.
    pub fn from_file(file: ContractFile) -> Result<Self, ContractError> {
        let bad = |m: String| Err(ContractError(m));
        if file.version != 1 {
            return bad(format!("unknown version {}", file.version));
        }
        if file.dialogs.len() > MAX_DIALOGS {
            return bad(format!("more than {MAX_DIALOGS} dialogs"));
        }
        if file.base.len() > MAX_COMMANDS {
            return bad(format!("more than {MAX_COMMANDS} base commands"));
        }
        if let Some(c) = file.base.iter().find(|c| !valid_command(c)) {
            return bad(format!("bad base command {c:?}"));
        }
        for (id, c) in &file.dialogs {
            if !valid_dialog_id(id) {
                return bad(format!("bad dialog id {id:?}"));
            }
            if !c.kind.allows(c.modality) {
                return bad(format!(
                    "{id}: a {} dialog cannot be {:?}",
                    c.kind.as_str(),
                    c.modality
                ));
            }
            if c.required_choice && c.modality != Modality::Modal {
                return bad(format!("{id}: only a modal can be a required choice"));
            }
            if c.min_size.is_some_and(|s| !s.is_valid()) {
                return bad(format!("{id}: bad minimum size"));
            }
            if c.commands.len() > MAX_COMMANDS {
                return bad(format!("{id}: more than {MAX_COMMANDS} commands"));
            }
            if let Some(n) = c.commands.iter().find(|n| !valid_command(n)) {
                return bad(format!("{id}: bad command {n:?}"));
            }
        }
        Ok(Self {
            base: file.base,
            dialogs: file.dialogs,
        })
    }

    /// No dialogs at all (what an app falls back to when its contract file is unusable).
    pub fn empty() -> Self {
        Self::default()
    }

    /// The contract of dialog `id`.
    pub fn get(&self, id: &str) -> Option<&Contract> {
        self.dialogs.get(id)
    }

    /// Every dialog id, sorted.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.dialogs.keys().map(String::as_str)
    }

    /// The app's base commands (without [`BASE_COMMANDS`]).
    pub fn base(&self) -> &[String] {
        &self.base
    }

    /// Every dialog's id and declared commands, as `paddock_commands::Table::contract_problems` takes them.
    pub fn declared(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.dialogs
            .iter()
            .map(|(id, c)| (id.as_str(), c.commands.as_slice()))
    }

    /// What is wrong with these contracts against the app's command table (every base and declared command exists and
    /// is open to dialogs, none is host-only). Empty when they agree; each app runs it in a test (rule 40).
    pub fn problems(&self, table: &paddock_commands::Table, host_only: &[&str]) -> Vec<String> {
        table.contract_problems(&self.base, self.declared(), host_only)
    }

    /// Whether the window of dialog `id` may call `command`: a shell base command, an app base command, or one its
    /// contract declares. An unknown dialog may call nothing.
    pub fn allows(&self, id: &str, command: &str) -> bool {
        self.get(id).is_some_and(|c| {
            covers(BASE_COMMANDS, command)
                || covers(&self.base, command)
                || covers(&c.commands, command)
        })
    }
}

/// What one dialog window may call: the contracts and the id of the dialog it shows, as the dialog layer recorded them
/// when it opened the window (never the webview's word). The command guard's hook ([`DialogGrant`]).
#[derive(Debug, Clone)]
pub struct Grant {
    contracts: Arc<Contracts>,
    dialog: String,
}

impl Grant {
    /// The grant of dialog `dialog` under `contracts`.
    pub fn new(contracts: Arc<Contracts>, dialog: impl Into<String>) -> Self {
        Grant {
            contracts,
            dialog: dialog.into(),
        }
    }
}

impl DialogGrant for Grant {
    fn dialog(&self) -> &str {
        &self.dialog
    }

    fn allows(&self, command: &str) -> bool {
        self.contracts.allows(&self.dialog, command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "version": 1,
        "base": ["ui_log"],
        "dialogs": {
            "about": { "kind": "viewer", "modality": "modeless", "requiredChoice": false, "instanced": false,
                       "minSize": { "width": 680, "height": 560 }, "commands": ["app_status", "ui_*"] },
            "close-prompt": { "kind": "decision", "modality": "modal", "requiredChoice": true, "instanced": false,
                              "minSize": null, "commands": [] }
        }
    }"#;

    #[test]
    fn a_dialog_may_call_the_base_commands_and_its_own_only() {
        let c = Contracts::parse(SAMPLE).unwrap();
        assert!(
            c.allows("about", "dialog_close")
                && c.allows("about", "app_status")
                && c.allows("about", "ui_sync_x")
        );
        assert!(
            !c.allows("about", "resolve_close"),
            "another dialog's command"
        );
        assert!(
            c.allows("close-prompt", "dialog_close") && !c.allows("close-prompt", "app_status")
        );
        assert!(c.allows("close-prompt", "ui_log"), "the app's base set");
        for base in BASE_COMMANDS {
            assert!(c.allows("close-prompt", base), "{base}");
        }
        assert!(
            !c.allows("unknown", "dialog_close"),
            "an unknown dialog may call nothing"
        );
        assert_eq!(
            c.get("about").and_then(|a| a.min_size),
            Some(Size {
                width: 680.0,
                height: 560.0
            })
        );
        assert_eq!(c.ids().collect::<Vec<_>>(), ["about", "close-prompt"]);
        assert_eq!(c.base(), ["ui_log"]);
        let declared: Vec<_> = c.declared().collect();
        assert_eq!(declared[0].0, "about");
        assert_eq!(declared[0].1, ["app_status", "ui_*"]);
        assert_eq!(Contracts::empty().ids().count(), 0);
    }

    #[test]
    fn optional_fields_default_and_the_floor_is_the_kinds_at_least() {
        let c = Contracts::parse(
            r#"{ "version": 1, "dialogs": {
                "v": { "kind": "viewer", "modality": "modeless" },
                "f": { "kind": "form", "modality": "modal", "minSize": { "width": 500, "height": 100 } },
                "t": { "kind": "tool", "modality": "modeless", "minSize": { "width": 300, "height": 900 } } } }"#,
        )
        .unwrap();
        let v = c.get("v").unwrap();
        assert!(
            !v.required_choice && !v.instanced && v.commands.is_empty() && v.min_size.is_none()
        );
        assert_eq!(v.floor(), VIEWER_MIN);
        assert_eq!(
            c.get("f").unwrap().floor(),
            Size {
                width: 500.0,
                height: 200.0
            }
        );
        assert_eq!(
            c.get("t").unwrap().floor(),
            Size {
                width: 320.0,
                height: 900.0
            }
        );
        assert!(c.base().is_empty());
    }

    #[test]
    fn a_contract_file_that_contradicts_itself_or_breaks_a_limit_is_refused() {
        let with = |from: &str, to: &str| SAMPLE.replace(from, to);
        let many_dialogs = format!(
            r#"{{ "version": 1, "dialogs": {{ {} }} }}"#,
            (0..=MAX_DIALOGS)
                .map(|i| format!(r#""d{i}": {{ "kind": "tool", "modality": "modeless" }}"#))
                .collect::<Vec<_>>()
                .join(",")
        );
        let commands = format!(
            "[{}]",
            (0..=MAX_COMMANDS)
                .map(|i| format!(r#""c{i}""#))
                .collect::<Vec<_>>()
                .join(",")
        );
        for (what, json, says) in [
            (
                "a modeless decision",
                with(
                    r#""kind": "decision", "modality": "modal""#,
                    r#""kind": "decision", "modality": "modeless""#,
                ),
                "cannot be",
            ),
            (
                "a modal viewer",
                with(
                    r#""kind": "viewer", "modality": "modeless""#,
                    r#""kind": "viewer", "modality": "modal""#,
                ),
                "cannot be",
            ),
            (
                "an unknown field",
                with(
                    r#""requiredChoice": true"#,
                    r#""requiredChoice": true, "x": 1"#,
                ),
                "unknown field",
            ),
            (
                "version 2",
                with(r#""version": 1"#, r#""version": 2"#),
                "version 2",
            ),
            (
                "a bad command",
                with(r#""app_status""#, r#""App Status""#),
                "bad command",
            ),
            (
                "a bad base command",
                with(r#""ui_log""#, r#""ui log""#),
                "bad base",
            ),
            (
                "a bad id",
                with(r#""about""#, r#""../about""#),
                "bad dialog id",
            ),
            (
                "a bad minimum size",
                with(r#""width": 680"#, r#""width": -1"#),
                "minimum size",
            ),
            (
                "a huge minimum size",
                with(r#""height": 560"#, r#""height": 1e9"#),
                "minimum size",
            ),
            ("too many dialogs", many_dialogs, "more than 1024 dialogs"),
            (
                "too many commands",
                with(r#"["app_status", "ui_*"]"#, &commands),
                "more than 512 commands",
            ),
            (
                "too many base commands",
                with(r#"["ui_log"]"#, &commands),
                "base commands",
            ),
            ("not json", "{".into(), "dialog contracts"),
            (
                "a huge file",
                format!("{SAMPLE}{}", " ".repeat(MAX_FILE_BYTES)),
                "larger",
            ),
        ] {
            let e = Contracts::parse(&json).expect_err(what).to_string();
            assert!(
                e.starts_with("dialog contracts: ") && e.contains(says),
                "{what}: {e}"
            );
        }
    }

    #[test]
    fn only_a_modal_can_be_a_required_choice() {
        let json = SAMPLE.replace(
            r#""kind": "viewer", "modality": "modeless", "requiredChoice": false"#,
            r#""kind": "form", "modality": "modeless", "requiredChoice": true"#,
        );
        let e = Contracts::parse(&json).unwrap_err();
        assert!(e.to_string().contains("required choice"));
        let _: &dyn std::error::Error = &e;
    }

    #[test]
    fn kinds_limit_modality_and_set_the_size_rules() {
        assert!(
            Kind::Decision.allows(Modality::Modal) && !Kind::Decision.allows(Modality::Modeless)
        );
        assert!(Kind::Form.allows(Modality::Modal) && Kind::Form.allows(Modality::Modeless));
        for k in [Kind::Viewer, Kind::Tool] {
            assert!(k.allows(Modality::Modeless) && !k.allows(Modality::Modal));
        }
        let names: Vec<_> = Kind::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(names, ["decision", "form", "viewer", "tool"]);
        for k in Kind::ALL {
            assert_eq!(serde_json::to_value(k).unwrap(), k.as_str());
        }
        assert_eq!(Kind::Viewer.policy().min_size, Some(VIEWER_MIN));
        assert!(Kind::Viewer.policy().grows_only && Kind::Viewer.policy().fits_content);
        assert_eq!(Kind::Tool.policy().default_size, Some(TOOL_DEFAULT));
        assert!(!Kind::Tool.policy().fits_content);
        assert!(Kind::Decision.policy().fits_content && !Kind::Decision.policy().grows_only);
        assert_eq!(Kind::Form.policy().default_size, None);
    }

    #[test]
    fn reads_the_contract_file_the_shell_writes_from_a_registry() {
        // Written by `dialogContractFile` (shell/dialogs) from the shell demo's registry (plan S6).
        let c = Contracts::parse(include_str!(
            "../../../ui/projects/shell-demo/dialog-contracts.json"
        ))
        .unwrap();
        assert_eq!(
            c.ids().collect::<Vec<_>>(),
            [
                "pd.about",
                "pd.channels.secret",
                "pd.report-problem",
                "pd.settings",
                "pd.updates",
                "pd.whats-new",
                "sd.about",
                "sd.choose-style",
                "sd.confirm-reset",
                "sd.inspector",
                "sd.rename"
            ]
        );
        assert!(c.allows("sd.inspector", "demo_status") && !c.allows("sd.about", "demo_status"));
        assert!(c.get("sd.choose-style").unwrap().required_choice);
        assert_eq!(c.get("sd.about").unwrap().floor(), VIEWER_MIN);
    }

    #[test]
    fn names_and_ids_follow_the_schemes() {
        for ok in ["a", "dialog_close", "ui_*", "x1"] {
            assert!(valid_command(ok), "{ok}");
        }
        for bad in ["", "*", "A", "a-b", "a b", &"a".repeat(65)] {
            assert!(!valid_command(bad), "{bad}");
        }
        for ok in [
            "about",
            "close-prompt",
            "a1",
            "pw.create-app",
            "sd.confirm-reset",
        ] {
            assert!(valid_dialog_id(ok), "{ok}");
        }
        for bad in ["", "About", "a_b", "a/b", &"a".repeat(65)] {
            assert!(!valid_dialog_id(bad), "{bad}");
        }
    }

    #[test]
    fn a_grant_answers_for_its_dialog_only() {
        let c = Arc::new(Contracts::parse(SAMPLE).unwrap());
        let g = Grant::new(c.clone(), "about");
        assert_eq!(g.dialog(), "about");
        assert!(g.allows("app_status") && g.allows("dialog_close") && !g.allows("resolve_close"));
        let gone = Grant::new(c, "removed");
        assert!(
            !gone.allows("dialog_close"),
            "a dialog without a contract may call nothing"
        );
        assert!(format!("{gone:?}").contains("removed"));
    }

    #[test]
    fn contracts_are_checked_against_the_command_table() {
        paddock_commands::command_table! {
            static TABLE;
            Query [WINDOW] => app_status,
            Mutation [MAIN] => resolve_close,
            Query [WINDOW] => ui_sync_theme,
            Quiet [WINDOW] => ui_log,
        }
        let c = Contracts::parse(SAMPLE).unwrap();
        assert_eq!(c.problems(&TABLE, &[]), Vec::<String>::new());
        let p = c.problems(&TABLE, &["app_status"]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(
            p[0].contains("about") && p[0].contains("app_status"),
            "{p:?}"
        );
        let bad = Contracts::parse(&SAMPLE.replace("app_status", "resolve_close")).unwrap();
        assert!(bad.problems(&TABLE, &[])[0].contains("does not open to dialogs"));
    }
}
