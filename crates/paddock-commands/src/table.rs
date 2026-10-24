//! The command table: one [`Spec`] per command (its wire name, [`Kind`], the [`Windows`] that may call it and its largest
//! request), validated once when the table is built. A table is built by [`command_table!`](crate::command_table) in a
//! `static`, so a malformed table (a name listed twice, a command no window may call, an absurd limit) fails to compile.

use crate::labels::WindowClass;
use serde::Serialize;

/// One kibibyte, for request limits.
pub const KIB: usize = 1024;
/// One mebibyte, for request limits.
pub const MIB: usize = 1024 * KIB;
/// The largest request a command accepts unless its entry says otherwise.
pub const DEFAULT_LIMIT: usize = 256 * KIB;
/// The smallest limit an entry may give.
pub const MIN_LIMIT: usize = KIB;
/// The largest limit an entry may give (larger data goes through a file or a stream, not one request).
pub const MAX_LIMIT: usize = 64 * MIB;
/// The longest command name.
pub const MAX_NAME_LEN: usize = 64;

/// How a command is logged and what it does.
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "CommandKind"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Read-only: logged at Debug (or Info, as the app decides).
    Query,
    /// Changes state or acts for the user: always logged at Info.
    Mutation,
    /// Polling and bookkeeping (autosave, progress): at Trace, or not at all.
    Quiet,
}

impl Kind {
    /// The kind's name on the wire and in generated TypeScript.
    pub const fn as_str(self) -> &'static str {
        match self {
            Kind::Query => "query",
            Kind::Mutation => "mutation",
            Kind::Quiet => "quiet",
        }
    }
}

/// The window classes a command may be called from: a set of [`WindowClass`]es.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Windows(u8);

impl Windows {
    /// No window: a table refuses an entry with this set.
    pub const NONE: Windows = Windows(0);
    /// `main` and the app's other top-level windows (`w-<n>`).
    pub const MAIN: Windows = Windows(1);
    /// Panels popped out of a window (`p-<host>-<n>`).
    pub const POPOUT: Windows = Windows(2);
    /// Dialogs of a window (`d-<host>-<n>`). The class only opens a command to dialogs; which dialog may call it is the
    /// dialog's own contract ([`DialogGrant`](crate::DialogGrant), ADR 0009), so a command without this class can never
    /// be reached from any dialog.
    pub const DIALOG: Windows = Windows(4);
    /// Tray popovers (`tray`, `tray-<item>`). Never part of [`Windows::WINDOW`]: a command opens to a popover only by
    /// naming this class (`[WINDOW | TRAY]`).
    pub const TRAY: Windows = Windows(8);
    /// A window, its pop-outs and its dialogs (not the tray popovers).
    pub const WINDOW: Windows = Windows(1 | 2 | 4);

    /// This set plus `other`.
    pub const fn with(self, other: Windows) -> Windows {
        Windows(self.0 | other.0)
    }

    /// Whether the set is empty.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether windows of `class` are in the set.
    pub const fn contains(self, class: WindowClass) -> bool {
        self.0 & Self::of(class).0 != 0
    }

    /// The set of one class.
    pub const fn of(class: WindowClass) -> Windows {
        match class {
            WindowClass::Main => Windows::MAIN,
            WindowClass::Popout => Windows::POPOUT,
            WindowClass::Dialog => Windows::DIALOG,
            WindowClass::Tray => Windows::TRAY,
        }
    }

    /// The classes in the set, in the order main, popout, dialog, tray.
    pub fn classes(self) -> Vec<WindowClass> {
        WindowClass::ALL
            .into_iter()
            .filter(|c| self.contains(*c))
            .collect()
    }
}

/// `limit` as written in a table entry (none: [`DEFAULT_LIMIT`]). Used by [`command_table!`](crate::command_table).
pub const fn limit(given: &[usize]) -> usize {
    match given {
        [l] => *l,
        _ => DEFAULT_LIMIT,
    }
}

/// One command of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    path: &'static str,
    name: &'static str,
    kind: Kind,
    windows: Windows,
    limit: usize,
}

