//! Baselines: the findings a legacy repository already has, so CI fails only on new ones (note sections 3.3 and 3.7).

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::{Finding, FindingFingerprint, IsoDate, RuleId};

/// One recorded finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineEntry {
    /// The rule.
    pub rule: RuleId,
    /// The rule's version when it was recorded.
    pub rule_version: u32,
    /// The finding's identity.
    pub fingerprint: FindingFingerprint,
    /// Which of several identical findings (1-based, in file order).
    pub ordinal: u32,
    /// The day it was first recorded.
    pub first_seen: IsoDate,
}

/// The recorded findings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Baseline {
    /// The entries, in the order written.
    pub entries: Vec<BaselineEntry>,
}

/// Why a baseline file was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BaselineError {
    /// Not TOML, too big, or an unknown key.
    #[error("the baseline cannot be read: {0}")]
    Syntax(String),
    /// A version this program does not read.
    #[error(
        "the baseline is version {version} with fingerprint algorithm {algorithm}; this program reads version 1 and algorithm {expected}; record the baseline again"
    )]
    Version {
        /// The file's version.
        version: u32,
        /// The file's algorithm.
        algorithm: u32,
        /// The algorithm this build computes.
        expected: u8,
    },
    /// An entry is wrong.
    #[error("finding number {number}: {problem}")]
    Entry {
        /// 1-based.
        number: usize,
        /// What to fix.
        problem: String,
    },
}

/// The largest baseline file read.
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
/// The most entries.
pub const MAX_ENTRIES: usize = 100_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    version: u32,
    algorithm: u32,
    #[serde(default)]
    finding: Vec<RawEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    rule: String,
    rule_version: u32,
    fingerprint: String,
    ordinal: u32,
    first_seen: String,
}

fn rule_id(text: &str) -> Result<RuleId, String> {
    match text.split_once(':') {
        Some((tool, id)) => RuleId::external(tool, id).map_err(|e| e.to_string()),
        None => RuleId::local(text).map_err(|e| e.to_string()),
    }
}

impl Baseline {
    /// The TOML text.
    pub fn to_toml(&self) -> String {
        let mut out = format!("version = 1\nalgorithm = {}\n", super::ALGORITHM_VERSION);
        for e in &self.entries {
            let rule = e.rule.to_string().replace('\\', "\\\\").replace('"', "\\\"");
            let _ = write!(
                out,
                "\n[[finding]]\nrule = \"{rule}\"\nrule_version = {}\nfingerprint = \"{}\"\nordinal = {}\nfirst_seen = \"{}\"\n",
                e.rule_version, e.fingerprint, e.ordinal, e.first_seen
            );
        }
        out
    }

    /// Reads and checks a baseline file; bounded, never panics.
    pub fn from_toml(text: &str) -> Result<Baseline, BaselineError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(BaselineError::Syntax(format!(
                "the file is {} bytes; at most {MAX_FILE_BYTES} are read",
                text.len()
            )));
        }
        let raw: RawFile = toml::from_str(text).map_err(|e| BaselineError::Syntax(e.message().to_owned()))?;
        if raw.version != 1 || raw.algorithm != u32::from(super::ALGORITHM_VERSION) {
            return Err(BaselineError::Version {
                version: raw.version,
                algorithm: raw.algorithm,
                expected: super::ALGORITHM_VERSION,
            });
        }
        if raw.finding.len() > MAX_ENTRIES {
            return Err(BaselineError::Syntax(format!("more than {MAX_ENTRIES} findings")));
        }
        let mut entries = Vec::with_capacity(raw.finding.len());
        for (i, e) in raw.finding.iter().enumerate() {
            let bad = |problem: String| BaselineError::Entry { number: i + 1, problem };
            entries.push(BaselineEntry {
                rule: rule_id(&e.rule).map_err(bad)?,
                rule_version: e.rule_version,
                fingerprint: FindingFingerprint::from_hex(&e.fingerprint)
                    .ok_or_else(|| bad("the fingerprint is not 64 hex digits".to_owned()))?,
                ordinal: e.ordinal,
                first_seen: IsoDate::parse(&e.first_seen).map_err(|err| bad(err.to_string()))?,
            });
        }
        Ok(Baseline { entries })
    }
}

