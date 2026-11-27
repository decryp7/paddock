//! The approvals in the gate register (docs/gates.toml): the `approved` field of each entry and the `[[register_growth]]` records.

use crate::approver::{Approver, Claim};
use crate::growth::GrowthRecord;
use crate::problem::Problem;
use crate::stage::{Severity, Stage, severity};
use crate::verdict::Verdict;
use std::collections::BTreeSet;

/// The tables of the register whose entries carry `approved`, and who must give it. A ratchet is adopted by the user only; the other
/// entries take the coordinator's or the user's.
pub const ENTRY_TABLES: &[(&str, Approver)] = &[
    ("not_gated", Approver::Coordinator),
    ("conditional", Approver::Coordinator),
    ("report_only", Approver::Coordinator),
    ("level", Approver::Coordinator),
    ("planned", Approver::Coordinator),
    ("ignored", Approver::Coordinator),
    ("defect", Approver::Coordinator),
    ("ratchet", Approver::User),
];

/// The fields that tell two entries of one table apart.
const IDENTITY_FIELDS: &[&str] = &["id", "app", "apps", "path", "tests"];

/// The `approved` field of one register entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRecord {
    /// The table (`report_only`).
    pub table: String,
    /// What tells the entry apart in its table (its id, apps, path).
    pub identity: String,
    /// What the entry says about its approval.
    pub claim: Claim,
    /// Who must give it.
    pub needs: Approver,
}

impl EntryRecord {
    /// The entry as a key that names it in a finding and in a comparison with the target branch.
    pub fn key(&self) -> String {
        format!("[[{}]] {}", self.table, self.identity)
    }

    /// The verdict on this entry's approval (never Stale: an entry the register carries is not an exception that landed elsewhere).
    pub fn verdict(&self) -> Verdict {
        self.claim.verdict(
            "approved",
            self.needs,
            format!("approved = \"{}\"", self.needs.as_str()),
        )
    }
}

/// Every approval record of the register, as written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GatesApprovals {
    /// The `approved` field of every entry of [`ENTRY_TABLES`].
    pub entries: Vec<EntryRecord>,
    /// The `[[register_growth]]` records.
    pub growth: Vec<GrowthRecord>,
}

fn identity_of(t: &toml::Table) -> String {
    let mut parts = Vec::new();
    for f in IDENTITY_FIELDS {
        match t.get(*f) {
            Some(toml::Value::String(s)) => parts.push(s.clone()),
            Some(toml::Value::Array(a)) => {
                let items: Vec<&str> = a.iter().filter_map(toml::Value::as_str).collect();
                if !items.is_empty() {
                    parts.push(items.join(","));
                }
            }
            _ => {}
        }
    }
    parts.join(" ")
}

/// Reads the approval records of the register text with a real TOML parser. A text that is not TOML, or a table that is not an
/// array of tables, is a [`Problem`]: a broken register approves nothing.
pub fn parse_gates(text: &str) -> Result<GatesApprovals, Problem> {
    let table: toml::Table = text
        .parse()
        .map_err(|e| Problem::new("", format!("is not valid TOML: {e}")))?;
    let array = |name: &str| -> Result<Vec<&toml::Table>, Problem> {
        match table.get(name) {
            None => Ok(Vec::new()),
            Some(toml::Value::Array(a)) => a
                .iter()
                .map(|v| {
                    v.as_table().ok_or_else(|| {
                        Problem::new(format!("[[{name}]]"), "holds an entry that is not a table")
                    })
                })
                .collect(),
            Some(_) => Err(Problem::new(
                format!("[[{name}]]"),
                "is not an array of tables",
            )),
        }
    };
    let mut out = GatesApprovals::default();
    for (name, needs) in ENTRY_TABLES {
        for t in array(name)? {
            out.entries.push(EntryRecord {
                table: (*name).to_string(),
                identity: identity_of(t),
                claim: Claim::from_value(t.get("approved")),
                needs: *needs,
            });
        }
    }
    out.growth = array("register_growth")?
        .into_iter()
        .filter_map(GrowthRecord::from_table)
        .collect();
    Ok(out)
}

impl GatesApprovals {
    /// The keys of the entries ([`EntryRecord::key`]).
    pub fn entry_keys(&self) -> BTreeSet<String> {
        self.entries.iter().map(EntryRecord::key).collect()
    }
}

/// One entry's verdict and what it counts as at the stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgement {
    /// The entry ([`EntryRecord::key`]).
    pub subject: String,
    /// What the approvals say.
    pub verdict: Verdict,
    /// Whether the branch itself adds the entry (it is not in the target branch's register).
    pub added_by_branch: bool,
    /// How it counts at the stage.
    pub severity: Severity,
}

/// Judges every entry of `now` at `stage`. `base` is the register of the target branch (main): an entry it lacks is added by the
/// branch. When `base` is `None` (the target cannot be read) no entry counts as the branch's, so nothing is lenient (fail closed).
pub fn judge_entries(
    now: &GatesApprovals,
    base: Option<&GatesApprovals>,
    stage: Stage,
) -> Vec<Judgement> {
    let base_keys = base.map(GatesApprovals::entry_keys);
    now.entries
        .iter()
        .filter_map(|e| {
            let verdict = e.verdict();
            let added_by_branch = base_keys.as_ref().is_some_and(|b| !b.contains(&e.key()));
            severity(stage, &verdict, added_by_branch).map(|severity| Judgement {
                subject: e.key(),
                verdict,
                added_by_branch,
                severity,
            })
        })
        .collect()
}
