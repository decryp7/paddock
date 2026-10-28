//! The dialog contracts as static Rust tables (ADR 0026 §3; ADR 0022 decision 5): the owner of a dialog declares it with
//! [`dialogs!`](crate::dialogs) beside its `command_table!`, and the app registers the tables it uses with
//! [`Contracts::from_tables`](crate::Contracts::from_tables). Rust is the one source: the TypeScript side reads a file
//! generated from these tables ([`crate::ts`]) and adds only what TypeScript alone knows (title, result type, loader).
//!
//! What the compiler checks:
//!
//! - every command a contract names is a `dialog_callable::<name>` marker of a command table (a misspelt command, a
//!   removed one or one closed to dialogs is an unresolved name);
//! - a `prefix*` entry ([`prefix`](paddock_commands::prefix)) matches at least one dialog-callable command;
//! - kind and modality are one typed value ([`Shape`]): a modeless decision or a modal viewer cannot be written;
//! - ids follow the scheme and are unique, a required choice is modal, only a tool declares a number for its width, only
//!   a viewer or a tool a preferred height ([`DialogTable::new`] runs in the `static`'s initializer: a defect is a
//!   compile error naming the rule).

use crate::contract::{Kind, MAX_COMMANDS, MAX_DIALOGS, MAX_NAME_LEN, MAX_SIDE, Modality};
use paddock_commands::{DialogCallable, DialogCallableKind};

/// Kind and modality as one value: only the pairs ADR 0009 §1 allows can be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shape {
    /// Modal; Cancel and one affirmative.
    Decision,
    /// Modal or modeless.
    Form(Modality),
    /// Modeless; Close.
    Viewer,
    /// Modeless, user-sized; Close.
    Tool,
}

impl Shape {
    /// The kind.
    pub const fn kind(self) -> Kind {
        match self {
            Shape::Decision => Kind::Decision,
            Shape::Form(_) => Kind::Form,
            Shape::Viewer => Kind::Viewer,
            Shape::Tool => Kind::Tool,
        }
    }

    /// The modality: a decision is modal, a viewer and a tool modeless, a form as declared.
    pub const fn modality(self) -> Modality {
        match self {
            Shape::Decision => Modality::Modal,
            Shape::Form(m) => m,
            Shape::Viewer | Shape::Tool => Modality::Modeless,
        }
    }
}

/// A dialog's width (ADR 0026 §1, user decision D2): three named steps, defaulted by kind; only a tool names a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Width {
    /// 400 logical px: decisions.
    Narrow,
    /// 560 logical px: forms.
    Medium,
    /// 720 logical px: viewers and wide forms.
    Wide,
    /// A tool's own width, logical px.
    Px(u16),
}

impl Width {
    /// The width in logical px.
    pub const fn px(self) -> f64 {
        match self {
            Width::Narrow => 400.0,
            Width::Medium => 560.0,
            Width::Wide => 720.0,
            Width::Px(px) => px as f64,
        }
    }

    /// The step's name (`None` for a tool's number).
    pub const fn step(self) -> Option<&'static str> {
        match self {
            Width::Narrow => Some("narrow"),
            Width::Medium => Some("medium"),
            Width::Wide => Some("wide"),
            Width::Px(_) => None,
        }
    }

    /// The three named steps, narrow to wide.
    pub const STEPS: [Width; 3] = [Width::Narrow, Width::Medium, Width::Wide];
}

