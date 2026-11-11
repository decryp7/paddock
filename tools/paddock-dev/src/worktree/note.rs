//! The lock note of an agent worktree and the UTC time it carries.
//!
//! `git worktree add --lock --reason <note>` stores the note; `list` and `check primary` read it back. The note says what
//! the worktree is for (`owner=<role>:<task>`); the owner map says which agent may write in it (ADR 0083 §1).

use std::time::{SystemTime, UNIX_EPOCH};

/// What a lock note says: `owner=<role>:<task>; branch=<name|detached>; base=<sha>; created=<UTC>; by=<coordinator|agent:<id>>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The owner's role (`implementer`, `integrator`, `reviewer`).
    pub role: String,
    /// The task the worktree is for (`RT-3`, `train/2026-11-10-1`).
    pub task: String,
    /// The branch, or `detached`.
    pub branch: String,
    /// The commit the worktree was made from.
    pub base: String,
    /// When it was made, seconds since the Unix epoch (`None` for the free-text form below).
    pub created: Option<u64>,
    /// `coordinator` or `agent:<id>`.
    pub by: String,
}

/// Why `<role>:<task>` was refused.
pub fn check_owner(owner: &str) -> Result<(String, String), String> {
    let (role, task) = owner.split_once(':').ok_or_else(|| format!("`{owner}` is not <role>:<task>."))?;
    let role_ok = !role.is_empty() && role.len() <= 40 && role.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    let task_ok = !task.is_empty()
        && task.len() <= 120
        && task
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-' | b'@' | b'+'));
    if role_ok && task_ok {
        Ok((role.to_string(), task.to_string()))
    } else {
        Err(format!(
            "`{owner}` is not <role>:<task> (a role of letters, digits and `-`; a task of letters, digits and `._/-@+`)."
        ))
    }
}

impl Note {
    /// The text stored as the lock reason.
    pub fn render(&self) -> String {
        format!(
            "owner={}:{}; branch={}; base={}; created={}; by={}",
            self.role,
            self.task,
            self.branch,
            self.base,
            format_utc(self.created.unwrap_or(0)),
            self.by
        )
    }

    /// Reads a note back. Besides the form above it reads the engineering-rules skill's `<role>: <task text>` (what a
    /// coordinator types with a raw `git worktree add --lock --reason`); a lock with any other text has no note.
    pub fn parse(text: &str) -> Option<Note> {
        Note::parse_structured(text).or_else(|| Note::parse_free(text))
    }

    fn parse_free(text: &str) -> Option<Note> {
        let (role, task) = text.split_once(": ")?;
        let role_ok = !role.is_empty() && role.len() <= 40 && role.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
        (role_ok && !task.trim().is_empty()).then(|| Note {
            role: role.to_string(),
            task: task.trim().to_string(),
            branch: String::new(),
            base: String::new(),
            created: None,
            by: String::new(),
        })
    }

    fn parse_structured(text: &str) -> Option<Note> {
        let mut fields = std::collections::HashMap::new();
        for part in text.split("; ") {
            let (k, v) = part.split_once('=')?;
            fields.insert(k.trim(), v.trim());
        }
        let (role, task) = fields.get("owner")?.split_once(':')?;
        Some(Note {
            role: role.to_string(),
            task: task.to_string(),
            branch: fields.get("branch")?.to_string(),
            base: fields.get("base").copied().unwrap_or("").to_string(),
            created: Some(parse_utc(fields.get("created")?)?),
            by: fields.get("by")?.to_string(),
        })
    }
}

/// Seconds since the Unix epoch now.
pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `2026-11-10T14:56:03Z` for seconds since the Unix epoch.
pub fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3_600, rem % 3_600 / 60, rem % 60)
}

/// The inverse of [`format_utc`]; `None` for any other text.
pub fn parse_utc(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z' {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> { text.get(from..to)?.parse().ok() };
    let (y, m, d, hh, mm, ss) = (num(0, 4)?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3_600 + mm * 60 + ss).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_round_trips_and_matches_known_dates() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_utc(1_793_372_163), "2026-10-30T14:56:03Z");
        for s in [0, 59, 86_399, 86_400, 951_782_400, 1_793_372_163, 4_102_444_800] {
            assert_eq!(parse_utc(&format_utc(s)), Some(s));
        }
        assert_eq!(parse_utc("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_utc("yesterday"), None);
    }

    #[test]
    fn a_note_round_trips_and_foreign_text_has_none() {
        let n = Note {
            role: "integrator".into(),
            task: "train/2026-11-10-1".into(),
            branch: "detached".into(),
            base: "abc123".into(),
            created: Some(1_793_372_163),
            by: "agent:a-1".into(),
        };
        assert_eq!(Note::parse(&n.render()), Some(n));
        assert_eq!(Note::parse(""), None);
        assert_eq!(Note::parse("added with --lock"), None);
    }

    #[test]
    fn the_skills_free_text_form_is_read_too() {
        let n = Note::parse("implementer: RT-3 dev worktree").unwrap();
        assert_eq!(
            (n.role.as_str(), n.task.as_str(), n.created),
            ("implementer", "RT-3 dev worktree", None)
        );
    }

    #[test]
    fn owner_text_is_checked() {
        assert_eq!(check_owner("implementer:RT-3"), Ok(("implementer".into(), "RT-3".into())));
        assert!(check_owner("integrator:train/2026-11-10-1").is_ok());
        for bad in ["implementer", ":x", "x:", "a b:c", "a:b; c", "a:b\nc", "a:b:c d"] {
            assert!(check_owner(bad).is_err(), "{bad}");
        }
    }
}
