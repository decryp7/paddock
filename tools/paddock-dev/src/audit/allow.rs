//! The family's approved npm audit exceptions: `scripts/npm-audit-allow.json`, whose owner is `scripts/npm-audit.mjs` (the app jobs' plain
//! audit). `dev audit` reads the same file, in its declared format and with the same checks and the same rule: an entry counts while
//! its review day is after today, and on its review day it stops counting (ER-46; ER-42: one declared source, never a second list).
//! `parse_allow` is tested against the script's own `parseAllow` on the same fixtures, so the two readers cannot drift apart.

use serde_json::Value;
use std::path::Path;

/// Where the list lives, relative to the repository root.
pub const ALLOW_FILE: &str = "scripts/npm-audit-allow.json";

/// One approved exception.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exception {
    /// The advisory.
    pub id: String,
    /// Why it is accepted.
    pub reason: String,
    /// Who approved it.
    pub approved_by: String,
    /// The approval day, `YYYY-MM-DD`.
    pub approved: String,
    /// The review day: from this day on the entry no longer counts.
    pub review_by: String,
}

impl Exception {
    /// Whether it counts on `today` (`YYYY-MM-DD`; days compare as text).
    pub fn live(&self, today: &str) -> bool {
        self.review_by.as_str() > today
    }
}

fn is_day(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter()
            .enumerate()
            .all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

fn is_ghsa(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 4
        && parts[0] == "GHSA"
        && parts[1..]
            .iter()
            .all(|p| p.len() == 4 && p.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
}

/// The list, parsed and checked as `parseAllow` in scripts/npm-audit.mjs does: every entry has an id, a reason, an approver, an approval
/// day and a later review day, and no id is listed twice.
pub fn parse_allow(text: &str) -> Result<Vec<Exception>, String> {
    let doc: Value = serde_json::from_str(text).map_err(|e| format!("the allow list is not JSON: {e}"))?;
    let entries = doc
        .get("allow")
        .and_then(Value::as_array)
        .ok_or("the allow list has no \"allow\" array")?;
    let mut out: Vec<Exception> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let n = i + 1;
        let field = |k: &str| -> Result<String, String> {
            match e.get(k).and_then(Value::as_str) {
                Some(s) if !s.trim().is_empty() => Ok(s.to_string()),
                _ => Err(format!("entry {n} has no {k}")),
            }
        };
        let x = Exception {
            id: field("id")?,
            reason: field("reason")?,
            approved_by: field("approvedBy")?,
            approved: field("approved")?,
            review_by: field("reviewBy")?,
        };
        if !is_ghsa(&x.id) {
            return Err(format!("entry {n}: {} is not a GHSA id", x.id));
        }
        if !is_day(&x.approved) || !is_day(&x.review_by) {
            return Err(format!("entry {n} ({}): days are YYYY-MM-DD", x.id));
        }
        if x.review_by <= x.approved {
            return Err(format!("entry {n} ({}): the review day is not after the approval day", x.id));
        }
        if out.iter().any(|o| o.id == x.id) {
            return Err(format!("entry {n}: {} is listed twice", x.id));
        }
        out.push(x);
    }
    Ok(out)
}

/// Reads the list under `root`. A missing file is an error: the list is part of the tree, and "no exceptions" is an empty array.
pub fn load(root: &Path) -> Result<Vec<Exception>, String> {
    let file = root.join(ALLOW_FILE);
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{} cannot be read: {e}", file.display()))?;
    parse_allow(&text).map_err(|e| format!("{} is broken: {e}", file.display()))
}

/// Today's date in UTC, `YYYY-MM-DD` (civil-from-days, Howard Hinnant's algorithm), as `npm-audit.mjs` reads it.
pub fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    day_of(i64::try_from(secs / 86_400).unwrap_or(0))
}

/// The civil day of a count of days since 1970-01-01.
pub fn day_of(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}