/// One dialog's contract as its owner declares it. Written through [`dialogs!`](crate::dialogs) with struct-update
/// syntax over [`Entry::new`]: `"pd.mcp.key" => Form(Modal) { commands: &[mcp::mcp_issue_key] }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Stable, namespaced id (`pd.channels.secret`): the window's route, its remembered bounds' key, its test id.
    pub id: &'static str,
    pub shape: Shape,
    /// `None`: the kind's default ([`Kind::default_width`]).
    pub width: Option<Width>,
    /// The preferred height of a viewer or a tool, logical px (`None`: the kind's default). A decision or form is
    /// measured, never declared.
    pub height: Option<u16>,
    /// One window per distinct input (the request names an instance key).
    pub instanced: bool,
    /// A modal without a way out: no dismiss action; Escape, Cmd/Ctrl+W and an OS close leave it open.
    pub required_choice: bool,
    /// The commands its window may call beyond the base ones: `dialog_callable` markers, [`prefix`](paddock_commands::prefix)
    /// and [`plugin`](paddock_commands::plugin) entries.
    pub commands: &'static [DialogCallable],
}

impl Entry {
    /// A dialog `id` of `shape` with every other field at its default (no commands).
    pub const fn new(id: &'static str, shape: Shape) -> Entry {
        Entry {
            id,
            shape,
            width: None,
            height: None,
            instanced: false,
            required_choice: false,
            commands: &[],
        }
    }

    /// The width in logical px: declared, else the kind's default.
    pub const fn width_px(&self) -> f64 {
        match self.width {
            Some(w) => w.px(),
            None => self.shape.kind().default_width(),
        }
    }

    /// The preferred height in logical px: declared, else the kind's default (`None`: measured).
    pub const fn height_px(&self) -> Option<f64> {
        match self.height {
            Some(h) => Some(h as f64),
            None => self.shape.kind().default_height(),
        }
    }
}

/// The dialogs of one owner (a shell crate or an app), validated when built: in a `static` a defect fails to compile.
#[derive(Debug, Clone, Copy)]
pub struct DialogTable {
    entries: &'static [Entry],
    base: &'static [DialogCallable],
}

/// What is wrong with a dialog table: the entry at `index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableError {
    /// The id is empty, longer than 64 bytes or not lower-case letters, digits, `-` and `.`.
    BadId { index: usize },
    /// The id is listed twice.
    Duplicate { first: usize, index: usize },
    /// A required choice that is not modal.
    RequiredChoiceNotModal { index: usize },
    /// A width in px on a dialog that is not a tool, or a width outside 1..=4096.
    BadWidth { index: usize },
    /// A preferred height on a decision or form, or a height outside 1..=4096.
    BadHeight { index: usize },
    /// More commands than [`MAX_COMMANDS`].
    TooManyCommands { index: usize },
    /// More dialogs than [`MAX_DIALOGS`], or more base commands than [`MAX_COMMANDS`].
    TooMany,
}

/// A dialog id: lower-case letters, digits, `-` and `.`, 1 to 64 bytes (`crate::contract::valid_dialog_id` as a `const fn`).
pub const fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    if b.is_empty() || b.len() > MAX_NAME_LEN {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'.') {
            return false;
        }
        i += 1;
    }
    true
}

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

impl DialogTable {
    /// A table with no dialogs.
    pub const EMPTY: DialogTable = DialogTable {
        entries: &[],
        base: &[],
    };

    /// The first defect of `entries` and `base`, if any.
    pub const fn validate(entries: &[Entry], base: &[DialogCallable]) -> Result<(), TableError> {
        if entries.len() > MAX_DIALOGS || base.len() > MAX_COMMANDS {
            return Err(TableError::TooMany);
        }
        let mut index = 0;
        while index < entries.len() {
            let e = &entries[index];
            if !valid_id(e.id) {
                return Err(TableError::BadId { index });
            }
            let mut first = 0;
            while first < index {
                if same(entries[first].id, e.id) {
                    return Err(TableError::Duplicate { first, index });
                }
                first += 1;
            }
            if e.required_choice && !matches!(e.shape.modality(), Modality::Modal) {
                return Err(TableError::RequiredChoiceNotModal { index });
            }
            if let Some(Width::Px(px)) = e.width
                && (!matches!(e.shape, Shape::Tool) || px == 0 || px as f64 > MAX_SIDE)
            {
                return Err(TableError::BadWidth { index });
            }
            if let Some(h) = e.height
                && (!matches!(e.shape, Shape::Viewer | Shape::Tool)
                    || h == 0
                    || h as f64 > MAX_SIDE)
            {
                return Err(TableError::BadHeight { index });
            }
            if e.commands.len() > MAX_COMMANDS {
                return Err(TableError::TooManyCommands { index });
            }
            index += 1;
        }
        Ok(())
    }

