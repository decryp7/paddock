//! The pure plan of `dev host prune worktrees` (rules in [`crate::prune`]): which folders are candidates, which are kept, and
//! how a row prints.

use crate::prune::FileId;
use std::path::PathBuf;

/// The most `--keep` suffixes.
pub const MAX_KEEP: usize = 256;

/// The folders under `<repo>/target` that belong to the editor and the build, never to a finished agent.
pub const PROTECTED_TARGETS: [&str; 5] = ["debug", "release", "rust-analyzer", "doc", "tmp"];

/// `[a-z][a-z0-9-]*`, the app name of the `.ps1`'s `ValidatePattern`.
pub fn valid_app(app: &str) -> bool {
    let mut bytes = app.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase()) && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Validates the keep suffixes (bounded in number and shape).
pub fn check_keep(keep: &[String]) -> Result<(), crate::prune::PruneError> {
    if keep.len() > MAX_KEEP {
        return Err(crate::prune::PruneError::BadKeep(format!("{} suffixes", keep.len())));
    }
    for k in keep {
        if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
            return Err(crate::prune::PruneError::BadKeep(k.clone()));
        }
    }
    Ok(())
}

fn starts_with_ci(name: &str, prefix: &str) -> bool {
    name.len() >= prefix.len() && name.is_char_boundary(prefix.len()) && name[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// A folder directly in the home is a candidate when it is named `.<app>-target*`.
pub fn home_candidate(app: &str, name: &str) -> bool {
    starts_with_ci(name, &format!(".{app}-target"))
}

/// A folder directly in `<repo>/target` is a candidate unless it is protected or an editor's `flycheck*`.
pub fn target_candidate(name: &str) -> bool {
    !PROTECTED_TARGETS.iter().any(|p| name.eq_ignore_ascii_case(p)) && !starts_with_ci(name, "flycheck")
}

/// Whether a candidate is named by a keep suffix: `.<app>-target-<k>*` or `agent-<k>*`.
pub fn is_kept(app: &str, keep: &[String], name: &str) -> bool {
    keep.iter()
        .any(|k| starts_with_ci(name, &format!(".{app}-target-{k}")) || starts_with_ci(name, &format!("agent-{k}")))
}

/// A listed build folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The folder.
    pub path: PathBuf,
    /// Its name.
    pub name: String,
    /// Summed file lengths.
    pub bytes: u64,
    /// It is a symlink: listed, never followed or removed.
    pub symlink: bool,
    /// It has a Cargo marker at its top level; a folder without one is never removed.
    pub cargo: bool,
    /// Its identity when it was listed; removal checks it is still the same folder.
    pub id: Option<FileId>,
}

/// What happens to a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Listed only (no `--remove`).
    Found,
    /// Named by `--keep`.
    Kept,
    /// Removed.
    Removing,
    /// Would be removed (`--remove --dry-run`).
    WouldRemove,
    /// A symlink: left alone.
    Skipped,
    /// Not a Cargo build folder (no CACHEDIR.TAG or .rustc_info.json): left alone.
    NotCargo,
}

impl Action {
    /// The word in the listing and the JSON.
    pub fn word(self) -> &'static str {
        match self {
            Action::Found => "found",
            Action::Kept => "kept",
            Action::Removing => "removing",
            Action::WouldRemove => "would-remove",
            Action::Skipped => "skipped",
            Action::NotCargo => "not-cargo",
        }
    }
}

/// One row of the listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The folder.
    pub path: PathBuf,
    /// Summed file lengths.
    pub bytes: u64,
    /// What happens to it.
    pub action: Action,
    /// The folder's identity captured at listing time.
    pub id: Option<FileId>,
}

/// Decides every candidate's action.
pub fn plan_rows(app: &str, keep: &[String], remove: bool, dry_run: bool, candidates: &[Candidate]) -> Vec<Row> {
    candidates
        .iter()
        .map(|c| {
            let action = if c.symlink {
                Action::Skipped
            } else if !remove {
                Action::Found
            } else if is_kept(app, keep, &c.name) {
                Action::Kept
            } else if !c.cargo {
                Action::NotCargo
            } else if dry_run {
                Action::WouldRemove
            } else {
                Action::Removing
            };
            Row {
                path: c.path.clone(),
                bytes: c.bytes,
                action,
                id: c.id,
            }
        })
        .collect()
}

/// Gigabytes to one decimal with banker's rounding, as `[math]::Round(x, 1)` gives, grouped as `N1` prints.
pub fn format_gb(bytes: u64) -> String {
    let tenths = (bytes as f64 / 1_073_741_824.0 * 10.0).round_ties_even() as u64;
    let int = (tenths / 10).to_string();
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{grouped}.{}", tenths % 10)
}

