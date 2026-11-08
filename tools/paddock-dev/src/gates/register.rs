//! The gate register `docs/gates.toml` (ADR 0060 §3): data about instances that are deliberately not gated, conditional,
//! report-only, ratcheted or planned, each with a reason, an owner and a review date. It is parsed once into typed values;
//! the rules of §3.1 and §3.2 are checked here, in a pure function of the register text, today's date and the history.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A calendar day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(pub i64);

impl Date {
    /// Parses `YYYY-MM-DD`.
    pub fn parse(s: &str) -> Option<Date> {
        let mut p = s.trim().split('-');
        let (y, m, d): (i64, i64, i64) = (p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?);
        if p.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        // days_from_civil (Howard Hinnant)
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Some(Date(era * 146_097 + doe - 719_468))
    }

    /// Today by the system clock (the edge passes it in; the core never reads a clock).
    pub fn today() -> Date {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Date(i64::try_from(secs / 86_400).unwrap_or(0))
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let z = self.0 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        write!(f, "{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
    }
}

/// Who approved an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Approver {
    /// The coordinator.
    Coordinator,
    /// The user.
    User,
}

/// The fields every dated entry carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Stamp {
    /// Why.
    pub reason: String,
    /// Who works on it.
    pub owner: String,
    /// When it was entered.
    pub since: Date,
    /// When it is reviewed at the latest.
    pub review_by: Date,
    /// Who approved it.
    pub approved: Approver,
}

/// An entry of `not_gated`, `conditional`, `report_only` or `level`.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The check id.
    pub id: String,
    /// The apps it concerns (per-app checks).
    pub apps: Vec<String>,
    /// For `level`: the lowered level.
    pub level: Option<String>,
    /// The common fields.
    pub stamp: Stamp,
}

/// An entry of `planned`: a check id that docs name before it exists.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    /// The check id.
    pub id: String,
    /// The ADR that plans it.
    pub adr: String,
    /// The common fields (the reason is the ADR).
    pub stamp: Stamp,
}

/// An entry of `retired`: an id that existed.
#[derive(Debug, Clone, PartialEq)]
pub struct Retired {
    /// The check id.
    pub id: String,
    /// The commit that removed it.
    pub commit: String,
    /// Where its job went.
    pub moved_to: String,
}

/// Where a registered ignored or env-gated test is gated.
#[derive(Debug, Clone, PartialEq)]
pub enum Gate {
    /// Not gated, on purpose, for now.
    NotGated,
    /// A recorded measurement (rule 48).
    Measurement,
    /// A CI job, `job:<workflow>#<job>`.
    Job(String),
    /// A registered harness.
    Harness(String),
    /// A developer tool or hook.
    Tool(String),
}

/// An entry of `ignored`: tests of one file with one gate.
#[derive(Debug, Clone, PartialEq)]
pub struct IgnoredEntry {
    /// The file.
    pub path: String,
    /// The test names, or `*`.
    pub tests: Vec<String>,
    /// The gate.
    pub gate: Gate,
    /// The common fields.
    pub stamp: Stamp,
}

/// A ratchet: an adoption list that may only shrink.
#[derive(Debug, Clone, PartialEq)]
pub struct Ratchet {
    /// The check id.
    pub id: String,
    /// The app.
    pub app: String,
    /// The members not yet opted in.
    pub not_yet: Vec<String>,
    /// Who works on it.
    pub owner: String,
    /// The release whose tag is refused while the list is not empty (`debrief-v1.3.0`).
    pub target: String,
}

/// A harness: files outside CI that run checks.
#[derive(Debug, Clone, PartialEq)]
pub struct Harness {
    /// Its name.
    pub name: String,
    /// Its path globs.
    pub paths: Vec<String>,
}

