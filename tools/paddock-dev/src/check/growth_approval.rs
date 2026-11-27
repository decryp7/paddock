//! The user's approval of growth of a shrink-only list (ER-46), read by every list's growth check: the ONE reader (ER-22; C155 first
//! carried it, then C53, C139 and the allow lists each had only a warning or an unlandable error).
//!
//! The approval is one entry in docs/gates.toml (the register of gates), made by the user and never by an agent:
//!
//! ```toml
//! [[register_growth]]
//! check = "C139"
//! by = "user"
//! date = "2026-11-27"
//! count = 2
//! digest = "<growth_digest of the added keys>"
//! ```
//!
//! It names exactly the additions the user saw: the `count` of added entries and the SHA-256 of their sorted keys, so it cannot
//! approve other additions of the same number. A matching entry turns the growth finding off; a non-matching one says why; a missing
//! one keeps the finding and prints the line to add ([`standing`]). The added keys come from [`super::baseline::Growth::Added`]'s
//! `fresh`. A broken register approves nothing.

use super::Ctx;
use sha2::{Digest, Sha256};

/// The register of the user's approvals of growth: docs/gates.toml.
pub const REGISTER: &str = "docs/gates.toml";

/// The digest an approval names: SHA-256 (hex) of the added keys, sorted and joined by a newline.
pub fn growth_digest(added: &[String]) -> String {
    let mut keys: Vec<&str> = added.iter().map(String::as_str).collect();
    keys.sort_unstable();
    Sha256::digest(keys.join("\n").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// What the register says about the additions of one check.
#[derive(Debug, PartialEq, Eq)]
pub enum GrowthApproval {
    /// An entry of the user names these additions exactly (count and digest).
    Approved,
    /// No entry for the check (or the register cannot be read: a broken register approves nothing).
    Missing,
    /// An entry for the check exists but none matches; the reasons.
    Mismatch(String),
}

/// The user's approval of `added` for `check`, read from `gates` (the text of docs/gates.toml).
pub fn growth_approval(gates: Option<&str>, check: &str, added: &[String]) -> GrowthApproval {
    let Some(table) = gates.and_then(|t| t.parse::<toml::Table>().ok()) else {
        return GrowthApproval::Missing;
    };
    let entries: Vec<&toml::Table> = table
        .get("register_growth")
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_table))
        .into_iter()
        .flatten()
        .filter(|e| e.get("check").and_then(toml::Value::as_str) == Some(check))
        .collect();
    if entries.is_empty() {
        return GrowthApproval::Missing;
    }
    let digest = growth_digest(added);
    let mut why = Vec::new();
    for e in entries {
        let s = |k: &str| e.get(k).and_then(toml::Value::as_str).filter(|v| !v.trim().is_empty());
        if s("by") != Some("user") {
            why.push("an entry is not `by = \"user\"`".to_string());
        } else if s("date").is_none() {
            why.push("an entry has no `date`".to_string());
        } else if e.get("count").and_then(toml::Value::as_integer) != i64::try_from(added.len()).ok() {
            why.push(format!("an entry's `count` is not {}", added.len()));
        } else if s("digest") != Some(digest.as_str()) {
            why.push(format!("an entry's `digest` is not {digest}"));
        } else {
            return GrowthApproval::Approved;
        }
    }
    GrowthApproval::Mismatch(why.join("; "))
}

/// What a growth check does with its finding.
#[derive(Debug, PartialEq, Eq)]
pub enum Standing {
    /// The user approved exactly these additions: no finding.
    Approved,
    /// The finding stands. `detail` (empty, or a parenthesis starting with a space) goes on its message, `ask` (a sentence) on its fix.
    Stands {
        /// Why a present entry does not match, or empty when there is none.
        detail: String,
        /// The exact line the user adds.
        ask: String,
    },
}

/// The line the user adds to [`REGISTER`] to approve `added` for `check`, as the finding's fix prints it (the date is the user's).
pub fn approval_line(check: &str, added: &[String]) -> String {
    format!(
        "[[register_growth]] check = \"{check}\", by = \"user\", date = \"<day>\", count = {}, digest = \"{}\"",
        added.len(),
        growth_digest(added)
    )
}

/// What `check`'s growth finding does given the additions (`fresh` of the growth, their exact keys) and the register of this run's tree.
pub fn standing(ctx: &Ctx, check: &str, added: &[String]) -> Standing {
    let gates = ctx.read(REGISTER);
    let detail = match growth_approval(gates.as_deref().map(String::as_str), check, added) {
        GrowthApproval::Approved => return Standing::Approved,
        GrowthApproval::Missing => String::new(),
        GrowthApproval::Mismatch(why) => {
            format!(" ({REGISTER} has a [[register_growth]] entry for {check} that does not match: {why})")
        }
    };
    Standing::Stands {
        detail,
        ask: format!("Or the user adds to {REGISTER}: {} (ER-46).", approval_line(check, added)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gates_with(check: &str, count: usize, digest: &str, by: &str) -> String {
        format!(
            "version = 1\n[[register_growth]]\ncheck = \"{check}\"\nby = \"{by}\"\ndate = \"2026-11-27\"\ncount = {count}\ndigest = \"{digest}\"\n"
        )
    }

    fn two() -> Vec<String> {
        vec!["tools/x.json skip b::".to_string(), "tools/x.json skip a::".to_string()]
    }

    #[test]
    fn growth_with_the_users_matching_approval_is_approved_whatever_the_order_of_the_additions() {
        let digest = growth_digest(&two());
        assert_eq!(
            growth_approval(Some(&gates_with("CX", 2, &digest, "user")), "CX", &two()),
            GrowthApproval::Approved
        );
    }

    #[test]
    fn growth_without_an_approval_is_missing_so_the_finding_stands() {
        assert_eq!(growth_approval(None, "CX", &two()), GrowthApproval::Missing);
        assert_eq!(growth_approval(Some("version = 1\n"), "CX", &two()), GrowthApproval::Missing);
        assert_eq!(
            growth_approval(Some("not toml ["), "CX", &two()),
            GrowthApproval::Missing,
            "a broken register approves nothing"
        );
        // An approval of another check is not this check's.
        let digest = growth_digest(&two());
        assert_eq!(
            growth_approval(Some(&gates_with("CY", 2, &digest, "user")), "CX", &two()),
            GrowthApproval::Missing
        );
    }

    #[test]
    fn an_approval_that_does_not_match_the_additions_or_is_not_the_users_approves_nothing() {
        let digest = growth_digest(&two());
        let mismatch = |gates: String, added: &[String]| matches!(growth_approval(Some(&gates), "CX", added), GrowthApproval::Mismatch(_));
        // One more addition than the user saw.
        let mut more = two();
        more.push("tools/x.json skip c::".into());
        assert!(mismatch(gates_with("CX", 2, &digest, "user"), &more));
        // The same count with another content.
        let mut other = two();
        other[0] = "tools/x.json skip z::".into();
        assert!(mismatch(gates_with("CX", 2, &digest, "user"), &other));
        // A wrong count with the right digest.
        assert!(mismatch(gates_with("CX", 3, &digest, "user"), &two()));
        // Not the user's.
        assert!(mismatch(gates_with("CX", 2, &digest, "coordinator"), &two()));
    }

    #[test]
    fn the_digest_is_sha256_of_the_sorted_keys_joined_by_a_newline() {
        // The oracle is a known SHA-256 (of "a\nb"), not this module's code.
        assert_eq!(
            growth_digest(&["b".to_string(), "a".to_string()]),
            "7e18f737311b2dc3b2f269dd78396b0351f14fb66efa879f768cb23181883c78"
        );
    }
}
