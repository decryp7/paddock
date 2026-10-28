//! The TypeScript generated from the Rust dialog contracts (ADR 0026 §3): the kind table every app shares
//! (`ui/projects/paddock-ui/shell/generated/dialog-kinds.ts`) and one `dialog-contracts.ts` per owner of a
//! [`DialogTable`]. Each owner checks its file in a test (`paddock_commands::testing::sync_generated`: a stale file
//! fails, `PADDOCK_WRITE_TS=1` rewrites it), so TypeScript can no longer disagree with Rust on kind, modality, size or
//! commands.

use crate::contract::{CAP_FRACTION, DIALOG_MIN, Kind, Modality, Remembers};
use crate::table::{DialogTable, Width};
use std::fmt::Write;

/// A number as TypeScript writes it (`400`, not `400.0`).
fn num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// A string literal in single quotes. Ids and command names are restricted to `[a-z0-9_.*|:-]`, so no escaping is
/// needed beyond refusing a quote or a backslash outright.
fn quoted(s: &str) -> String {
    debug_assert!(!s.contains(['\'', '\\']));
    format!("'{s}'")
}

fn list(items: impl IntoIterator<Item = String>) -> String {
    let items: Vec<String> = items.into_iter().collect();
    format!("[{}]", items.join(", "))
}

/// The kind table as TypeScript: widths, floors, defaults, `fits`, `growsOnly` and what each kind remembers.
pub fn kinds_module(regenerate: &str) -> String {
    let mut out = format!(
        "// Generated from paddock-dialogs (the kind table of ADR 0026 §1). Do not edit: {regenerate}.\n\n\
/** The three width steps, logical px (ADR 0026 §1, D2). */\n\
export const DIALOG_WIDTH_STEPS = {{ narrow: {}, medium: {}, wide: {} }} as const;\n\
export type DialogWidthStep = keyof typeof DIALOG_WIDTH_STEPS;\n\n\
/** The least a dialog window can be, logical px. */\n\
export const DIALOG_MIN_SIZE = {{ width: {}, height: {} }} as const;\n\n\
/** The largest share of the work area a dialog's preferred size takes; never below its minimum content. */\n\
export const DIALOG_CAP_FRACTION = {CAP_FRACTION};\n\n\
/**\n * What each kind fixes: modalities, the default width and preferred height (`null`: measured), its floor, whether it\n\
\x20* is fitted to its content (`fits`, the one \"is fitted\" table of ADR 0016 decision 7), whether a refit only grows, and\n\
\x20* what it remembers between openings.\n */\n\
export const DIALOG_KIND_TABLE = {{\n",
        num(Width::Narrow.px()),
        num(Width::Medium.px()),
        num(Width::Wide.px()),
        num(DIALOG_MIN.width),
        num(DIALOG_MIN.height),
    );
    for kind in Kind::ALL {
        let p = kind.policy();
        let modalities = [Modality::Modal, Modality::Modeless]
            .into_iter()
            .filter(|m| kind.allows(*m))
            .map(|m| quoted(m.as_str()));
        let min = p.min_size.map_or("null".to_string(), |s| {
            format!("{{ width: {}, height: {} }}", num(s.width), num(s.height))
        });
        let _ = writeln!(
            out,
            "  {}: {{ modalities: {}, width: {}, height: {}, minSize: {min}, fits: {}, growsOnly: {}, remembers: {}, rememberHeight: {} }},",
            kind.as_str(),
            list(modalities),
            num(kind.default_width()),
            kind.default_height().map_or("null".to_string(), num),
            p.fits_content,
            p.grows_only,
            quoted(p.remembers.as_str()),
            p.remembers == Remembers::SizeAndOffset,
        );
    }
    out.push_str("} as const;\n");
    out
}

