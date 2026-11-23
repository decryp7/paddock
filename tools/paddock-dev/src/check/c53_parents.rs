//! C53, parent-pid half: a parent pid is only a number. On Windows it outlives its process, so the orphans of an earlier holder of a
//! pid name its new holder as their parent; a tree walked through it takes strangers for children (CI runs 8674 and 8694, ADR 0033).
//! Clippy bans `sysinfo::Process::parent` (lint/clippy.toml), but it cannot ban a struct field, so this half finds the raw parent-pid
//! fields of the OS process APIs read outside paddock-proc: the Toolhelp entry's `th32ParentProcessID`, the NT process information's
//! `InheritedFromUniqueProcessId`, macOS's `pbi_ppid`, `pbsi_ppid` and `e_ppid`. Put the rows into a paddock_proc table
//! (`Listed`, `table_of_listing`) and ask `parent_of` or `descendants`. A justified read (one that only feeds that table, or that
//! checks the parent's start time itself) carries `// conformance: parent-pid — <reason>` on the line or the line above.

use super::finding::Finding;
use super::inline_waiver::{CommentView, InlineWaiver, Kind};
use super::jsre::{LineIndex, rx};
use super::mask::mask_rust;
use regex::Regex;
use std::sync::LazyLock;

/// paddock-proc is the one implementation: its own Toolhelp and `/proc` readers are the canonical source.
const IMPLEMENTATION: &str = "crates/paddock-proc/";

static VENDORED: LazyLock<Regex> = LazyLock::new(|| rx(r"^apps/[^/]+/vendor/"));
static FIELDS: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(th32ParentProcessID|InheritedFromUniqueProcessId|pbi_ppid|pbsi_ppid|e_ppid)\b"));

/// One raw parent-pid read in a Rust source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentRead {
    /// The 1-based line.
    pub line: usize,
    /// The field it reads.
    pub field: String,
    /// The waiver comment.
    pub waived: InlineWaiver,
}

/// The raw parent-pid field reads in one Rust source (code only: strings and comments are masked).
pub fn parent_reads(text: &str, file: &str) -> Vec<ParentRead> {
    if file.starts_with(IMPLEMENTATION) || VENDORED.is_match(file) || !FIELDS.is_match(text) {
        return Vec::new();
    }
    let masked = mask_rust(text);
    let view = CommentView::of(text, Kind::Rust);
    let lines = LineIndex::new(&masked);
    FIELDS
        .captures_iter(&masked)
        .map(|m| {
            let line = lines.line(m.get(0).map_or(0, |w| w.start()));
            ParentRead {
                line,
                field: m[1].to_string(),
                waived: view.waiver(line, "parent-pid"),
            }
        })
        .collect()
}

/// The findings of the parent-pid half over the run's files.
pub fn parent_findings<T: std::borrow::Borrow<String>>(files: &[String], read: impl Fn(&str) -> Option<T>) -> Vec<Finding> {
    let fix = "Put the rows into a paddock_proc table (paddock_proc::Listed, table_of_listing) and ask paddock_proc::parent_of or \
               descendants, which follow a link only from an older process to a newer one (ADR 0033). A read that only feeds that \
               table, or checks the parent's start time itself, carries `// conformance: parent-pid — <reason>`.";
    let mut out = Vec::new();
    for f in files.iter().filter(|f| f.ends_with(".rs")) {
        let Some(text) = read(f) else { continue };
        for r in parent_reads(std::borrow::Borrow::<String>::borrow(&text).as_str(), f) {
            let site = Finding::error(
                "C53",
                53,
                f,
                r.line,
                format!(
                    "reads the raw parent pid `{}`: its parent may have ended and the pid been handed on (Windows does so at once)",
                    r.field
                ),
                fix,
            );
            out.push(r.waived.apply(site, || {
                Finding::error(
                    "C53",
                    53,
                    f,
                    r.line,
                    "the parent-pid waiver gives no reason",
                    "Write `// conformance: parent-pid — <why this link cannot name a stranger>`.",
                )
            }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raw_parent_field_is_found_outside_paddock_proc_and_a_justified_one_is_not_an_error() {
        let text = "fn a(e: E) -> u32 {\n    e.th32ParentProcessID\n}\n\
                    fn b(i: I) -> u32 {\n    // conformance: parent-pid — feeds paddock_proc::table_of_listing\n    i.pbsi_ppid\n}\n\
                    // th32ParentProcessID in a comment\nconst S: &str = \"InheritedFromUniqueProcessId\";\n";
        let reads = parent_reads(text, "apps/x/src/a.rs");
        assert_eq!(
            reads.iter().map(|r| (r.line, r.field.as_str())).collect::<Vec<_>>(),
            [(2, "th32ParentProcessID"), (6, "pbsi_ppid")]
        );
        assert_eq!(reads[0].waived, InlineWaiver::No);
        assert!(matches!(reads[1].waived, InlineWaiver::Reason(_)));
        assert!(
            parent_reads(text, "crates/paddock-proc/src/sys.rs").is_empty(),
            "the implementation"
        );
        assert!(parent_reads(text, "apps/x/vendor/d/src/w.rs").is_empty(), "vendored code");
    }
}