    /// The table of `entries` with the app's `base` commands (every dialog window may call them). Panics on a defect
    /// ([`DialogTable::validate`]); in a `static` (as [`dialogs!`](crate::dialogs) builds it) that is a compile error.
    pub const fn new(entries: &'static [Entry], base: &'static [DialogCallable]) -> DialogTable {
        match Self::validate(entries, base) {
            Ok(()) => DialogTable { entries, base },
            Err(TableError::BadId { .. }) => {
                panic!(
                    "dialog table: an id is not lower-case letters, digits, '-' and '.' of 1 to 64 bytes"
                )
            }
            Err(TableError::Duplicate { .. }) => {
                panic!("dialog table: a dialog id is listed twice")
            }
            Err(TableError::RequiredChoiceNotModal { .. }) => {
                panic!("dialog table: only a modal can be a required choice")
            }
            Err(TableError::BadWidth { .. }) => {
                panic!(
                    "dialog table: only a tool declares a width in px (others use Narrow, Medium or Wide), within 1 to 4096"
                )
            }
            Err(TableError::BadHeight { .. }) => {
                panic!(
                    "dialog table: only a viewer or a tool declares a preferred height, within 1 to 4096"
                )
            }
            Err(TableError::TooManyCommands { .. }) => {
                panic!("dialog table: a dialog declares more than 512 commands")
            }
            Err(TableError::TooMany) => {
                panic!("dialog table: more than 1024 dialogs or 512 base commands")
            }
        }
    }

    /// Every entry, in table order.
    pub const fn entries(&self) -> &'static [Entry] {
        self.entries
    }

    /// The base commands of the table's owner.
    pub const fn base(&self) -> &'static [DialogCallable] {
        self.base
    }

    /// The entry of dialog `id`.
    pub fn get(&self, id: &str) -> Option<&'static Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Every plugin command the table names (`(plugin, command)`), for the owner's test against the plugin's permissions.
    pub fn plugin_commands(&self) -> Vec<(&'static str, &'static str)> {
        self.entries
            .iter()
            .flat_map(|e| e.commands.iter())
            .chain(self.base.iter())
            .filter_map(|c| match c.kind() {
                DialogCallableKind::Plugin { plugin, command } => Some((plugin, command)),
                _ => None,
            })
            .collect()
    }
}