/// `'{0,8:N1} GB  {1,-9} {2}'`.
pub fn format_row(row: &Row) -> String {
    format!("{:>8} GB  {:<9} {}", format_gb(row.bytes), row.action.word(), row.path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_names_and_keep_suffixes_are_checked() {
        for ok in ["debrief", "a", "pit-wall2"] {
            assert!(valid_app(ok), "{ok}");
        }
        for bad in ["", "Debrief", "2fast", "-a", "a b", "a_b", "a*"] {
            assert!(!valid_app(bad), "{bad:?}");
        }
        assert_eq!(check_keep(&["6c".into(), "sdk-1.2_x".into()]), Ok(()));
        for bad in ["", "a*", "a?", "[a]", "a/b", "a\\b"] {
            assert_eq!(
                check_keep(&[bad.to_string()]),
                Err(crate::prune::PruneError::BadKeep(bad.to_string())),
                "{bad:?}"
            );
        }
        assert!(check_keep(&vec!["a".to_string(); MAX_KEEP + 1]).is_err());
    }

    #[test]
    fn candidates_are_chosen_by_name_without_regard_to_case() {
        assert!(home_candidate("debrief", ".debrief-target") && home_candidate("debrief", ".Debrief-Target-6c"));
        assert!(
            !home_candidate("debrief", ".dyno-target-x")
                && !home_candidate("debrief", "debrief-target")
                && !home_candidate("debrief", ".deb")
        );
        for protected in ["debug", "Release", "rust-analyzer", "DOC", "tmp", "flycheck0", "Flycheck-x"] {
            assert!(!target_candidate(protected), "{protected}");
        }
        for ok in ["agent-6c", "agent-x", "other", "debugger", "tmp2"] {
            assert!(target_candidate(ok), "{ok}");
        }
        assert!(
            !starts_with_ci("é", "ab") && !starts_with_ci("aé", "a\u{e9}x"),
            "a prefix inside a character is not a match"
        );
        assert!(!starts_with_ci("a", "ab"));
    }

    #[test]
    fn keep_suffixes_name_both_kinds_of_folder() {
        let keep = vec!["6c".to_string(), "sdk".to_string()];
        assert!(is_kept("debrief", &keep, ".debrief-target-6c1") && is_kept("debrief", &keep, "agent-SDK"));
        assert!(
            !is_kept("debrief", &keep, ".debrief-target")
                && !is_kept("debrief", &keep, "agent-7")
                && !is_kept("debrief", &keep, ".dyno-target-6c")
        );
        assert!(!is_kept("debrief", &[], "agent-6c"));
    }

    fn cands() -> Vec<Candidate> {
        let c = |name: &str, bytes: u64, symlink: bool, cargo: bool| Candidate {
            path: PathBuf::from("/h").join(name),
            name: name.into(),
            bytes,
            symlink,
            cargo,
            id: None,
        };
        vec![
            c(".d-target-6c", 1 << 30, false, true),
            c("agent-9", 3 << 30, false, true),
            c("agent-link", 0, true, false),
            c("agent-plain", 1, false, false),
        ]
    }

    #[test]
    fn rows_list_remove_keep_and_skip_links() {
        let keep = vec!["6c".to_string()];
        let words = |remove, dry| {
            plan_rows("d", &keep, remove, dry, &cands())
                .iter()
                .map(|r| r.action.word())
                .collect::<Vec<_>>()
        };
        assert_eq!(words(false, false), ["found", "found", "skipped", "found"]);
        assert_eq!(words(true, false), ["kept", "removing", "skipped", "not-cargo"]);
        assert_eq!(words(true, true), ["kept", "would-remove", "skipped", "not-cargo"]);
        assert_eq!(
            words(false, true),
            ["found", "found", "skipped", "found"],
            "dry run without --remove only lists"
        );
    }

    #[test]
    fn rows_print_like_the_powershell_format() {
        assert_eq!(format_gb(0), "0.0");
        assert_eq!(format_gb(1 << 30), "1.0");
        assert_eq!(format_gb(1 << 28), "0.2", "0.25 GB is 2.5 tenths: a tie rounds to the even 2");
        assert_eq!(format_gb(3 << 28), "0.8", "0.75 GB is 7.5 tenths: a tie rounds to the even 8");
        assert_eq!(format_gb(1_536 * (1 << 20)), "1.5");
        assert_eq!(format_gb(1234 * (1 << 30)), "1,234.0");
        assert_eq!(format_gb(1_234_567 * (1 << 30)), "1,234,567.0");
        let row = Row {
            id: None,
            path: PathBuf::from("/h/agent-9"),
            bytes: 3 << 30,
            action: Action::Removing,
        };
        assert_eq!(format_row(&row), "     3.0 GB  removing  /h/agent-9");
        assert_eq!(Action::Found.word(), "found");
        assert_eq!(Action::Kept.word(), "kept");
    }
}