/// The contracts of one owner's `table` as TypeScript: `DIALOG_CONTRACTS` (`as const`), `DialogId`, `DialogCommandOf` and
/// the owner's base commands. `owner` names the table in the header (`paddock-channels`, `shell-demo`).
pub fn contracts_module(owner: &str, table: &DialogTable, regenerate: &str) -> String {
    let mut out = format!(
        "// Generated from the dialog table of {owner} (paddock-dialogs `dialogs!`, ADR 0026 §3). Do not edit: {regenerate}.\n\n\
/** Every dialog's contract as Rust declares it: `defineDialog(DIALOG_CONTRACTS['<id>'], {{ title, result, load }})`. */\n\
export const DIALOG_CONTRACTS = {{\n"
    );
    for e in table.entries() {
        let mut commands: Vec<String> = e.commands.iter().map(|c| c.wire()).collect();
        commands.sort();
        commands.dedup();
        let _ = writeln!(
            out,
            "  {id}: {{\n    id: {id},\n    kind: {},\n    modality: {},\n    width: {},\n    widthStep: {},\n    height: {},\n    instanced: {},\n    requiredChoice: {},\n    commands: {},\n  }},",
            quoted(e.shape.kind().as_str()),
            quoted(e.shape.modality().as_str()),
            num(e.width_px()),
            e.width
                .unwrap_or(e.shape.kind().policy().default_width)
                .step()
                .map_or("null".to_string(), quoted),
            e.height_px().map_or("null".to_string(), num),
            e.instanced,
            e.required_choice,
            list(commands.iter().map(|c| quoted(c))),
            id = quoted(e.id),
        );
    }
    let mut base: Vec<String> = table.base().iter().map(|c| c.wire()).collect();
    base.sort();
    base.dedup();
    let _ = write!(
        out,
        "}} as const;\n\n\
export type DialogId = keyof typeof DIALOG_CONTRACTS;\n\
export type DialogCommandOf<Id extends DialogId> = (typeof DIALOG_CONTRACTS)[Id]['commands'][number];\n\n\
/** The commands every dialog window of this owner's app may call besides the shell's base set. */\n\
export const DIALOG_BASE = {} as const;\n",
        list(base.iter().map(|c| quoted(c)))
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::dialogs! {
        static T, base &[paddock_commands::DialogCallable::__command("window_ready")];
        "a.ask" => Decision { instanced: true },
        "a.tool" => Tool {
            width: Some(Px(880)),
            height: Some(620),
            commands: &[paddock_commands::DialogCallable::__command("b_x"), paddock_commands::DialogCallable::__command("a_x"), paddock_commands::DialogCallable::__command("a_x")],
        },
    }

    #[test]
    fn numbers_are_written_as_typescript_writes_them() {
        assert_eq!(
            (num(400.0), num(0.75), num(-2.0)),
            ("400".into(), "0.75".into(), "-2".into())
        );
    }

    #[test]
    fn the_contracts_module_lists_every_entry_sorted_commands_and_the_base() {
        let ts = contracts_module("app", &T, "regen");
        assert!(
            ts.starts_with("// Generated from the dialog table of app"),
            "{ts}"
        );
        assert!(ts.contains("  'a.ask': {\n    id: 'a.ask',\n    kind: 'decision',\n    modality: 'modal',\n    width: 400,\n    widthStep: 'narrow',\n    height: null,\n    instanced: true,\n    requiredChoice: false,\n    commands: [],\n  },"), "{ts}");
        assert!(
            ts.contains("width: 880,\n    widthStep: null,\n    height: 620,"),
            "{ts}"
        );
        assert!(ts.contains("commands: ['a_x', 'b_x'],"), "{ts}");
        assert!(
            ts.contains("export const DIALOG_BASE = ['window_ready'] as const;"),
            "{ts}"
        );
    }

    #[test]
    fn the_kinds_module_is_the_kind_table() {
        let ts = kinds_module("regen");
        assert!(ts.contains("narrow: 400, medium: 560, wide: 720"), "{ts}");
        assert!(ts.contains("  decision: { modalities: ['modal'], width: 400, height: null, minSize: null, fits: true, growsOnly: false, remembers: 'offset', rememberHeight: false },"), "{ts}");
        assert!(
            ts.contains("  form: { modalities: ['modal', 'modeless'], width: 560,"),
            "{ts}"
        );
        assert!(ts.contains("  viewer: { modalities: ['modeless'], width: 720, height: 560, minSize: { width: 680, height: 560 }, fits: true, growsOnly: true,"), "{ts}");
        assert!(ts.contains("  tool: { modalities: ['modeless'], width: 960, height: 680, minSize: null, fits: false,"), "{ts}");
        assert!(ts.contains("DIALOG_CAP_FRACTION = 0.75;"), "{ts}");
    }
}