/// The parsed register.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Register {
    /// Repository variables as CI sets them.
    pub vars: BTreeMap<String, String>,
    /// Rule-bearing doc globs.
    pub docs_include: Vec<String>,
    /// Doc globs that are history.
    pub docs_exclude: Vec<String>,
    /// Harnesses.
    pub harness: Vec<Harness>,
    /// Globs of the files whose results blocks record measurements (rule 48): `docs/benchmarks.md`, each app's `docs/performance.md`.
    pub measurements: Vec<String>,
    /// Checks that are not gated, on purpose.
    pub not_gated: Vec<Entry>,
    /// Checks that run only under a condition on branches.
    pub conditional: Vec<Entry>,
    /// Checks accepted as report-only.
    pub report_only: Vec<Entry>,
    /// Checks with a lowered required level.
    pub level: Vec<Entry>,
    /// Ids that docs name before they exist.
    pub planned: Vec<Planned>,
    /// Ids that existed.
    pub retired: Vec<Retired>,
    /// Ignored and env-gated tests.
    pub ignored: Vec<IgnoredEntry>,
    /// Adoption lists.
    pub ratchet: Vec<Ratchet>,
}

fn text(t: &toml::Table, k: &str) -> Option<String> {
    t.get(k).and_then(|v| v.as_str()).map(str::to_string)
}

fn strings(t: &toml::Table, k: &str) -> Vec<String> {
    t.get(k)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn date(t: &toml::Table, k: &str) -> Option<Date> {
    match t.get(k)? {
        toml::Value::Datetime(d) => Date::parse(&d.to_string()),
        toml::Value::String(s) => Date::parse(s),
        _ => None,
    }
}

fn stamp(t: &toml::Table, ctx: &str, errors: &mut Vec<String>) -> Option<Stamp> {
    let mut missing = Vec::new();
    for k in ["reason", "owner", "since", "review_by", "approved"] {
        if !t.contains_key(k) {
            missing.push(k);
        }
    }
    if !missing.is_empty() {
        errors.push(format!("{ctx} lacks {}", missing.join(", ")));
        return None;
    }
    let approved = match text(t, "approved").as_deref() {
        Some("coordinator") => Approver::Coordinator,
        Some("user") => Approver::User,
        other => {
            errors.push(format!("{ctx}: approved = {other:?} must be \"coordinator\" or \"user\""));
            return None;
        }
    };
    let (Some(since), Some(review_by)) = (date(t, "since"), date(t, "review_by")) else {
        errors.push(format!("{ctx}: since and review_by must be dates (YYYY-MM-DD)"));
        return None;
    };
    Some(Stamp {
        reason: text(t, "reason").unwrap_or_default(),
        owner: text(t, "owner").unwrap_or_default(),
        since,
        review_by,
        approved,
    })
}

fn tables<'a>(t: &'a toml::Table, k: &str) -> Vec<&'a toml::Table> {
    t.get(k)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_table()).collect())
        .unwrap_or_default()
}

