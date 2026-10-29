//! The contract of every dialog at run time (ADR 0009, ADR 0026 §3): its kind, modality, size and the IPC commands its
//! window may call, built from the owners' static tables ([`crate::table`], [`Contracts::from_tables`]) and answering
//! "may dialog X call command Y?" for the command guard of `paddock-commands` through [`Grant`].
//!
//! The kind table ([`Kind::policy`]) is the one "is fitted" table of ADR 0016 decision 7: the TypeScript side reads it
//! from the generated `dialog-kinds.ts` ([`crate::ts::kinds_module`]) and never writes it again.

use crate::table::{DialogTable, Entry, Width};
use paddock_commands::{DialogCallable, DialogGrant};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// The commands every dialog window may call whatever it shows (taking its input, answering, sizing and showing itself, and, as every
/// window, posting a notice to the engine's centre and reading its host's list: ADR 0010, rule 34): the kind adds them,
/// a contract never lists them. An app adds its own base set in its table's head (`base &[...]`).
pub const BASE_COMMANDS: [&str; 6] = [
    "dialog_take_input",
    "dialog_close",
    "dialog_ready",
    "dialog_fit",
    "notifications_post",
    "notifications_list",
];
/// The longest dialog id and command name.
pub const MAX_NAME_LEN: usize = 64;
/// The most dialogs one table declares.
pub const MAX_DIALOGS: usize = 1024;
/// The most commands one dialog (or the base set) declares.
pub const MAX_COMMANDS: usize = 512;
/// The least a dialog window can be, logical px.
pub const DIALOG_MIN: Size = Size {
    width: 320.0,
    height: 200.0,
};
/// The viewer floor: results open at their preferred size and only grow.
pub const VIEWER_MIN: Size = Size {
    width: 680.0,
    height: 560.0,
};
/// A tool's default size.
pub const TOOL_DEFAULT: Size = Size {
    width: 960.0,
    height: 680.0,
};
/// The height a measured dialog (a decision or a form) is created at before its content is measured.
pub const UNMEASURED_HEIGHT: f64 = 520.0;
/// The largest share of the work area a dialog's preferred size takes (ADR 0026 §1); never below its minimum content.
pub const CAP_FRACTION: f64 = 0.75;
/// The largest window side a dialog may ask for, logical px.
pub const MAX_SIDE: f64 = 4096.0;

/// The four kinds of dialog: the kind decides modality limits, size, buttons and keys (ADR 0009 §1, ADR 0026 §1).
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DialogKind"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Modal, narrow, measured; Cancel and one affirmative.
    Decision,
    /// Modal or modeless, medium, measured; pinned footer.
    Form,
    /// Modeless, wide, a preferred height that grows to the cap; Close.
    Viewer,
    /// Modeless, user-sized, remembers its size.
    Tool,
}

/// What a dialog remembers between openings (ADR 0026 §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Remembers {
    /// Its offset from the parent only (decisions).
    Offset,
    /// Its width and offset (forms).
    WidthAndOffset,
    /// Its size and offset, clamped (viewers and tools).
    SizeAndOffset,
}

impl Remembers {
    /// The name in generated TypeScript.
    pub const fn as_str(self) -> &'static str {
        match self {
            Remembers::Offset => "offset",
            Remembers::WidthAndOffset => "widthAndOffset",
            Remembers::SizeAndOffset => "sizeAndOffset",
        }
    }
}

/// What a kind fixes, independent of any one dialog.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KindPolicy {
    /// The smallest window of the kind (the viewer floor), or none (then [`DIALOG_MIN`]).
    pub min_size: Option<Size>,
    /// The width a dialog of the kind opens at when it declares none.
    pub default_width: Width,
    /// The preferred height when the dialog declares none (`None`: measured).
    pub default_height: Option<f64>,
    /// The window is fitted to its content.
    pub fits_content: bool,
    /// A re-fit never makes the window shorter than it is (a result being read does not shrink under the reader).
    pub grows_only: bool,
    /// What it remembers between openings.
    pub remembers: Remembers,
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

    /// The kind's size rules (the table of ADR 0026 §1).
    pub const fn policy(self) -> KindPolicy {
        match self {
            Kind::Decision => KindPolicy {
                min_size: None,
                default_width: Width::Narrow,
                default_height: None,
                fits_content: true,
                grows_only: false,
                remembers: Remembers::Offset,
            },
            Kind::Form => KindPolicy {
                min_size: None,
                default_width: Width::Medium,
                default_height: None,
                fits_content: true,
                grows_only: false,
                remembers: Remembers::WidthAndOffset,
            },
            Kind::Viewer => KindPolicy {
                min_size: Some(VIEWER_MIN),
                default_width: Width::Wide,
                default_height: Some(VIEWER_MIN.height),
                fits_content: true,
                grows_only: true,
                remembers: Remembers::SizeAndOffset,
            },
            Kind::Tool => KindPolicy {
                min_size: None,
                default_width: Width::Px(TOOL_DEFAULT.width as u16),
                default_height: Some(TOOL_DEFAULT.height),
                fits_content: false,
                grows_only: false,
                remembers: Remembers::SizeAndOffset,
            },
        }
    }

    /// The width a dialog of the kind opens at when it declares none, logical px.
    pub const fn default_width(self) -> f64 {
        self.policy().default_width.px()
    }

    /// The preferred height of the kind (`None`: measured).
    pub const fn default_height(self) -> Option<f64> {
        self.policy().default_height
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

impl Modality {
    /// The modality's name on the wire.
    pub const fn as_str(self) -> &'static str {
        match self {
            Modality::Modal => "modal",
            Modality::Modeless => "modeless",
        }
    }
}

