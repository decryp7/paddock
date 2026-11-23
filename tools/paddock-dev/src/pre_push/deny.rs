//! The push gate's `cargo deny` row: licences, bans and sources on every push; advisories only when the push changes what a
//! workspace depends on (docs/ci-schedules.md "Dependency audits").
//!
//! An advisory is published by someone else, so it turns every push of every branch red with no change in the code (RUSTSEC-2026-0543
//! on chrono, run 8214; lettre 0.11.23 on 2026-11-18). Like Dependabot and a scheduled `cargo deny`, the push asks "does this change
//! add one?" and the nightly audit (`./dev audit nightly`) asks "does main have one?". The scope is a value computed here from the
//! change, and it alone decides the arguments of the command and the line printed; no caller adds or drops `advisories` by hand.
//! An advisory is a time-based condition (ER-41, user 2026-11-23): a push that does not change the dependencies still looks at them, in a
//! row of its own that only warns; one that does, and the nightly, block. A change whose base is unknown is not measured, so the advisories run (ADR 0089: not measured never passes).

use super::gates::Change;

/// What the deny row checks for one workspace and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Licences, bans, sources and advisories.
    All {
        /// Why the advisories run (printed).
        reason: String,
    },
    /// Licences, bans and sources only.
    WithoutAdvisories {
        /// Why they are left out (printed, with the nightly that covers them).
        reason: String,
    },
}

impl Scope {
    /// The `cargo deny` arguments of this scope.
    pub fn args(&self) -> Vec<String> {
        let mut a: Vec<String> = ["deny", "--workspace", "check"].iter().map(|s| s.to_string()).collect();
        if matches!(self, Scope::WithoutAdvisories { .. }) {
            a.extend(["licenses", "bans", "sources"].iter().map(|s| s.to_string()));
        }
        a
    }

    /// The arguments of the warning row (a push that does not touch the dependencies): the advisories alone, whose failure only
    /// warns (ER-41). `None` when the advisories already run in the blocking row.
    pub fn warning_args(&self) -> Option<Vec<String>> {
        match self {
            Scope::All { .. } => None,
            Scope::WithoutAdvisories { .. } => Some(
                ["deny", "--workspace", "check", "advisories"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
        }
    }

    /// The row label: the full command's row is plain, a row that leaves the advisories out says what it did check, so it does not
    /// read as a full pass.
    pub fn label(&self, id: &str) -> String {
        match self {
            Scope::All { .. } => id.to_string(),
            Scope::WithoutAdvisories { .. } => format!("{id} (licences, bans, sources)"),
        }
    }

    /// The one line a push prints when the advisories are left out; `None` when they run.
    pub fn notice(&self, workspace: &str) -> Option<String> {
        match self {
            Scope::All { .. } => None,
            Scope::WithoutAdvisories { reason } => Some(format!(
                "cargo deny in {workspace}: advisories only warn on this push, {reason}; a push that changes the dependencies and the nightly audit (`./dev audit nightly`) block on them"
            )),
        }
    }
}

/// Whether `file` changes what `workspace` (a folder such as `apps/pitwall`) depends on: its own lockfile, any manifest below it, its
/// `deny.toml`, a manifest of the shared crates it reaches by path, or a `.cargo/config.toml`.
fn changes_dependencies(file: &str, workspace: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    let inside = file.starts_with(&format!("{workspace}/"));
    let shared = file.starts_with("crates/") && name == "Cargo.toml";
    // Cargo's own configuration (a `[patch]`, a source replacement) changes the resolved graph: the root one and any below the workspace.
    let cargo_config = matches!(file, ".cargo/config.toml" | ".cargo/config")
        || (inside && (file.contains("/.cargo/config.toml") || file.contains("/.cargo/config")));
    (inside && matches!(name, "Cargo.lock" | "Cargo.toml" | "deny.toml")) || shared || cargo_config
}

/// The scope of the deny row of `workspace` for this push. A change with no known base runs the advisories.
pub fn scope(change: &Change, workspace: &str) -> Scope {
    if change.base.is_none() {
        return Scope::All {
            reason: "the base of the push is not known, so what it adds is not measured".into(),
        };
    }
    match change.files.iter().find(|f| changes_dependencies(f, workspace)) {
        Some(f) => Scope::All {
            reason: format!("the push changes {f}"),
        },
        None => Scope::WithoutAdvisories {
            reason: format!("the push changes no Cargo.lock or Cargo.toml {workspace} depends on"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(files: &[&str], base: Option<&str>) -> Change {
        Change {
            files: files.iter().map(|f| f.to_string()).collect(),
            ranges: vec![],
            base: base.map(String::from),
        }
    }

    #[test]
    fn the_scope_is_decided_by_the_change_and_names_its_reason() {
        let ws = "apps/pitwall";
        let skip = scope(&change(&["apps/pitwall/src/a.rs", "apps/dyno/Cargo.lock"], Some("b")), ws);
        assert_eq!(skip.args().join(" "), "deny --workspace check licenses bans sources");
        assert!(skip.notice(ws).unwrap().contains("nightly"));
        for f in [
            "apps/pitwall/app/src-tauri/Cargo.lock",
            "apps/pitwall/Cargo.toml",
            "crates/a/Cargo.toml",
            ".cargo/config.toml",
            "apps/pitwall/app/.cargo/config.toml",
        ] {
            let all = scope(&change(&[f], Some("b")), ws);
            assert_eq!(all.args().join(" "), "deny --workspace check", "{f}");
            assert!(all.notice(ws).is_none(), "{f}");
        }
        // Positive control: no base, no skip.
        assert!(matches!(scope(&change(&["apps/pitwall/src/a.rs"], None), ws), Scope::All { .. }));
    }
}