/// `base` as written in a [`dialogs!`](crate::dialogs) head (none: no base commands).
#[doc(hidden)]
pub const fn base_or_empty(given: &[&'static [DialogCallable]]) -> &'static [DialogCallable] {
    match given {
        [b] => b,
        _ => &[],
    }
}

/// The names a [`dialogs!`](crate::dialogs) entry is written with: the width steps, the modalities and the shapes.
pub mod prelude {
    pub use super::Shape::{Decision, Form, Tool, Viewer};
    pub use super::Width::{Medium, Narrow, Px, Wide};
    pub use crate::contract::Modality::{Modal, Modeless};
    pub use paddock_commands::{plugin, prefix};
}

/// Declares the dialog contracts of one owner as a validated `static` [`DialogTable`] (ADR 0026 §3).
///
/// Each entry is `"<id>" => <Shape> { <field>: <value>, ... }`, the fields those of [`Entry`] (`width`, `height`,
/// `instanced`, `required_choice`, `commands`), the braces optional. In the values, [`prelude`] is in scope: `Narrow`,
/// `Medium`, `Wide`, `Px(n)`, `prefix`, `plugin`. Commands are `dialog_callable` markers of a command table (bring the
/// owner's module in with a `use ... as ...;` for short names). An optional `base &[...]` in the head lists the app's
/// commands every dialog window may call.
///
/// ```
/// mod app {
///     paddock_commands::command_table! {
///         pub static COMMANDS;
///         Query [MAIN | DIALOG] => app::app_status,
///         Query [WINDOW] => app::settings_get,
///         Mutation [MAIN | DIALOG] => app::settings_set,
///         Mutation [MAIN] => app::app_quit,
///     }
/// }
/// use app::dialog_callable as app_cmd;
///
/// paddock_dialogs::dialogs! {
///     /// The app's own dialogs.
///     pub static DIALOGS, base &[app_cmd::app_status];
///     "app.quit" => Decision { required_choice: true },
///     "app.rename" => Form(Modal) { width: Some(Narrow) },
///     "app.settings" => Tool { width: Some(Px(880)), height: Some(620), commands: &[prefix(app_cmd::ALL, "settings_")] },
///     "app.about" => Viewer,
/// }
/// let c = paddock_dialogs::Contracts::from_tables(&[&DIALOGS]).unwrap();
/// assert!(c.allows("app.settings", "settings_set") && c.allows("app.rename", "app_status"));
/// assert!(!c.allows("app.about", "settings_get"));
/// assert_eq!(c.get("app.rename").map(|r| r.width), Some(400.0));
/// ```
///
/// A command closed to dialogs (or misspelt, or removed) is an unresolved name:
///
/// ```compile_fail,E0425
/// mod app { paddock_commands::command_table! { pub static COMMANDS; Mutation [MAIN] => app::app_quit, } }
/// paddock_dialogs::dialogs! { static D; "a.quit" => Decision { commands: &[app::dialog_callable::app_quit] } }
/// ```
///
/// ```compile_fail,E0425
/// mod app { paddock_commands::command_table! { pub static COMMANDS; Query [WINDOW] => app::app_status, } }
/// paddock_dialogs::dialogs! { static D; "a.x" => Decision { commands: &[app::dialog_callable::app_stauts] } }
/// ```
///
/// A prefix matching no dialog-callable command does not compile:
///
/// ```compile_fail,E0080
/// mod app { paddock_commands::command_table! { pub static COMMANDS; Query [WINDOW] => app::app_status, } }
/// paddock_dialogs::dialogs! { static D; "a.x" => Tool { commands: &[prefix(app::dialog_callable::ALL, "settings_")] } }
/// ```
///
/// Nor does a modal viewer, a modeless decision, a required choice that is not modal, a number for a form's width or a
/// declared height on a decision, or an id listed twice:
///
/// ```compile_fail,E0532
/// paddock_dialogs::dialogs! { static D; "a.v" => Viewer(Modal) }
/// ```
///
/// ```compile_fail,E0532
/// paddock_dialogs::dialogs! { static D; "a.d" => Decision(Modeless) }
/// ```
///
/// ```compile_fail,E0080
/// paddock_dialogs::dialogs! { static D; "a.f" => Form(Modeless) { required_choice: true } }
/// ```
///
/// ```compile_fail,E0080
/// paddock_dialogs::dialogs! { static D; "a.f" => Form(Modal) { width: Some(Px(500)) } }
/// ```
///
/// ```compile_fail,E0080
/// paddock_dialogs::dialogs! { static D; "a.d" => Decision { height: Some(300) } }
/// ```
///
/// ```compile_fail,E0080
/// paddock_dialogs::dialogs! { static D; "a.d" => Decision, "a.d" => Decision }
/// ```
#[macro_export]
macro_rules! dialogs {
    (
        $(#[$table_meta:meta])* $vis:vis static $table:ident $(, base $base:expr)? ;
        $( $(#[$doc:meta])* $id:literal => $shape:ident $( ( $modality:ident ) )? $( { $( $field:ident : $value:expr ),* $(,)? } )? ),* $(,)?
    ) => {
        $(#[$table_meta])*
        $vis static $table: $crate::DialogTable = {
            #[allow(unused_imports)]
            use $crate::table::prelude::*;
            $crate::DialogTable::new(
                &[ $( $crate::Entry {
                    $( $( $field: $value, )* )?
                    ..$crate::Entry::new($id, $crate::Shape::$shape $( ($crate::Modality::$modality) )?)
                } ),* ],
                $crate::table::base_or_empty(&[ $( $base )? ]),
            )
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn entry(id: &'static str, shape: Shape) -> Entry {
        Entry::new(id, shape)
    }

    #[test]
    fn shapes_fix_kind_and_modality() {
        assert_eq!(Shape::Decision.kind(), Kind::Decision);
        assert_eq!(Shape::Decision.modality(), Modality::Modal);
        for m in [Modality::Modal, Modality::Modeless] {
            assert_eq!(Shape::Form(m).kind(), Kind::Form);
            assert_eq!(Shape::Form(m).modality(), m);
        }
        assert_eq!(
            (Shape::Viewer.kind(), Shape::Viewer.modality()),
            (Kind::Viewer, Modality::Modeless)
        );
        assert_eq!(
            (Shape::Tool.kind(), Shape::Tool.modality()),
            (Kind::Tool, Modality::Modeless)
        );
    }

    #[test]
    fn widths_are_three_steps_and_a_tools_number() {
        let px: Vec<f64> = Width::STEPS.iter().map(|w| w.px()).collect();
        assert_eq!(px, [400.0, 560.0, 720.0]);
        let names: Vec<_> = Width::STEPS.iter().map(|w| w.step()).collect();
        assert_eq!(names, [Some("narrow"), Some("medium"), Some("wide")]);
        assert_eq!((Width::Px(960).px(), Width::Px(960).step()), (960.0, None));
    }

    #[test]
    fn an_entry_defaults_its_size_by_kind() {
        assert_eq!(entry("a", Shape::Decision).width_px(), 400.0);
        assert_eq!(entry("a", Shape::Form(Modality::Modal)).width_px(), 560.0);
        assert_eq!(entry("a", Shape::Viewer).width_px(), 720.0);
        assert_eq!(entry("a", Shape::Tool).width_px(), 960.0);
        assert_eq!(entry("a", Shape::Decision).height_px(), None);
        assert_eq!(entry("a", Shape::Viewer).height_px(), Some(560.0));
        assert_eq!(entry("a", Shape::Tool).height_px(), Some(680.0));
        let own = Entry {
            width: Some(Width::Px(880)),
            height: Some(620),
            ..entry("a", Shape::Tool)
        };
        assert_eq!((own.width_px(), own.height_px()), (880.0, Some(620.0)));
    }

    #[test]
    fn a_table_that_breaks_a_rule_is_refused_with_the_entry() {
        let modal = Shape::Form(Modality::Modal);
        let cases: Vec<(Vec<Entry>, TableError)> = vec![
            (
                vec![entry("A", Shape::Decision)],
                TableError::BadId { index: 0 },
            ),
            (
                vec![entry("", Shape::Decision)],
                TableError::BadId { index: 0 },
            ),
            (
                vec![entry("a", Shape::Decision), entry("a", Shape::Tool)],
                TableError::Duplicate { first: 0, index: 1 },
            ),
            (
                vec![Entry {
                    required_choice: true,
                    ..entry("a", Shape::Form(Modality::Modeless))
                }],
                TableError::RequiredChoiceNotModal { index: 0 },
            ),
            (
                vec![Entry {
                    width: Some(Width::Px(500)),
                    ..entry("a", modal)
                }],
                TableError::BadWidth { index: 0 },
            ),
            (
                vec![Entry {
                    width: Some(Width::Px(0)),
                    ..entry("a", Shape::Tool)
                }],
                TableError::BadWidth { index: 0 },
            ),
            (
                vec![Entry {
                    width: Some(Width::Px(5000)),
                    ..entry("a", Shape::Tool)
                }],
                TableError::BadWidth { index: 0 },
            ),
            (
                vec![Entry {
                    height: Some(300),
                    ..entry("a", modal)
                }],
                TableError::BadHeight { index: 0 },
            ),
            (
                vec![Entry {
                    height: Some(0),
                    ..entry("a", Shape::Viewer)
                }],
                TableError::BadHeight { index: 0 },
            ),
            (
                vec![Entry {
                    height: Some(5000),
                    ..entry("a", Shape::Tool)
                }],
                TableError::BadHeight { index: 0 },
            ),
        ];
        for (entries, error) in cases {
            assert_eq!(
                DialogTable::validate(&entries, &[]),
                Err(error),
                "{entries:?}"
            );
            let leaked: &'static [Entry] = Box::leak(entries.into_boxed_slice());
            assert!(std::panic::catch_unwind(|| DialogTable::new(leaked, &[])).is_err());
        }
        static MANY: [DialogCallable; MAX_COMMANDS + 1] =
            [DialogCallable::__command("x"); MAX_COMMANDS + 1];
        let busy = [Entry {
            commands: &MANY,
            ..entry("a", Shape::Tool)
        }];
        assert_eq!(
            DialogTable::validate(&busy, &[]),
            Err(TableError::TooManyCommands { index: 0 })
        );
        assert!(
            std::panic::catch_unwind(|| DialogTable::new(Box::leak(Box::new(busy)), &[])).is_err()
        );
        assert_eq!(DialogTable::validate(&[], &MANY), Err(TableError::TooMany));
        assert!(std::panic::catch_unwind(|| DialogTable::new(&[], &MANY)).is_err());
        assert!(
            valid_id("pd.wb.dock.layout-save") && !valid_id(&"a".repeat(65)) && !valid_id("a_b")
        );
    }

    #[test]
    fn a_table_finds_its_entries_and_lists_its_plugin_commands() {
        static ENTRIES: [Entry; 2] = [
            Entry {
                commands: &[
                    DialogCallable::__command("app_status"),
                    paddock_commands::plugin("dialog", "open"),
                ],
                ..Entry::new("a.one", Shape::Tool)
            },
            Entry::new("a.two", Shape::Decision),
        ];
        static T: DialogTable =
            DialogTable::new(&ENTRIES, &[paddock_commands::plugin("log", "log")]);
        assert_eq!(T.entries().len(), 2);
        assert_eq!(T.get("a.two").map(|e| e.shape), Some(Shape::Decision));
        assert!(T.get("a.three").is_none());
        assert_eq!(T.base().len(), 1);
        assert_eq!(T.plugin_commands(), [("dialog", "open"), ("log", "log")]);
        assert_eq!(DialogTable::EMPTY.entries().len(), 0);
        assert_eq!(base_or_empty(&[]).len(), 0);
    }

    crate::dialogs! {
        static WRITTEN, base &[paddock_commands::DialogCallable::__command("window_ready")];
        /// A doc comment on an entry is allowed.
        "t.decision" => Decision { instanced: true },
        "t.form" => Form(Modeless) { width: Some(Wide) },
        "t.tool" => Tool { width: Some(Px(820)), height: Some(640), commands: &[plugin("dialog", "open")] },
        "t.viewer" => Viewer,
    }

    #[test]
    fn the_macro_writes_entries_over_the_defaults() {
        let e = WRITTEN.entries();
        assert_eq!(e.len(), 4);
        assert!(e[0].instanced && e[0].shape == Shape::Decision && e[0].width.is_none());
        assert_eq!(e[1].shape, Shape::Form(Modality::Modeless));
        assert_eq!(e[1].width_px(), 720.0);
        assert_eq!((e[2].width_px(), e[2].height_px()), (820.0, Some(640.0)));
        assert_eq!(e[3], Entry::new("t.viewer", Shape::Viewer));
        assert_eq!(WRITTEN.base(), [DialogCallable::__command("window_ready")]);
    }
}