/// Parses the register. Every problem is returned, none is skipped (fail closed).
pub fn parse(src: &str) -> (Register, Vec<String>) {
    let mut errors = Vec::new();
    let mut r = Register::default();
    let table: toml::Table = match src.parse() {
        Ok(t) => t,
        Err(e) => return (r, vec![format!("docs/gates.toml is not valid TOML: {e}")]),
    };
    if table.get("version").and_then(|v| v.as_integer()) != Some(1) {
        errors.push("docs/gates.toml: version must be 1".into());
    }
    if let Some(v) = table.get("vars").and_then(|v| v.as_table()) {
        r.vars = v.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect();
    }
    if let Some(d) = table.get("docs").and_then(|v| v.as_table()) {
        r.docs_include = strings(d, "include");
        r.docs_exclude = strings(d, "exclude");
    }
    if let Some(m) = table.get("measurements").and_then(|v| v.as_table()) {
        r.measurements = strings(m, "files");
    }
    for h in tables(&table, "harness") {
        r.harness.push(Harness {
            name: text(h, "name").unwrap_or_default(),
            paths: strings(h, "paths"),
        });
    }
    for (key, dst) in [("not_gated", 0), ("conditional", 1), ("report_only", 2), ("level", 3)] {
        for (i, e) in tables(&table, key).into_iter().enumerate() {
            let ctx = format!("[[{key}]] #{} ({})", i + 1, text(e, "id").unwrap_or_default());
            let Some(id) = text(e, "id") else {
                errors.push(format!("{ctx} lacks id"));
                continue;
            };
            let Some(st) = stamp(e, &ctx, &mut errors) else { continue };
            let entry = Entry {
                id,
                apps: strings(e, "apps"),
                level: text(e, "level"),
                stamp: st,
            };
            match dst {
                0 => r.not_gated.push(entry),
                1 => r.conditional.push(entry),
                2 => r.report_only.push(entry),
                _ => r.level.push(entry),
            }
        }
    }
    for (i, e) in tables(&table, "planned").into_iter().enumerate() {
        let ctx = format!("[[planned]] #{} ({})", i + 1, text(e, "id").unwrap_or_default());
        let (Some(id), Some(adr)) = (text(e, "id"), text(e, "adr")) else {
            errors.push(format!("{ctx} lacks id or adr"));
            continue;
        };
        let mut e2 = e.clone();
        e2.entry("reason".to_string())
            .or_insert_with(|| toml::Value::String(format!("planned by ADR {adr}")));
        if let Some(stamp) = stamp(&e2, &ctx, &mut errors) {
            r.planned.push(Planned { id, adr, stamp });
        }
    }
    for e in tables(&table, "retired") {
        r.retired.push(Retired {
            id: text(e, "id").unwrap_or_default(),
            commit: text(e, "commit").unwrap_or_default(),
            moved_to: text(e, "moved_to").unwrap_or_default(),
        });
    }
    for (i, e) in tables(&table, "ignored").into_iter().enumerate() {
        let path = text(e, "path").unwrap_or_default();
        let ctx = format!("[[ignored]] #{} ({path})", i + 1);
        let gate = match text(e, "gate").as_deref() {
            Some("not-gated") => Gate::NotGated,
            Some("measurement") => Gate::Measurement,
            Some(g) if g.starts_with("job:") => Gate::Job(g[4..].to_string()),
            Some(g) if g.starts_with("harness:") => Gate::Harness(g[8..].to_string()),
            Some(g) if g.starts_with("tool:") => Gate::Tool(g[5..].to_string()),
            other => {
                errors.push(format!(
                    "{ctx}: gate = {other:?} must be not-gated, measurement, job:<workflow>#<job>, harness:<name> or tool:<name>"
                ));
                continue;
            }
        };
        let tests = strings(e, "tests");
        if tests.is_empty() {
            errors.push(format!("{ctx} lists no tests"));
            continue;
        }
        if let Some(stamp) = stamp(e, &ctx, &mut errors) {
            r.ignored.push(IgnoredEntry { path, tests, gate, stamp });
        }
    }
    for (i, e) in tables(&table, "ratchet").into_iter().enumerate() {
        let ctx = format!("[[ratchet]] #{} ({})", i + 1, text(e, "id").unwrap_or_default());
        if text(e, "approved").as_deref() != Some("user") {
            errors.push(format!("{ctx} needs approved = \"user\""));
            continue;
        }
        let (Some(id), Some(app), Some(owner), Some(target)) = (text(e, "id"), text(e, "app"), text(e, "owner"), text(e, "target")) else {
            errors.push(format!("{ctx} lacks id, app, owner or target"));
            continue;
        };
        r.ratchet.push(Ratchet {
            id,
            app,
            not_yet: strings(e, "not_yet"),
            owner,
            target,
        });
    }
    (r, errors)
}

/// A problem with the register itself, for a finding.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    /// What is wrong.
    pub message: String,
    /// The next step.
    pub fix: String,
}

fn stamp_problems(kind: &str, id: &str, s: &Stamp, today: Date, out: &mut Vec<Problem>) {
    if s.review_by < today {
        out.push(Problem {
            message: format!("[[{kind}]] {id}: review_by {} has passed", s.review_by),
            fix: "Fix the cause and remove the entry, or move the date with a `Gates-date:` trailer in the commit message.".into(),
        });
    }
    if s.since > today {
        out.push(Problem {
            message: format!("[[{kind}]] {id}: since {} is in the future", s.since),
            fix: "since is the day the entry was made; a later date would buy more than 60 days.".into(),
        });
    }
    if s.review_by.0 - s.since.0 > 60 {
        out.push(Problem {
            message: format!(
                "[[{kind}]] {id}: review_by {} is more than 60 days after since {}",
                s.review_by, s.since
            ),
            fix: "Use a review date within 60 days of since; a longer adoption belongs in a [[ratchet]].".into(),
        });
    }
}