impl Spec {
    /// An entry for the handler at `path` (as written in the table, `workspace :: open_paths`); its wire name is the last
    /// path segment.
    pub const fn new(path: &'static str, kind: Kind, windows: Windows, limit: usize) -> Spec {
        Spec {
            path,
            name: last_segment(path),
            kind,
            windows,
            limit,
        }
    }

    /// The handler's path as written in the table.
    pub const fn path(&self) -> &'static str {
        self.path
    }

    /// The command's name on the wire.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    pub const fn kind(&self) -> Kind {
        self.kind
    }

    pub const fn windows(&self) -> Windows {
        self.windows
    }

    /// The largest request body in bytes.
    pub const fn limit(&self) -> usize {
        self.limit
    }

    /// The entry as data (for generated TypeScript and diagnostics).
    pub fn info(&self) -> CommandInfo {
        CommandInfo {
            kind: self.kind,
            windows: self.windows.classes(),
            limit: self.limit,
        }
    }
}

/// What the UI knows of a command: its kind, the window classes that may call it and its largest request in bytes.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandInfo {
    pub kind: Kind,
    pub windows: Vec<WindowClass>,
    pub limit: usize,
}

/// The last `::` segment of `path`, without the spaces `stringify!` puts around `::`.
const fn last_segment(path: &'static str) -> &'static str {
    let bytes = path.as_bytes();
    let mut start = bytes.len();
    while start > 0 && bytes[start - 1] != b':' {
        start -= 1;
    }
    while start < bytes.len() && bytes[start] == b' ' {
        start += 1;
    }
    // `start` follows an ASCII byte (or is 0), so it is a char boundary.
    path.split_at(start).1
}

/// What is wrong with a table: the entry at `index` (and, for a duplicate, the earlier entry `first`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableError {
    /// The name is empty, longer than [`MAX_NAME_LEN`] or not `lower_snake_case` ASCII starting with a letter.
    BadName { index: usize },
    /// No window class may call the command.
    NoWindows { index: usize },
    /// The limit is outside [`MIN_LIMIT`]..=[`MAX_LIMIT`].
    BadLimit { index: usize },
    /// The name is listed twice.
    Duplicate { first: usize, index: usize },
}

const fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > MAX_NAME_LEN || !b[0].is_ascii_lowercase() {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        if !(b[i].is_ascii_lowercase() || b[i].is_ascii_digit() || b[i] == b'_') {
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

/// The number of commands in `parts` together (used by [`command_table!`](crate::command_table) to compose tables).
#[doc(hidden)]
pub const fn total(parts: &[&Table]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < parts.len() {
        n += parts[i].specs.len();
        i += 1;
    }
    n
}

/// Every entry of `parts`, in order (used by [`command_table!`](crate::command_table) to compose tables). `N` must be
/// [`total`]`(parts)`; any other length fails the build that evaluates it.
#[doc(hidden)]
pub const fn concat<const N: usize>(parts: &[&Table]) -> [Spec; N] {
    let mut out = [Spec::new("", Kind::Quiet, Windows::NONE, 0); N];
    let mut k = 0;
    let mut i = 0;
    while i < parts.len() {
        let mut j = 0;
        while j < parts[i].specs.len() {
            assert!(k < N, "command table: the composed length is wrong");
            out[k] = parts[i].specs[j];
            k += 1;
            j += 1;
        }
        i += 1;
    }
    assert!(k == N, "command table: the composed length is wrong");
    out
}

/// Every command an app (or a shell plugin) serves, in one validated list.
#[derive(Debug, Clone, Copy)]
pub struct Table {
    specs: &'static [Spec],
}

impl Table {
    /// A table with no commands.
    pub const EMPTY: Table = Table { specs: &[] };

    /// The first defect of `specs`, if any.
    pub const fn validate(specs: &[Spec]) -> Result<(), TableError> {
        let mut index = 0;
        while index < specs.len() {
            let s = &specs[index];
            if !valid_name(s.name) {
                return Err(TableError::BadName { index });
            }
            if s.windows.is_empty() {
                return Err(TableError::NoWindows { index });
            }
            if s.limit < MIN_LIMIT || s.limit > MAX_LIMIT {
                return Err(TableError::BadLimit { index });
            }
            let mut first = 0;
            while first < index {
                if same(specs[first].name, s.name) {
                    return Err(TableError::Duplicate { first, index });
                }
                first += 1;
            }
            index += 1;
        }
        Ok(())
    }

    /// The table of `specs`. Panics on a defect ([`Table::validate`]); in a `static` (as
    /// [`command_table!`](crate::command_table) builds it) that is a compile error.
    pub const fn new(specs: &'static [Spec]) -> Table {
        match Self::validate(specs) {
            Ok(()) => Table { specs },
            Err(TableError::BadName { .. }) => panic!(
                "command table: a command name is not lower_snake_case ASCII of 1 to 64 characters"
            ),
            Err(TableError::NoWindows { .. }) => {
                panic!("command table: a command is callable from no window class")
            }
            Err(TableError::BadLimit { .. }) => {
                panic!("command table: a request limit is outside 1 KiB to 64 MiB")
            }
            Err(TableError::Duplicate { .. }) => panic!("command table: a command is listed twice"),
        }
    }

    /// Every entry, in table order.
    pub const fn specs(&self) -> &'static [Spec] {
        self.specs
    }

    /// The entry of the command `name`.
    pub fn spec(&self, name: &str) -> Option<&'static Spec> {
        self.specs.iter().find(|s| s.name == name)
    }

    /// Whether the command `name` is in the table and open to dialogs (a dialog contract may name it).
    pub fn opens_to_dialogs(&self, name: &str) -> bool {
        self.spec(name)
            .is_some_and(|s| s.windows.contains(WindowClass::Dialog))
    }

    /// How `command` is logged. Plugin commands (`plugin:<name>|<cmd>`, Tauri's own plugins as the app's activity log
    /// sees them): the log plugin's forwarding and event subscriptions are quiet, native dialogs are user actions, the
    /// rest are queries. Unknown commands are mutations, so nothing new goes unlogged.
    pub fn kind_of(&self, command: &str) -> Kind {
        if let Some(rest) = command.strip_prefix("plugin:") {
            return match rest.split('|').next().unwrap_or_default() {
                "log" | "event" => Kind::Quiet,
                "dialog" => Kind::Mutation,
                _ => Kind::Query,
            };
        }
        self.spec(command).map_or(Kind::Mutation, |s| s.kind)
    }

    /// What is wrong with a set of dialog contracts against this table: every `base` command (callable from every
    /// dialog) and every command a dialog declares must exist and be open to dialogs, a `prefix*` must match at least one
    /// command, and none may be one of `host_only` (commands that act for the window as a whole: a dialog asks its host
    /// instead). Empty when they agree. One check over every instance (rule 40); each app runs it in a test.
    pub fn contract_problems<'a>(
        &self,
        base: &[String],
        dialogs: impl IntoIterator<Item = (&'a str, &'a [String])>,
        host_only: &[&str],
    ) -> Vec<String> {
        let mut problems = Vec::new();
        let mut check = |who: &str, name: &str| {
            if let Some(prefix) = name.strip_suffix('*') {
                let matched: Vec<&Spec> = self
                    .specs
                    .iter()
                    .filter(|s| s.name.starts_with(prefix))
                    .collect();
                if matched.is_empty() {
                    problems.push(format!("{who}: {name} matches no command"));
                }
                for s in matched {
                    if !s.windows.contains(WindowClass::Dialog) {
                        problems.push(format!(
                            "{who}: {name} matches {}, which the table does not open to dialogs",
                            s.name
                        ));
                    }
                    if host_only.contains(&s.name) {
                        problems.push(format!(
                            "{who}: {name} matches {}, which stays with the host window",
                            s.name
                        ));
                    }
                }
                return;
            }
            match self.spec(name) {
                None => problems.push(format!(
                    "{who} declares {name}, which is not in the command table"
                )),
                Some(s) if !s.windows.contains(WindowClass::Dialog) => problems.push(format!(
                    "{who} declares {name}, which the table does not open to dialogs"
                )),
                Some(_) => {}
            }
            if host_only.contains(&name) {
                problems.push(format!(
                    "{who} declares {name}: that stays with the host window"
                ));
            }
        };
        for name in base {
            check("base", name);
        }
        for (id, commands) in dialogs {
            for name in commands {
                check(id, name);
            }
        }
        problems
    }
}
