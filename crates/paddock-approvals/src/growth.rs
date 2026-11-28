//! The user's approval of growth of a shrink-only list (ER-46): one `[[register_growth]]` record in docs/gates.toml.
//!
//! ```text
//! [[register_growth]]
//! check = "C139"
//! by = "user"
//! date = "2026-11-27"
//! count = 2
//! digest = "<growth_digest of the added keys>"
//! ```
//!
//! It names exactly the additions the user saw: the `count` of added entries and the SHA-256 of their sorted keys, so it cannot
//! approve other additions of the same number.

use crate::approver::{Approver, Claim};
use crate::verdict::Verdict;
use sha2::{Digest, Sha256};

/// One `[[register_growth]]` record, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrowthRecord {
    /// The check whose growth it approves.
    pub check: String,
    /// Who approved it (only the user can).
    pub by: Claim,
    /// The day, if written.
    pub date: Option<String>,
    /// The number of added entries it names, if written as an integer.
    pub count: Option<i64>,
    /// The digest of the added keys it names, if written.
    pub digest: Option<String>,
}

impl GrowthRecord {
    pub(crate) fn from_table(t: &toml::Table) -> Option<GrowthRecord> {
        let text = |k: &str| {
            t.get(k)
                .and_then(toml::Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        Some(GrowthRecord {
            check: text("check")?.to_string(),
            by: Claim::from_value(t.get("by")),
            date: text("date").map(str::to_string),
            count: t.get("count").and_then(toml::Value::as_integer),
            digest: text("digest").map(str::to_string),
        })
    }
}

/// The digest an approval names: SHA-256 (hex) of the added keys, sorted and joined by a newline.
pub fn growth_digest(added: &[String]) -> String {
    let mut keys: Vec<&str> = added.iter().map(String::as_str).collect();
    keys.sort_unstable();
    Sha256::digest(keys.join("\n").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The line the user adds to the register to approve `added` for `check` (the date is the user's).
pub fn growth_line(check: &str, added: &[String]) -> String {
    format!(
        "[[register_growth]] check = \"{check}\", by = \"user\", date = \"<day>\", count = {}, digest = \"{}\"",
        added.len(),
        growth_digest(added)
    )
}

/// The verdict on the growth `added` (the exact keys of the entries the branch adds) of `check`, from the register's records.
///
/// With additions: Approved when a record of the user names exactly them; Missing when no record is for the check; Mismatch when
/// records exist and none matches. With no additions, a record for the check is Stale (what it approved is on the target branch);
/// no record is Approved (nothing to approve).
pub fn judge_growth(records: &[GrowthRecord], check: &str, added: &[String]) -> Verdict {
    let of_check: Vec<&GrowthRecord> = records.iter().filter(|r| r.check == check).collect();
    if added.is_empty() {
        return if of_check.is_empty() {
            Verdict::Approved
        } else {
            Verdict::Stale {
                why: format!(
                    "{check} adds nothing against the target branch, so its growth approval is no longer needed"
                ),
            }
        };
    }
    if of_check.is_empty() {
        return Verdict::Missing {
            line_to_add: growth_line(check, added),
        };
    }
    let digest = growth_digest(added);
    let mut why = Vec::new();
    for r in of_check {
        if r.by != Claim::By(Approver::User) {
            why.push("an entry is not `by = \"user\"`".to_string());
        } else if r.date.is_none() {
            why.push("an entry has no `date`".to_string());
        } else if r.count != i64::try_from(added.len()).ok() {
            why.push(format!("an entry's `count` is not {}", added.len()));
        } else if r.digest.as_deref() != Some(digest.as_str()) {
            why.push(format!("an entry's `digest` is not {digest}"));
        } else {
            return Verdict::Approved;
        }
    }
    Verdict::Mismatch {
        why: why.join("; "),
    }
}