/// The rules of §3.1 that need only the register and today's date.
pub fn check_dates(r: &Register, today: Date) -> Vec<Problem> {
    let mut out = Vec::new();
    for (kind, list) in [
        ("not_gated", &r.not_gated),
        ("conditional", &r.conditional),
        ("report_only", &r.report_only),
        ("level", &r.level),
    ] {
        for e in list {
            stamp_problems(kind, &e.id, &e.stamp, today, &mut out);
        }
    }
    for p in &r.planned {
        stamp_problems("planned", &p.id, &p.stamp, today, &mut out);
    }
    for e in &r.ignored {
        if e.gate == Gate::NotGated {
            stamp_problems("ignored", &e.path, &e.stamp, today, &mut out);
        }
    }
    out
}

/// One version of the register in the history: the commit, its message and the register text.
#[derive(Debug, Clone)]
pub struct Version {
    /// The commit id.
    pub sha: String,
    /// The full message.
    pub message: String,
    /// The register text at that commit.
    pub text: String,
}

fn dated(r: &Register) -> BTreeMap<String, Date> {
    let mut m = BTreeMap::new();
    for e in r.not_gated.iter().chain(&r.conditional).chain(&r.report_only).chain(&r.level) {
        m.insert(
            format!(
                "{}{}",
                e.id,
                if e.apps.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", e.apps.join(","))
                }
            ),
            e.stamp.review_by,
        );
    }
    for p in &r.planned {
        m.insert(p.id.clone(), p.stamp.review_by);
    }
    for e in &r.ignored {
        m.insert(e.path.clone(), e.stamp.review_by);
    }
    m
}

fn entry_key_matches(key: &str, id: &str) -> bool {
    key == id || key.starts_with(&format!("{id} ["))
}

/// Date history (§3.1): a change of `review_by` between two versions needs a `Gates-date: <id> <old> -> <new>; approved:
/// <coordinator|user>` trailer in the commit that made it, and a second move of the same entry needs `approved: user`.
/// `versions` are oldest first.
pub fn date_history(versions: &[Version]) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut moves: BTreeMap<String, u32> = BTreeMap::new();
    let mut prev: Option<BTreeMap<String, Date>> = None;
    for v in versions {
        let (r, _) = parse(&v.text);
        let now = dated(&r);
        if let Some(before) = &prev {
            for (k, new) in &now {
                if let Some(old) = before.get(k).filter(|o| *o != new) {
                    let id = k.split(" [").next().unwrap_or(k);
                    let want = format!("Gates-date: {id} {old} -> {new}; approved:");
                    let line = v.message.lines().find(|l| l.trim().starts_with(&want));
                    let n = moves.entry(k.clone()).or_insert(0);
                    *n += 1;
                    match line {
                        None => out.push(Problem {
                            message: format!(
                                "commit {} moved review_by of {k} from {old} to {new} without a `{want} <coordinator|user>` trailer",
                                short(&v.sha)
                            ),
                            fix: "Make the move in a commit whose message carries the trailer, or put the old date back.".into(),
                        }),
                        Some(l) if *n >= 2 && !l.trim().ends_with("user") => out.push(Problem {
                            message: format!(
                                "commit {} moved review_by of {k} a second time and needs `approved: user`",
                                short(&v.sha)
                            ),
                            fix: "A second move of the same entry is approved by the user.".into(),
                        }),
                        _ => {}
                    }
                }
            }
        }
        prev = Some(now);
    }
    let _ = entry_key_matches;
    out
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(9)]
}