/// A window size in logical px.
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "DialogSize"))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// One dialog's contract at run time.
#[derive(Debug, Clone, PartialEq)]
pub struct Contract {
    pub kind: Kind,
    pub modality: Modality,
    /// A modal without a way out: Escape and a close request leave it open.
    pub required_choice: bool,
    /// One window per distinct input (the request names an instance key).
    pub instanced: bool,
    /// The width it opens at, logical px.
    pub width: f64,
    /// Its preferred height (a viewer or a tool), logical px; `None`: measured.
    pub height: Option<f64>,
    /// The commands its window may call beyond the base ones; a trailing `*` is a prefix.
    pub commands: Vec<String>,
}

impl Contract {
    /// The run-time contract of a table entry.
    pub fn of(entry: &Entry) -> Contract {
        Contract {
            kind: entry.shape.kind(),
            modality: entry.shape.modality(),
            required_choice: entry.required_choice,
            instanced: entry.instanced,
            width: entry.width_px(),
            height: entry.height_px(),
            commands: entry.commands.iter().map(|c| c.wire()).collect(),
        }
    }

    /// The smallest window of the dialog: the kind's floor, never below [`DIALOG_MIN`].
    pub fn floor(&self) -> Size {
        let kind = self.kind.policy().min_size.unwrap_or(DIALOG_MIN);
        Size {
            width: kind.width.max(DIALOG_MIN.width),
            height: kind.height.max(DIALOG_MIN.height),
        }
    }
}

/// Why the tables could not be registered: the message names the dialog and what is wrong.
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
    crate::table::valid_id(id)
}

/// All contracts of an app plus the commands every dialog window may call.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Contracts {
    base: Vec<String>,
    dialogs: BTreeMap<String, Contract>,
}

impl Contracts {
    /// The contracts of every dialog of `tables` (the shell's, a feature's, the app's own), with the union of their base
    /// commands. Each table was validated when it was built; together they must not declare one id twice.
    pub fn from_tables(tables: &[&DialogTable]) -> Result<Self, ContractError> {
        let mut out = Contracts::default();
        for t in tables {
            out.add(t.entries(), t.base())?;
        }
        Ok(out)
    }

    /// The contracts of `entries` with the base commands `base`, checked by the rules of [`DialogTable::validate`]: for
    /// entries built at run time (tests, tools). An app registers its static tables with [`Contracts::from_tables`].
    pub fn from_entries(entries: &[Entry], base: &[DialogCallable]) -> Result<Self, ContractError> {
        DialogTable::validate(entries, base).map_err(|e| ContractError(format!("{e:?}")))?;
        let mut out = Contracts::default();
        out.add(entries, base)?;
        Ok(out)
    }

    fn add(&mut self, entries: &[Entry], base: &[DialogCallable]) -> Result<(), ContractError> {
        for b in base {
            let w = b.wire();
            if !self.base.contains(&w) {
                self.base.push(w);
            }
        }
        self.base.sort();
        for e in entries {
            if self
                .dialogs
                .insert(e.id.to_string(), Contract::of(e))
                .is_some()
            {
                return Err(ContractError(format!("{} is declared by two tables", e.id)));
            }
        }
        Ok(())
    }

    /// No dialogs at all.
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
    /// is open to dialogs, none is host-only). The markers make the first two compile-time checks for a single owner;
    /// this one also catches a command that two tables would answer to. Empty when they agree.
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
    use crate::table::Shape;

    mod app {
        paddock_commands::command_table! {
            pub static TABLE;
            Query [WINDOW] => app_status,
            Mutation [MAIN] => resolve_close,
            Query [WINDOW] => ui_sync_theme,
            Quiet [WINDOW] => ui_log,
        }
    }
    use app::dialog_callable as cmd;

    crate::dialogs! {
        static SAMPLE, base &[cmd::ui_log];
        "about" => Viewer { commands: &[cmd::app_status, prefix(cmd::ALL, "ui_")] },
        "close-prompt" => Decision { required_choice: true },
    }

