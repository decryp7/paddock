//! Generated TypeScript for a command table: a `const` object of every command (kind, window classes, limit) and the
//! union of its names, so a typo in a command name is a compile error in the UI. The file is written by a test of the
//! crate that owns the table and compared on every run ([`crate::testing::sync_generated`]): a stale file fails CI.
//!
//! The format (stable; the S2 contributions core and every app import it):
//!
//! ```ts
//! // Generated from the command table by paddock-commands. Do not edit: <how to regenerate>.
//!
//! /** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
//! export const APP_COMMANDS = {
//!   open_paths: { kind: 'mutation', windows: ['main', 'popout', 'dialog'], limit: 262144 },
//! } as const;
//!
//! /** A command name of the table. */
//! export type AppCommand = keyof typeof APP_COMMANDS;
//! ```
//!
//! Entries are sorted by name; an empty table gives `{} as const` (its name type is `never`).

use crate::table::Table;
use std::fmt;

/// A name given for the generated `const` or type that is not a TypeScript identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidIdentifier(pub String);

impl fmt::Display for InvalidIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} is not a TypeScript identifier (ASCII letters, digits, `_`, `$`; not starting with a digit)",
            self.0
        )
    }
}

impl std::error::Error for InvalidIdentifier {}

fn identifier(name: &str) -> Result<&str, InvalidIdentifier> {
    let ok = name.bytes().next().is_some_and(|b| !b.is_ascii_digit())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$');
    if ok {
        Ok(name)
    } else {
        Err(InvalidIdentifier(name.to_string()))
    }
}

/// The TypeScript module of `table`: `export const <constant>` with every command and `export type <type_name>` of their
/// names. `regenerate` tells a reader how to rewrite the file (the command line of the test that writes it).
pub fn names_module(
    table: &Table,
    constant: &str,
    type_name: &str,
    regenerate: &str,
) -> Result<String, InvalidIdentifier> {
    let constant = identifier(constant)?;
    let type_name = identifier(type_name)?;
    let mut specs: Vec<_> = table.specs().iter().collect();
    specs.sort_by_key(|s| s.name());
    let mut out = format!(
        "// Generated from the command table by paddock-commands. Do not edit: {regenerate}.\n\n\
         /** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */\n"
    );
    if specs.is_empty() {
        out.push_str(&format!("export const {constant} = {{}} as const;\n"));
    } else {
        out.push_str(&format!("export const {constant} = {{\n"));
        for s in specs {
            let windows: Vec<String> = s
                .windows()
                .classes()
                .iter()
                .map(|c| format!("'{}'", c.as_str()))
                .collect();
            out.push_str(&format!(
                "  {}: {{ kind: '{}', windows: [{}], limit: {} }},\n",
                s.name(),
                s.kind().as_str(),
                windows.join(", "),
                s.limit()
            ));
        }
        out.push_str("} as const;\n");
    }
    out.push_str(&format!(
        "\n/** A command name of the table. */\nexport type {type_name} = keyof typeof {constant};\n"
    ));
    Ok(out)
}
