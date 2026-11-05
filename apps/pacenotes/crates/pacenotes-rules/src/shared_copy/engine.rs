//! Applying waivers and suppressions to findings (conformance's `applyWaivers`, generalised; note section 3.5).

use pacenotes_core::Severity;
use std::collections::{BTreeMap, BTreeSet};

use super::{
    Finding, FindingFingerprint, FixSentence, FixSource, IsoDate, Origin, RuleId, Suppression, SuppressionScope, Waiver, path_matches,
};

/// Turns the problems of a leniently read waiver file into `W002` findings; the good entries of the file still apply.
pub fn waiver_problems(errors: &[super::WaiverError], file: &str) -> Vec<Finding> {
    errors
        .iter()
        .filter_map(|e| {
            engine_finding(
                "w002",
                Severity::High,
                format!("{file}: {e}"),
                "Fix the entry in the file, or delete it.",
            )
        })
        .collect()
}

/// What hid a finding: the index of the waiver or suppression in the slice that was passed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenBy {
    /// `waivers[i]`.
    Waiver(usize),
    /// `suppressions[i]`.
    Suppression(usize),
}

/// A finding that was hidden, and by what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hidden {
    /// The finding.
    pub finding: Finding,
    /// What hid it.
    pub by: HiddenBy,
}

/// The outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// Findings nothing hid.
    pub kept: Vec<Finding>,
    /// Findings a live waiver or suppression hid.
    pub hidden: Vec<Hidden>,
    /// The engine's own findings (`w001` expired, `w002` incomplete, `w003` unused, `w004` over its ceiling, `w005` ceiling can come
    /// down).
    pub engine: Vec<Finding>,
}

/// One of the engine's own findings. The ids and sentences are literals checked by the unit tests, so `None` never happens.
fn engine_finding(id: &str, severity: Severity, message: String, fix: &str) -> Option<Finding> {
    Some(Finding {
        rule: RuleId::local(id).ok()?,
        severity,
        location: None,
        message,
        fix: FixSentence::parse(fix).ok()?,
        fix_source: FixSource::Rule,
        explanation: None,
        evidence: Vec::new(),
        fix_diff: None,
        fingerprint: None::<FindingFingerprint>,
        confidence: None,
    })
}

fn path_of(f: &Finding) -> &str {
    f.location.as_ref().map_or("", |l| l.path.as_str())
}

fn canon<'a>(id: &'a RuleId, aliases: &'a BTreeMap<RuleId, RuleId>) -> &'a RuleId {
    aliases.get(id).unwrap_or(id)
}

fn suppression_covers(s: &Suppression, f: &Finding, aliases: &BTreeMap<RuleId, RuleId>) -> bool {
    if let Some(rule) = &s.rule
        && canon(rule, aliases) != canon(&f.rule, aliases)
    {
        return false;
    }
    match &s.scope {
        SuppressionScope::Line { path, line } => f
            .location
            .as_ref()
            .is_some_and(|l| l.path.as_str() == path && l.span.start().line() <= *line && *line <= l.span.end().line()),
        SuppressionScope::File { path } => path_of(f) == path,
        SuppressionScope::Path { glob } => path_matches(glob, path_of(f)),
        SuppressionScope::Everywhere => true,
    }
}