    crate::dialogs! {
        static OTHER, base &[cmd::ui_log, cmd::app_status];
        "f" => Form(Modal) { width: Some(Wide) },
        "t" => Tool { width: Some(Px(820)), height: Some(640) },
    }

    fn sample() -> Contracts {
        Contracts::from_tables(&[&SAMPLE]).unwrap()
    }

    #[test]
    fn a_dialog_may_call_the_base_commands_and_its_own_only() {
        let c = sample();
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
        assert_eq!(c.ids().collect::<Vec<_>>(), ["about", "close-prompt"]);
        assert_eq!(c.base(), ["ui_log"]);
        let declared: Vec<_> = c.declared().collect();
        assert_eq!(declared[0].0, "about");
        assert_eq!(declared[0].1, ["app_status", "ui_*"]);
        assert_eq!(Contracts::empty().ids().count(), 0);
        assert!(c.get("close-prompt").unwrap().required_choice);
    }

    #[test]
    fn tables_merge_their_dialogs_and_base_sets_and_never_one_id_twice() {
        let c = Contracts::from_tables(&[&SAMPLE, &OTHER]).unwrap();
        assert_eq!(c.ids().count(), 4);
        assert_eq!(
            c.base(),
            ["app_status", "ui_log"],
            "the union, sorted, once each"
        );
        let f = c.get("f").unwrap();
        assert_eq!(
            (f.kind, f.modality, f.width, f.height),
            (Kind::Form, Modality::Modal, 720.0, None)
        );
        let t = c.get("t").unwrap();
        assert_eq!((t.width, t.height), (820.0, Some(640.0)));
        let e = Contracts::from_tables(&[&SAMPLE, &SAMPLE]).unwrap_err();
        assert_eq!(
            e.to_string(),
            "dialog contracts: about is declared by two tables"
        );
        let _: &dyn std::error::Error = &e;
        let run_time = [Entry::new("r", Shape::Decision)];
        assert_eq!(
            Contracts::from_entries(&run_time, &[])
                .unwrap()
                .ids()
                .collect::<Vec<_>>(),
            ["r"]
        );
        let twice = [
            Entry::new("r", Shape::Decision),
            Entry::new("r", Shape::Tool),
        ];
        assert!(
            Contracts::from_entries(&twice, &[])
                .unwrap_err()
                .to_string()
                .contains("Duplicate")
        );
    }

    #[test]
    fn a_contract_takes_its_sizes_from_the_kind_unless_declared() {
        let v = Contract::of(&Entry::new("v", Shape::Viewer));
        assert_eq!(
            (v.width, v.height, v.floor()),
            (720.0, Some(560.0), VIEWER_MIN)
        );
        let d = Contract::of(&Entry::new("d", Shape::Decision));
        assert_eq!((d.width, d.height, d.floor()), (400.0, None, DIALOG_MIN));
        let p = Contract::of(&Entry {
            commands: {
                const C: &[DialogCallable] = &[paddock_commands::plugin("dialog", "open")];
                C
            },
            ..Entry::new("p", Shape::Tool)
        });
        assert_eq!(p.commands, ["plugin:dialog|open"]);
        assert_eq!((p.width, p.height), (960.0, Some(680.0)));
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
        for m in [Modality::Modal, Modality::Modeless] {
            assert_eq!(serde_json::to_value(m).unwrap(), m.as_str());
        }
        assert_eq!(Kind::Viewer.policy().min_size, Some(VIEWER_MIN));
        assert!(Kind::Viewer.policy().grows_only && Kind::Viewer.policy().fits_content);
        assert_eq!(
            (Kind::Tool.default_width(), Kind::Tool.default_height()),
            (TOOL_DEFAULT.width, Some(TOOL_DEFAULT.height))
        );
        assert!(!Kind::Tool.policy().fits_content);
        assert!(Kind::Decision.policy().fits_content && !Kind::Decision.policy().grows_only);
        assert_eq!(
            (
                Kind::Decision.default_width(),
                Kind::Form.default_width(),
                Kind::Viewer.default_width()
            ),
            (400.0, 560.0, 720.0)
        );
        let remembers: Vec<_> = Kind::ALL
            .iter()
            .map(|k| k.policy().remembers.as_str())
            .collect();
        assert_eq!(
            remembers,
            ["offset", "widthAndOffset", "sizeAndOffset", "sizeAndOffset"]
        );
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
        let c = Arc::new(sample());
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
        let c = sample();
        assert_eq!(c.problems(&app::TABLE, &[]), Vec::<String>::new());
        let p = c.problems(&app::TABLE, &["app_status"]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(
            p[0].contains("about") && p[0].contains("app_status"),
            "{p:?}"
        );
    }
}
