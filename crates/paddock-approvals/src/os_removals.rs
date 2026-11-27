//! The user's approvals of `hostTests` entries that take an operating system away from code (`skip`, `only`): docs/os-removals.toml
//! (conformance C155, ER-46).
//!
//! ```text
//! [[approved]]
//! contract = "crates/paddock.ci.json"
//! by = "user"
//! date = "2026-11-27"
//! why = "..."
//! entries = ["skip paddock-x", "only macos some::module"]
//! ```

use crate::approver::Approver;
use crate::problem::Problem;
use crate::verdict::Verdict;
use std::collections::BTreeSet;

/// One `[[approved]]` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsRemovalRecord {
    /// The CI contract whose entries it approves.
    pub contract: String,
    /// Who approved it (only the user can approve a removal).
    pub by: Approver,
    /// The day.
    pub date: String,
    /// Why.
    pub why: String,
    /// The removal keys exactly as C155 reports them (`only <os> <name>`, `skip <package> [<module>]`).
    pub entries: Vec<String>,
}

/// The register: every record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsRemovals {
    /// The records in file order.
    pub records: Vec<OsRemovalRecord>,
}

/// Reads the register with a real TOML parser. An unknown key, a missing field or a record that names no known approver is a
/// [`Problem`]: a broken register approves nothing.
pub fn parse_os_removals(text: &str) -> Result<OsRemovals, Problem> {
    let t: toml::Table = text
        .parse()
        .map_err(|e| Problem::new("", format!("not valid TOML: {e}")))?;
    if let Some(k) = t
        .keys()
        .find(|k| !["version", "approved"].contains(&k.as_str()))
    {
        return Err(Problem::new(
            "",
            format!("unknown key {k:?} (version, approved)"),
        ));
    }
    let list: &[toml::Value] = match t.get("approved") {
        None => &[],
        Some(toml::Value::Array(a)) => a,
        Some(_) => return Err(Problem::new("approved", "is not an array of tables")),
    };
    let mut records = Vec::new();
    for (i, a) in list.iter().enumerate() {
        let what = format!("approved #{}", i + 1);
        let a = a
            .as_table()
            .ok_or_else(|| Problem::new(&what, "is not a table"))?;
        if let Some(k) = a
            .keys()
            .find(|k| !["contract", "by", "date", "why", "entries"].contains(&k.as_str()))
        {
            return Err(Problem::new(&what, format!("has an unknown key {k:?}")));
        }
        let text = |k: &str| {
            a.get(k)
                .and_then(toml::Value::as_str)
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| Problem::new(&what, format!("has no {k}")))
        };
        let by_word = text("by")?;
        let by = Approver::parse(by_word).ok_or_else(|| {
            Problem::new(
                &what,
                format!(
                    "is approved by {by_word:?}, which is neither the coordinator nor the user"
                ),
            )
        })?;
        let entries = a
            .get("entries")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| Problem::new(&what, "has no entries"))?
            .iter()
            .map(|e| {
                e.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| Problem::new(&what, "holds an entry that is not a string"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        records.push(OsRemovalRecord {
            contract: text("contract")?.to_string(),
            by,
            date: text("date")?.to_string(),
            why: text("why")?.to_string(),
            entries,
        });
    }
    Ok(OsRemovals { records })
}

impl OsRemovals {
    /// Every key the register approves, per contract, whoever approved it (`<contract> <removal key>`): the register's own
    /// shrink-only list.
    pub fn keys(&self) -> Vec<String> {
        self.records
            .iter()
            .flat_map(|r| r.entries.iter().map(move |e| format!("{} {e}", r.contract)))
            .collect()
    }
}

/// One removal key and what the register says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyVerdict {
    /// The removal key.
    pub key: String,
    /// The verdict.
    pub verdict: Verdict,
}

/// The verdict on each removal key of `contract`: every key the branch adds (`added`) is Approved (a record of the user lists it),
/// Mismatch (only a record of another approver lists it) or Missing; every key a record lists that the branch does not add is
/// Stale: the merge base already has it, or the contract no longer does (`current` is the contract's keys now). Sorted by key.
pub fn judge_os_removals(
    register: &OsRemovals,
    contract: &str,
    current: &BTreeSet<String>,
    added: &BTreeSet<String>,
) -> Vec<KeyVerdict> {
    let listed = |key: &str, by: Option<Approver>| {
        register.records.iter().any(|r| {
            r.contract == contract
                && by.is_none_or(|b| r.by == b)
                && r.entries.iter().any(|e| e == key)
        })
    };
    let mut out: Vec<KeyVerdict> = added
        .iter()
        .map(|key| {
            let verdict = if listed(key, Some(Approver::User)) {
                Verdict::Approved
            } else if listed(key, None) {
                Verdict::Mismatch {
                    why: "taking an OS away needs the user (ER-46), `by = \"user\"`".to_string(),
                }
            } else {
                Verdict::Missing {
                    line_to_add: format!(
                        "[[approved]] contract = \"{contract}\", by = \"user\", date = \"<day>\", why = \"<why>\", entries = [\"{key}\"]"
                    ),
                }
            };
            KeyVerdict { key: key.clone(), verdict }
        })
        .collect();
    let approved: BTreeSet<&String> = register
        .records
        .iter()
        .filter(|r| r.contract == contract)
        .flat_map(|r| r.entries.iter())
        .collect();
    for key in approved.into_iter().filter(|k| !added.contains(*k)) {
        let why = if current.contains(key) {
            format!("the merge base already has it in {contract}")
        } else {
            format!("{contract} has no such entry")
        };
        out.push(KeyVerdict {
            key: key.clone(),
            verdict: Verdict::Stale { why },
        });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}