/// Numbers findings that share a rule and fingerprint 1, 2, 3 in the order given (the caller passes them in file order); a finding
/// without a fingerprint gets 0.
pub fn assign_ordinals(findings: &[Finding]) -> Vec<u32> {
    let mut seen: BTreeMap<(&RuleId, FindingFingerprint), u32> = BTreeMap::new();
    findings
        .iter()
        .map(|f| match f.fingerprint {
            Some(fp) => {
                let n = seen.entry((&f.rule, fp)).or_insert(0);
                *n += 1;
                *n
            }
            None => 0,
        })
        .collect()
}

/// A finding's state against a baseline (SARIF `baselineState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineState {
    /// Not in the baseline: CI fails on it.
    New,
    /// In the baseline, same ordinal.
    Unchanged,
    /// In the baseline, but its ordinal moved.
    Updated,
    /// In the baseline and no longer found.
    Absent,
}

impl BaselineState {
    /// The SARIF word.
    pub fn sarif_name(self) -> &'static str {
        match self {
            BaselineState::New => "new",
            BaselineState::Unchanged => "unchanged",
            BaselineState::Updated => "updated",
            BaselineState::Absent => "absent",
        }
    }
}

/// Findings against a baseline, by index into the slice that was compared.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Compared {
    /// Not in the baseline: CI fails on these.
    pub new: Vec<usize>,
    /// In the baseline, same ordinal.
    pub known: Vec<usize>,
    /// In the baseline, but its ordinal moved (an identical snippet above it came or went): not new.
    pub updated: Vec<usize>,
}

impl Compared {
    /// One state per compared finding, in order, for the SARIF writer.
    pub fn states(&self, len: usize) -> Vec<Option<BaselineState>> {
        let mut out = vec![None; len];
        for (list, state) in [
            (&self.new, BaselineState::New),
            (&self.known, BaselineState::Unchanged),
            (&self.updated, BaselineState::Updated),
        ] {
            for i in list {
                if let Some(slot) = out.get_mut(*i) {
                    *slot = Some(state);
                }
            }
        }
        out
    }
}

/// Compares findings with a baseline. `aliases` maps a deprecated rule id to its current one, so a renamed rule does not turn every
/// baselined finding new. For each (rule, fingerprint) the baseline holds `n` entries: the first `n` findings in file order are known
/// (same ordinal) or updated (the ordinal moved), and any beyond `n` are new, so a copy-pasted finding is never absorbed.
pub fn compare(findings: &[Finding], baseline: &Baseline, aliases: &BTreeMap<RuleId, RuleId>) -> Compared {
    let canon = |r: &RuleId| aliases.get(r).cloned().unwrap_or_else(|| r.clone());
    let mut recorded: BTreeMap<(RuleId, FindingFingerprint), BTreeSet<u32>> = BTreeMap::new();
    for e in &baseline.entries {
        recorded.entry((canon(&e.rule), e.fingerprint)).or_default().insert(e.ordinal);
    }
    let ordinals = assign_ordinals(findings);
    let mut out = Compared::default();
    for (i, f) in findings.iter().enumerate() {
        let Some(fp) = f.fingerprint else {
            out.new.push(i);
            continue;
        };
        match recorded.get(&(canon(&f.rule), fp)) {
            Some(set) if ordinals[i] as usize <= set.len() => {
                if set.contains(&ordinals[i]) {
                    out.known.push(i);
                } else {
                    out.updated.push(i);
                }
            }
            _ => out.new.push(i),
        }
    }
    out
}