/// Splits `findings` into kept and hidden, and reports bad entries. `ran` is the set of rules that actually ran (an unused waiver is
/// only reported for those). `aliases` maps a deprecated rule id to its current one. `today` is the UTC date; an entry is in force
/// through its `expires` day.
pub fn apply_waivers(
    findings: Vec<Finding>,
    waivers: &[Waiver],
    suppressions: &[Suppression],
    ran: &BTreeSet<RuleId>,
    aliases: &BTreeMap<RuleId, RuleId>,
    today: IsoDate,
) -> Applied {
    let mut engine = Vec::new();
    let mut live_waiver = vec![false; waivers.len()];
    for (i, w) in waivers.iter().enumerate() {
        let who = format!("the waiver for {} over {}", w.rule, w.path);
        match w.expires {
            None if w.origin == Origin::File => engine.extend(engine_finding(
                "w002",
                Severity::High,
                format!("{who} has no expiry"),
                "Add an expiry date to the waiver, or delete it.",
            )),
            Some(e) if e < today => engine.extend(engine_finding(
                "w001",
                Severity::High,
                format!("{who} expired on {e}"),
                "Fix the finding, or renew the waiver with a new date and a reason.",
            )),
            _ => live_waiver[i] = true,
        }
    }
    let mut live_suppression = vec![false; suppressions.len()];
    for (i, s) in suppressions.iter().enumerate() {
        match s.expires {
            Some(e) if e < today => engine.extend(engine_finding(
                "w001",
                Severity::High,
                format!("a suppression ({}) expired on {e}", s.scope),
                "Fix the finding, or renew the suppression with a new date and a reason.",
            )),
            _ => live_suppression[i] = true,
        }
    }

    // first live entry that covers a finding takes it
    let mut taken: Vec<Option<HiddenBy>> = Vec::with_capacity(findings.len());
    let mut counts_w = vec![0usize; waivers.len()];
    let mut counts_s = vec![0usize; suppressions.len()];
    for f in &findings {
        let by = waivers
            .iter()
            .enumerate()
            .find(|(i, w)| live_waiver[*i] && canon(&w.rule, aliases) == canon(&f.rule, aliases) && path_matches(&w.path, path_of(f)))
            .map(|(i, _)| HiddenBy::Waiver(i))
            .or_else(|| {
                suppressions
                    .iter()
                    .enumerate()
                    .find(|(i, s)| live_suppression[*i] && suppression_covers(s, f, aliases))
                    .map(|(i, _)| HiddenBy::Suppression(i))
            });
        match by {
            Some(HiddenBy::Waiver(i)) => counts_w[i] += 1,
            Some(HiddenBy::Suppression(i)) => counts_s[i] += 1,
            None => {}
        }
        taken.push(by);
    }

    // a waiver above its ceiling hides nothing; below it, the ceiling can come down; unused ones are reported for rules that ran
    let mut over = vec![false; waivers.len()];
    for (i, w) in waivers.iter().enumerate().filter(|(i, _)| live_waiver[*i]) {
        let n = counts_w[i];
        let who = format!("the waiver for {} over {}", w.rule, w.path);
        match w.max_count {
            Some(max) if n > max as usize => {
                over[i] = true;
                engine.extend(engine_finding(
                    "w004",
                    Severity::High,
                    format!("{who} may hide {max} findings and covers {n}, so it hides none"),
                    "Fix the new findings, or raise the ceiling with a reason.",
                ));
            }
            Some(max) if n < max as usize && n > 0 => engine.extend(engine_finding(
                "w005",
                Severity::Info,
                format!("{who} may hide {max} findings and covers {n}"),
                &format!("Lower the ceiling to {n}."),
            )),
            _ => {}
        }
        if n == 0 && ran.contains(canon(&w.rule, aliases)) {
            engine.extend(engine_finding(
                "w003",
                Severity::Medium,
                format!("{who} hides nothing"),
                "Delete the waiver; the finding is gone.",
            ));
        }
    }
    for (i, s) in suppressions.iter().enumerate().filter(|(i, _)| live_suppression[*i]) {
        let relevant = s.rule.as_ref().is_none_or(|r| ran.contains(canon(r, aliases)));
        if counts_s[i] == 0 && relevant {
            engine.extend(engine_finding(
                "w003",
                Severity::Medium,
                format!("a suppression ({}) hides nothing", s.scope),
                "Delete the suppression; the finding is gone.",
            ));
        }
    }

    let mut kept = Vec::new();
    let mut hidden = Vec::new();
    for (finding, by) in findings.into_iter().zip(taken) {
        match by {
            Some(HiddenBy::Waiver(i)) if over[i] => kept.push(finding),
            Some(by) => hidden.push(Hidden { finding, by }),
            None => kept.push(finding),
        }
    }
    Applied { kept, hidden, engine }
}
