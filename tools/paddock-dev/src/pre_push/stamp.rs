//! The one key function of the push gate's local records (ADR 0042 §5: local reuse and CI reuse share one key, rule 55: one
//! implementation per mechanism; ADR 0038 WP-6, the G26 stamp).
//!
//! Two kinds of record live in `<git common dir>/paddock-pre-push/records/`: the whole-run pass record of a pushed commit
//! (`<sha>.json`, [`super::known`]) and the per-row records of [`super::reuse`] (`rows/<key>.json`). Both name their inputs through
//! this module: [`tree_of`] reads a commit's tree id, and [`digest`] turns the fields of a key (the tree or the tree hashes of a row's
//! inputs, the tool, the command) into one hex string. The whole-run record stores its own [`run_stamp`]; a row record's key is a
//! [`digest`]. A second hash function for either is a copy.
//!
//! The CI verdict reuse (ADR 0113, `./dev ci reuse`) decides on a commit and the scope its run declared, over runs of the last 24
//! hours, through the one verdict reader; it does not read these records, and these records never stand in for a CI run (ER-49, ER-15).

use crate::plan_repo::git_text;
use sha2::{Digest, Sha256};
use std::path::Path;

/// The folder of the per-row records, below the records folder of the whole-run pass records.
pub const ROWS_DIR: &str = "records/rows";

/// SHA-256 over length-prefixed fields, as lower-case hex: two field lists differ in the key whenever they differ in any field or in
/// where the fields split.
pub fn digest(fields: &[String]) -> String {
    let mut h = Sha256::new();
    for f in fields {
        h.update((f.len() as u64).to_le_bytes());
        h.update(f.as_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The tree id of `rev` (a commit), or `None` when it is not one here.
pub fn tree_of(root: &Path, rev: &str) -> Option<String> {
    git_text(root, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{tree}}")]).map(|t| t.trim().to_string())
}

/// The stamp of a whole-run pass: the tree, the tool that ran and the set of rows it ran.
pub fn run_stamp(tree: &str, tool_version: &str, row_set: &str) -> String {
    digest(&[format!("tree {tree}"), format!("tool {tool_version}"), format!("rows {row_set}")])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_depends_on_where_fields_split() {
        assert_ne!(digest(&["ab".into(), "c".into()]), digest(&["a".into(), "bc".into()]));
        assert_eq!(digest(&["a".into()]), digest(&["a".into()]));
    }

    #[test]
    fn a_run_stamp_changes_with_the_tree_the_tool_and_the_rows() {
        let base = run_stamp("t1", "0.1.0", "fmt,");
        assert_ne!(base, run_stamp("t2", "0.1.0", "fmt,"));
        assert_ne!(base, run_stamp("t1", "0.2.0", "fmt,"));
        assert_ne!(base, run_stamp("t1", "0.1.0", "fmt,clippy,"));
    }
}
