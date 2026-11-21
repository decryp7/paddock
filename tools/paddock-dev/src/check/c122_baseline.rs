//! The shrink-only count of C122's rules D and E (ER-46, 2026-11-21): `conformance/baselines/C122.json` holds how many sites of each
//! rule the tree has today, so the two heuristic rules can start report-only without their count growing unseen.
//!
//! ```json
//! {"_comment": "...", "counts": {"D": 5, "E": 31}}
//! ```
//!
//! A run compares the tree's counts with the file ([`judge`]) and the file with the list at the merge base ([`growth_findings`], the
//! comparison [`super::baseline::growth`] shares with C53 and C115): a count above the file, or a file above the merge base, is one
//! unwaivable finding at the file that needs the user's approval; a count below the file asks for the file to be lowered. Like the other
//! lists of report-only checks (ER-41) these are warnings until the check is gated.

use super::Ctx;
use super::baseline::{self, Growth};
use super::finding::Finding;
use std::collections::BTreeMap;

/// The baseline file.
pub const FILE: &str = "conformance/baselines/C122.json";

/// The rules the baseline counts.
pub const RULES: [char; 2] = ['D', 'E'];

/// The per-rule counts of a baseline file's text, or why it cannot be read.
pub fn parse(text: &str) -> Result<BTreeMap<char, usize>, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{FILE} is not JSON: {e}"))?;
    let counts = v["counts"].as_object().ok_or_else(|| format!("{FILE} has no `counts` object"))?;
    let mut out = BTreeMap::new();
    for rule in RULES {
        let n = counts
            .get(&rule.to_string())
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| format!("{FILE}: `counts.{rule}` must be a whole number"))?;
        out.insert(rule, n as usize);
    }
    Ok(out)
}

/// One key per counted site (`D#1`, `D#2`, ...), so a raised count is added keys for the merge-base comparison.
fn keys(counts: &BTreeMap<char, usize>) -> Vec<String> {
    counts.iter().flat_map(|(r, n)| (1..=*n).map(move |i| format!("{r}#{i}"))).collect()
}

fn finding(file: &str, message: String, fix: &str) -> Finding {
    Finding::warn("C122", 40, file, 1, message, fix).unwaivable()
}

/// What the baseline says about the tree's `found` counts: findings at the baseline file, none when they are equal. A file that
/// cannot be read is one check-broken finding; a missing file says nothing in a tree that is no checkout (a fixture or a control).
pub fn judge(ctx: &Ctx, found: &BTreeMap<char, usize>) -> Vec<Finding> {
    let Some(text) = ctx.read(FILE) else {
        if ctx.is_in_memory() || !baseline::is_work_tree(ctx.root()) {
            return Vec::new();
        }
        return vec![finding(
            FILE,
            format!("the check cannot look: {FILE} cannot be read, so the count of rules D and E is not held"),
            "Restore the file: {\"counts\": {\"D\": <n>, \"E\": <n>}}.",
        )];
    };
    let allowed = match parse(&text) {
        Ok(a) => a,
        Err(why) => {
            return vec![finding(
                FILE,
                format!("the check cannot look: {why}"),
                "Restore the file: {\"counts\": {\"D\": <n>, \"E\": <n>}}.",
            )];
        }
    };
    let mut out = Vec::new();
    for rule in RULES {
        let (have, may) = (found.get(&rule).copied().unwrap_or(0), allowed.get(&rule).copied().unwrap_or(0));
        if have > may {
            out.push(finding(
                FILE,
                format!("rule {rule} has {have} sites and the baseline allows {may}: the count may only shrink"),
                "Fix or waive the new sites (`// conformance: path-text — <why>`). Raising the baseline needs the user's approval.",
            ));
        } else if have < may {
            out.push(finding(
                FILE,
                format!("rule {rule} has {have} sites and the baseline still allows {may}"),
                &format!("Lower `counts.{rule}` in {FILE} to {have}."),
            ));
        }
    }
    out.extend(growth_findings(ctx, &allowed));
    out
}

/// The one finding about the file's growth against the merge base: empty when nothing was added or it cannot apply here.
fn growth_findings(ctx: &Ctx, allowed: &BTreeMap<char, usize>) -> Vec<Finding> {
    if ctx.is_in_memory() || !baseline::is_work_tree(ctx.root()) {
        return Vec::new();
    }
    let current = keys(allowed);
    let entries_of = |text: &str| parse(text).ok().map(|c| keys(&c));
    let exact = |k: &String| k.clone();
    match baseline::growth(ctx.root(), &ctx.base_ref(), FILE, &current, &entries_of, &exact, None) {
        Growth::Unmeasured(why) => vec![baseline::unmeasured_finding(
            ctx.in_ci(),
            "C122",
            40,
            FILE,
            "the count baseline",
            &why,
        )],
        Growth::Added { added: 0, .. } => Vec::new(),
        Growth::Added { base, added, .. } => vec![finding(
            FILE,
            format!(
                "the count baseline grew by {added} site{} against {FILE} at the merge base {}; it may only shrink",
                if added == 1 { "" } else { "s" },
                &base[..base.len().min(9)]
            ),
            "Lower the counts again and fix the sites. Growing the baseline needs the user's approval.",
        )],
    }
}