/// The ratchet rule (§3.2): a list may only shrink against the register at the base. Also the release refusal: a tag of the
/// target release or a later one of the same app is refused while the list is not empty.
pub fn check_ratchets(r: &Register, base: Option<&Register>, release_tag: Option<&str>) -> Vec<Problem> {
    let mut out = Vec::new();
    for rt in &r.ratchet {
        if let Some(b) = base.and_then(|b| b.ratchet.iter().find(|x| x.id == rt.id && x.app == rt.app)) {
            let before: BTreeSet<&String> = b.not_yet.iter().collect();
            for m in rt.not_yet.iter().filter(|m| !before.contains(m)) {
                out.push(Problem {
                    message: format!(
                        "[[ratchet]] {} {}: `{m}` was added to not_yet (the list may only shrink)",
                        rt.id, rt.app
                    ),
                    fix: "A new member opts in from its first commit; remove it from not_yet.".into(),
                });
            }
            if b.target != rt.target {
                out.push(Problem {
                    message: format!(
                        "[[ratchet]] {} {}: target moved from {} to {} (needs a `Gates-date:` trailer approved by the user)",
                        rt.id, rt.app, b.target, rt.target
                    ),
                    fix: "Put the target back, or move it in a commit with `Gates-date: <id> <old> -> <new>; approved: user`.".into(),
                });
            }
        }
        if let Some(tag) = release_tag
            && !rt.not_yet.is_empty()
            && version_at_least(tag, &rt.target)
        {
            out.push(Problem {
                message: format!(
                    "tag {tag} is refused: [[ratchet]] {} {} still lists {} members in not_yet (target {})",
                    rt.id,
                    rt.app,
                    rt.not_yet.len(),
                    rt.target
                ),
                fix: "Opt the remaining members in (each clippy-clean under the preset) before tagging this release.".into(),
            });
        }
    }
    out
}

