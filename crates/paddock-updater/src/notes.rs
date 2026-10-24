//! What's New: the release notes of the running version, from the app's `CHANGELOG.md` (compiled into the binary).
//!
//! The notes shown are always the running version's: when the changelog has no section for it, there are none, and the
//! newest release's notes are never shown in their place (Debrief's fixed bug: a build between releases showed another
//! version's notes as its own).
//!
//! Format read: `## <version>` per release, with an optional `v`, square brackets and a date as `(2026-10-24)` or
//! `- 2026-10-24`; `### <Group>` per group; `- item` or `* item` per entry, wrapped lines (indented) joined to it.
//! `## Unreleased` and anything before the first release is ignored. Bounded; never panics.

use serde::Serialize;

/// A changelog longer than this many characters is read up to there.
pub const MAX_CHANGELOG_CHARS: usize = 4 << 20;
const MAX_ITEM_CHARS: usize = 4000;
const MAX_ITEMS: usize = 500;
const MAX_GROUPS: usize = 32;

/// One release's notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNotes {
    pub version: String,
    /// `YYYY-MM-DD`, when the heading has one.
    pub date: Option<String>,
    pub groups: Vec<NotesGroup>,
}

/// One group of a release (Added, Changed, Fixed…). Items are plain text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct NotesGroup {
    pub title: String,
    pub items: Vec<String>,
}

/// What What's New shows: the running version and its notes (`None`: the changelog has none for it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WhatsNew {
    pub version: String,
    pub release: Option<ReleaseNotes>,
}

/// What the start-up check decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum WhatsNewDecision {
    /// The app was updated to a version with notes: open What's New once.
    Show,
    /// Nothing to show (a first run, or a version without notes); the version is recorded as seen.
    Remember,
    /// This version was seen already.
    Nothing,
}

/// The notes of `version` in `changelog` (exact semantic version; never another release's).
pub fn release_for(changelog: &str, version: &str) -> Option<ReleaseNotes> {
    let want = semver::Version::parse(version.trim().trim_start_matches('v')).ok()?;
    parse(changelog)
        .into_iter()
        .find(|r| semver::Version::parse(&r.version).is_ok_and(|v| v == want))
}

/// Whether to open What's New at start-up. `seen` is the last version recorded (none on a first run, which only records
/// the version: a fresh install has nothing "new"); a version without notes shows nothing.
pub fn decide(seen: Option<&str>, version: &str, has_notes: bool) -> WhatsNewDecision {
    match seen {
        Some(s) if s == version => WhatsNewDecision::Nothing,
        None => WhatsNewDecision::Remember,
        Some(_) if has_notes => WhatsNewDecision::Show,
        Some(_) => WhatsNewDecision::Remember,
    }
}

/// Every release of the changelog, in the order written (newest first by convention). Empty groups are dropped.
pub fn parse(changelog: &str) -> Vec<ReleaseNotes> {
    let text = match changelog.char_indices().nth(MAX_CHANGELOG_CHARS) {
        Some((i, _)) => &changelog[..i],
        None => changelog,
    };
    let mut releases: Vec<ReleaseNotes> = Vec::new();
    // Whether lines belong to a release (false under `## Unreleased` or before the first release).
    let mut open = false;
    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some(head) = line.strip_prefix("## ") {
            open = match heading(head) {
                Some((version, date)) => {
                    releases.push(ReleaseNotes {
                        version,
                        date,
                        groups: Vec::new(),
                    });
                    true
                }
                None => false,
            };
            continue;
        }
        let Some(release) = releases.last_mut().filter(|_| open) else {
            continue;
        };
        if let Some(title) = line.strip_prefix("### ") {
            if release.groups.len() < MAX_GROUPS {
                release.groups.push(NotesGroup {
                    title: clean(title),
                    items: Vec::new(),
                });
            }
            continue;
        }
        let Some(group) = release.groups.last_mut() else {
            continue;
        };
        let trimmed = line.trim_start();
        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .filter(|_| raw.len() - trimmed.len() < 2)
        {
            if group.items.len() < MAX_ITEMS {
                group.items.push(clean(item));
            }
        } else if !trimmed.is_empty() && raw.starts_with(char::is_whitespace) {
            // A wrapped line of the item above.
            if let Some(last) = group.items.last_mut() {
                let joined = format!("{last} {}", clean(trimmed));
                *last = clip(&joined);
            }
        }
    }
    for r in &mut releases {
        r.groups.retain(|g| !g.items.is_empty());
    }
    releases
}

/// `[v]1.2.3[-pre]` with optional brackets, then an optional `(YYYY-MM-DD)` or `- YYYY-MM-DD`.
fn heading(head: &str) -> Option<(String, Option<String>)> {
    let head = head.trim();
    let (version, rest) = match head.strip_prefix('[') {
        Some(inner) => {
            let end = inner.find(']')?;
            (&inner[..end], &inner[end + 1..])
        }
        None => head.split_once(char::is_whitespace).unwrap_or((head, "")),
    };
    let version = version.trim().trim_start_matches('v');
    let parsed = semver::Version::parse(version).ok()?;
    let rest = rest.trim().trim_start_matches(['-', '–', '—']).trim();
    let rest = rest
        .strip_prefix('(')
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or(rest)
        .trim();
    let date = is_date(rest).then(|| rest.to_string());
    Some((parsed.to_string(), date))
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Plain text: Markdown emphasis markers removed, control characters dropped, bounded.
fn clean(s: &str) -> String {
    clip(
        &s.replace("**", "")
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>(),
    )
    .trim()
    .to_string()
}

fn clip(s: &str) -> String {
    match s.char_indices().nth(MAX_ITEM_CHARS) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}