/// Whether `tag` (`debrief-v1.3.1`) names the same app as `target` (`debrief-v1.3.0`) and a version at least as new.
fn version_at_least(tag: &str, target: &str) -> bool {
    let split = |s: &str| -> Option<(String, Vec<u64>)> {
        let (app, v) = s.rsplit_once("-v")?;
        Some((app.to_string(), v.split('.').map(|p| p.parse().ok()).collect::<Option<Vec<u64>>>()?))
    };
    match (split(tag), split(target)) {
        (Some((a, v)), Some((b, w))) => a == b && v >= w,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
version = 1
[docs]
include = ["AGENTS.md"]
exclude = ["docs/adr/**"]

[[harness]]
name = "vm-smoke"
paths = ["scripts/vm-smoke/**"]

[[not_gated]]
id = "C13"
scope = "apps"
apps = ["dyno"]
reason = "missing disallowed-methods"
owner = "ADR 0060 WP-4"
since = 2026-11-08
review_by = 2026-12-06
approved = "coordinator"

[[ignored]]
path = "apps/dyno/crates/dyno-agent/tests/helper_wiring.rs"
tests = ["a", "b"]
gate = "not-gated"
reason = "red until WP-INT"
owner = "Dyno WP-INT"
since = 2026-11-08
review_by = 2026-12-06
approved = "coordinator"

[[ratchet]]
id = "C13"
app = "debrief"
not_yet = ["xtask", "examples/x"]
owner = "DBR-LINT"
target = "debrief-v1.3.0"
approved = "user"
"#;

    #[test]
    fn dates_round_trip() {
        let d = Date::parse("2026-11-08").unwrap();
        assert_eq!(d.to_string(), "2026-11-08");
        assert_eq!(Date::parse("2026-12-06").unwrap().0 - d.0, 28);
        assert_eq!(Date::parse("1970-01-01"), Some(Date(0)));
        assert!(Date::parse("2026-13-01").is_none());
    }

    #[test]
    fn a_good_register_parses_and_a_bad_one_names_every_problem() {
        let (r, errs) = parse(GOOD);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!((r.not_gated.len(), r.ignored.len(), r.ratchet.len(), r.harness.len()), (1, 1, 1, 1));
        let bad = GOOD
            .replace("owner = \"ADR 0060 WP-4\"\n", "")
            .replace("approved = \"user\"", "approved = \"coordinator\"");
        let (_, errs) = parse(&bad);
        assert!(errs.iter().any(|e| e.contains("lacks owner")), "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("needs approved = \"user\"")), "{errs:?}");
        assert!(parse("not = toml = at all").1[0].contains("not valid TOML"));
        assert!(parse("version = 2").1[0].contains("version must be 1"));
    }

    #[test]
    fn an_entry_made_in_the_future_is_a_problem() {
        let (r, _) = parse(GOOD);
        let p = check_dates(&r, Date::parse("2026-11-01").unwrap());
        assert!(p.iter().any(|x| x.message.contains("is in the future")), "{p:?}");
    }

    #[test]
    fn expired_and_overlong_entries_are_problems() {
        let (r, _) = parse(GOOD);
        assert!(check_dates(&r, Date::parse("2026-11-20").unwrap()).is_empty());
        let late = check_dates(&r, Date::parse("2026-12-07").unwrap());
        assert_eq!(late.len(), 2);
        assert!(late[0].message.contains("has passed"));
        let long = GOOD.replace("review_by = 2026-12-06", "review_by = 2027-02-01");
        let (r, _) = parse(&long);
        assert!(
            check_dates(&r, Date::parse("2026-11-20").unwrap())
                .iter()
                .any(|p| p.message.contains("more than 60 days"))
        );
    }

    fn version(sha: &str, message: &str, text: &str) -> Version {
        Version {
            sha: sha.into(),
            message: message.into(),
            text: text.into(),
        }
    }

    #[test]
    fn moving_a_date_needs_its_trailer_and_a_second_move_needs_the_user() {
        let moved = GOOD.replacen("review_by = 2026-12-06", "review_by = 2026-12-20", 1);
        let moved2 = GOOD.replacen("review_by = 2026-12-06", "review_by = 2027-01-10", 1);
        // bad: no trailer
        let bad = date_history(&[version("a1", "feat: add", GOOD), version("b2", "fix: later", &moved)]);
        assert!(
            bad.iter()
                .any(|p| p.message.contains("without a `Gates-date: C13 2026-12-06 -> 2026-12-20; approved:")),
            "{bad:?}"
        );
        // good: the trailer
        let ok = date_history(&[
            version("a1", "feat: add", GOOD),
            version(
                "b2",
                "fix: later\n\nGates-date: C13 2026-12-06 -> 2026-12-20; approved: coordinator",
                &moved,
            ),
        ]);
        assert!(ok.is_empty(), "{ok:?}");
        // a second move approved by the coordinator is refused
        let twice = date_history(&[
            version("a1", "feat: add", GOOD),
            version("b2", "x\n\nGates-date: C13 2026-12-06 -> 2026-12-20; approved: coordinator", &moved),
            version(
                "c3",
                "y\n\nGates-date: C13 2026-12-20 -> 2027-01-10; approved: coordinator",
                &moved2,
            ),
        ]);
        assert!(twice.iter().any(|p| p.message.contains("second time")), "{twice:?}");
    }

    #[test]
    fn ratchets_only_shrink_and_block_the_target_release() {
        let (base, _) = parse(GOOD);
        let grown = GOOD.replace("[\"xtask\", \"examples/x\"]", "[\"xtask\", \"examples/x\", \"newcrate\"]");
        let (now, _) = parse(&grown);
        assert!(check_ratchets(&now, Some(&base), None)[0].message.contains("`newcrate` was added"));
        let shrunk = GOOD.replace("[\"xtask\", \"examples/x\"]", "[\"xtask\"]");
        assert!(check_ratchets(&parse(&shrunk).0, Some(&base), None).is_empty());
        // the tag of the target release (or a later one) is refused while the list is not empty
        assert!(
            check_ratchets(&base, None, Some("debrief-v1.3.0"))[0]
                .message
                .contains("is refused")
        );
        assert!(!check_ratchets(&base, None, Some("debrief-v1.4.2")).is_empty());
        assert!(check_ratchets(&base, None, Some("debrief-v1.2.9")).is_empty());
        assert!(check_ratchets(&base, None, Some("dyno-v9.0.0")).is_empty());
        let empty = GOOD.replace("[\"xtask\", \"examples/x\"]", "[]");
        assert!(check_ratchets(&parse(&empty).0, None, Some("debrief-v1.3.0")).is_empty());
    }
}
