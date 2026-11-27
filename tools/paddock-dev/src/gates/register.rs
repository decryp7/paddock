//! The gate register `docs/gates.toml` (ADR 0060 §3): data about instances that are deliberately not gated, conditional,
//! report-only, ratcheted or planned, each with a reason, an owner and a review date. It is parsed once into typed values;
//! the rules of §3.1 and §3.2 are checked here, in a pure function of the register text, today's date and the history.

use paddock_approvals::{Approver, Verdict, date_move_claim};
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

/// An entry of `defect`: a source file whose lint allowance is a known defect, not a justification (`KNOWN DEFECT` in its reason).
#[derive(Debug, Clone, PartialEq)]
pub struct Defect {
    /// The source file.
    pub path: String,
    /// The common fields; the owner is whoever fixes it (a branch or a work package).
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
    /// Clippy `disallowed-methods` paths the app's clippy.toml may lack while its call sites are cleaned up (C13); also only shrinks.
    pub clippy_not_yet: Vec<String>,
    /// The release whose tag is refused while the list is not empty (`debrief-v1.3.0`).
    pub target: String,
    /// `report-only` when the open lists are reported at the target release and never refuse its tag (the same value as C102's
    /// `blocks_tag`); empty when the tag is refused.
    pub blocks_tag: String,
}

/// The `blocks_tag` value of a ratchet that is reported and never refuses a tag (ADR 0060 amendment of 2026-11-12).
pub const REPORT_ONLY: &str = "report-only";

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
    /// Known defects behind a lint allowance.
    pub defects: Vec<Defect>,
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
    // `approved` is not read here: the approvals have one reader (`paddock_approvals`, judged in `evaluate`).
    for k in ["reason", "owner", "since", "review_by"] {
        if !t.contains_key(k) {
            missing.push(k);
        }
    }
    if !missing.is_empty() {
        errors.push(format!("{ctx} lacks {}", missing.join(", ")));
        return None;
    }
    let (Some(since), Some(review_by)) = (date(t, "since"), date(t, "review_by")) else {
        errors.push(format!("{ctx}: since and review_by must be dates (YYYY-MM-DD)"));
        return None;
    };
    Some(Stamp {
        reason: text(t, "reason").unwrap_or_default(),
        owner: text(t, "owner").unwrap_or_default(),
        since,
        review_by,
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
        let ctx = format!(
            "[[ratchet]] #{} ({} {})",
            i + 1,
            text(e, "id").unwrap_or_default(),
            text(e, "app").unwrap_or_default()
        );
        let (Some(id), Some(app), Some(owner), Some(target)) = (text(e, "id"), text(e, "app"), text(e, "owner"), text(e, "target")) else {
            errors.push(format!("{ctx} lacks id, app, owner or target"));
            continue;
        };
        let blocks_tag = text(e, "blocks_tag").unwrap_or_default();
        if !blocks_tag.is_empty() && blocks_tag != REPORT_ONLY {
            errors.push(format!(
                "{ctx} has blocks_tag = \"{blocks_tag}\": the only value is \"{REPORT_ONLY}\" (omit the key for a ratchet that refuses the tag)"
            ));
            continue;
        }
        r.ratchet.push(Ratchet {
            id,
            app,
            not_yet: strings(e, "not_yet"),
            clippy_not_yet: strings(e, "clippy_not_yet"),
            owner,
            target,
            blocks_tag,
        });
    }
    for (i, e) in tables(&table, "defect").into_iter().enumerate() {
        let path = text(e, "path").unwrap_or_default();
        let ctx = format!("[[defect]] #{} ({path})", i + 1);
        if let Some(stamp) = stamp(e, &ctx, &mut errors) {
            r.defects.push(Defect { path, stamp });
        }
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
    // A report-only entry's passed date is the sunset rule's ([`sunset`]): ER-41's time rule, not a finding on every push.
    if s.review_by < today && kind != "report_only" {
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

/// The first day of the sunset rule (user, 2026-11-24): a `[[report_only]]` entry made on or after it expires at most
/// [`SUNSET_DAYS`] after its `since`.
pub const SUNSET_FROM: &str = "2026-11-25";
/// How long a new report-only check, register or port may stay report-only: on its `review_by` (the expiry date) it is flipped to
/// blocking, switched over (the old side deleted) or removed.
pub const SUNSET_DAYS: i64 = 14;
/// How many days before the expiry date the reminder warning starts (ER-41: a reminder a week before).
pub const REMINDER_DAYS: i64 = 7;
const SUNSET_FIX: &str = "Flip the check to blocking (mode gate, its first green run id recorded where it is defined) and remove the entry, switch it over (delete the old side), or remove the check and the entry. A later date needs a `Gates-date:` trailer approved by the user.";

fn sunset_from() -> Date {
    Date::parse(SUNSET_FROM).unwrap_or(Date(i64::MAX))
}

/// A `[[report_only]]` entry the sunset rule names: past its expiry date (`overdue`) or within [`REMINDER_DAYS`] of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Due {
    /// The entry.
    pub entry: Entry,
    /// Its expiry date has passed.
    pub overdue: bool,
}

impl Due {
    /// What the finding or the warning says.
    pub fn message(&self) -> String {
        let (id, by) = (&self.entry.id, self.entry.stamp.review_by);
        if self.overdue {
            format!("[[report_only]] {id}: review_by {by} has passed: the report-only phase is over (sunset rule)")
        } else {
            format!("[[report_only]] {id}: review_by {by} is near: the report-only phase ends then (sunset rule)")
        }
    }

    /// The next step.
    pub fn fix(&self) -> &'static str {
        SUNSET_FIX
    }

    /// Whether a release tag of `app` (None: a tag of no app, such as the shared packages' `v*`) is held to this entry: an entry
    /// without `apps` concerns every app.
    pub fn concerns(&self, app: Option<&str>) -> bool {
        self.entry.apps.is_empty() || app.is_none_or(|a| self.entry.apps.iter().any(|x| x == a))
    }
}

/// The sunset rule (user, 2026-11-24; ER-41's time rule): every `[[report_only]]` entry whose expiry date (`review_by`) has passed or
/// comes within [`REMINDER_DAYS`]. The caller decides what blocks: an overdue entry blocks a release tag and a change that touches the
/// entry, and is a warning on any other push; a near one is a warning (the reminder).
pub fn sunset(r: &Register, today: Date) -> Vec<Due> {
    r.report_only
        .iter()
        .filter(|e| e.stamp.review_by.0 - today.0 <= REMINDER_DAYS)
        .map(|e| Due {
            entry: e.clone(),
            overdue: e.stamp.review_by < today,
        })
        .collect()
}

/// The sunset rule's limit for new entries: a `[[report_only]]` entry made on or after [`SUNSET_FROM`] expires at most [`SUNSET_DAYS`]
/// after its `since`, and one the register at the base lacks (new on this branch, whatever its `since` says) at most [`SUNSET_DAYS`]
/// after today. `base` is None when the base register could not be read: then only the first half is judged.
pub fn sunset_limits(r: &Register, base: Option<&Register>, today: Date) -> Vec<Problem> {
    let mut out = Vec::new();
    for e in &r.report_only {
        let s = &e.stamp;
        if s.since >= sunset_from() && s.review_by.0 - s.since.0 > SUNSET_DAYS {
            out.push(Problem {
                message: format!(
                    "[[report_only]] {}: review_by {} is more than {SUNSET_DAYS} days after since {} (sunset rule)",
                    e.id, s.review_by, s.since
                ),
                fix: format!("A new report-only check, register or port expires within {SUNSET_DAYS} days of its registration: set review_by to {} at the latest.", Date(s.since.0 + SUNSET_DAYS)),
            });
        }
        let new = base.is_some_and(|b| !b.report_only.iter().any(|x| x.id == e.id));
        if new && s.review_by.0 - today.0 > SUNSET_DAYS {
            out.push(Problem {
                message: format!(
                    "[[report_only]] {} is new and its review_by {} is more than {SUNSET_DAYS} days away (sunset rule)",
                    e.id, s.review_by
                ),
                fix: format!(
                    "Set review_by to {} at the latest, and since to the day the entry is made.",
                    Date(today.0 + SUNSET_DAYS)
                ),
            });
        }
    }
    out
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
    for d in &r.defects {
        stamp_problems("defect", &d.path, &d.stamp, today, &mut out);
    }
    for e in &r.ignored {
        if e.gate == Gate::NotGated {
            stamp_problems("ignored", &e.path, &e.stamp, today, &mut out);
        }
    }
    out
}

/// One version of the register in the history: the commit, its trailers and the register text.
#[derive(Debug, Clone)]
pub struct Version {
    /// The commit id.
    pub sha: String,
    /// The trailers of the message, as git's own parser gives them (`%(trailers:only,unfold)`, ER-42); for a merge or for HEAD those of
    /// every commit it brings in.
    pub trailers: String,
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
    for d in &r.defects {
        m.insert(format!("defect:{}", d.path), d.stamp.review_by);
    }
    m
}

/// Each ratchet's target release and `blocks_tag`, keyed `<id> <app>`.
fn ratchet_state(r: &Register) -> BTreeMap<String, (String, String)> {
    r.ratchet
        .iter()
        .map(|x| (format!("{} {}", x.id, x.app), (x.target.clone(), x.blocks_tag.clone())))
        .collect()
}

/// The trailer key that approves a move of a date or of a ratchet's target.
const GATES_DATE: &str = "Gates-date";

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
    let mut prev_state: Option<BTreeMap<String, (String, String)>> = None;
    for v in versions {
        let (r, _) = parse(&v.text);
        let now = dated(&r);
        let now_state = ratchet_state(&r);
        if let Some(before) = &prev_state {
            for (k, (target, blocks)) in &now_state {
                let Some((old_target, old_blocks)) = before.get(k) else { continue };
                let mut wants = Vec::new();
                if old_target != target {
                    wants.push((
                        format!("moved the target of [[ratchet]] {k} from {old_target} to {target}"),
                        format!("{k} {old_target} -> {target}"),
                    ));
                }
                if old_blocks != blocks && blocks == REPORT_ONLY {
                    let was = if old_blocks.is_empty() { "blocking" } else { old_blocks.as_str() };
                    wants.push((
                        format!("made [[ratchet]] {k} report-only (it was {was})"),
                        format!("{k} blocks_tag {was} -> {REPORT_ONLY}"),
                    ));
                }
                for (what, move_) in wants {
                    // The user alone approves a ratchet's move; the one judge is `paddock_approvals` (user decision 125).
                    let line = format!("{GATES_DATE}: {move_}; approved: user");
                    let verdict = date_move_claim(&v.trailers, GATES_DATE, &move_).verdict("approved", Approver::User, line.clone());
                    if verdict != Verdict::Approved {
                        out.push(Problem {
                            message: format!("commit {} {what} without a `{line}` trailer", short(&v.sha)),
                            fix: "Make the change in a commit whose message carries the trailer (approved by the user), or put it back."
                                .into(),
                        });
                    }
                }
            }
        }
        prev_state = Some(now_state);
        if let Some(before) = &prev {
            for (k, new) in &now {
                if let Some(old) = before.get(k).filter(|o| *o != new) {
                    let id = k.split(" [").next().unwrap_or(k);
                    let move_ = format!("{id} {old} -> {new}");
                    let n = moves.entry(k.clone()).or_insert(0);
                    *n += 1;
                    // The first move takes the coordinator's or the user's approval, a second move of the entry the user's.
                    let needs = if *n >= 2 { Approver::User } else { Approver::Coordinator };
                    let line = format!("{GATES_DATE}: {move_}; approved: {}", needs.as_str());
                    match date_move_claim(&v.trailers, GATES_DATE, &move_).verdict("approved", needs, line) {
                        Verdict::Approved => {}
                        Verdict::Mismatch { why } if *n >= 2 => out.push(Problem {
                            message: format!(
                                "commit {} moved review_by of {k} a second time and needs `approved: user` ({why})",
                                short(&v.sha)
                            ),
                            fix: "A second move of the same entry is approved by the user.".into(),
                        }),
                        _ => out.push(Problem {
                            message: format!(
                                "commit {} moved review_by of {k} from {old} to {new} without a `{GATES_DATE}: {move_}; approved: <coordinator|user>` trailer",
                                short(&v.sha)
                            ),
                            fix: "Make the move in a commit whose message carries the trailer, or put the old date back.".into(),
                        }),
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
            let before_paths: BTreeSet<&String> = b.clippy_not_yet.iter().collect();
            for p in rt.clippy_not_yet.iter().filter(|p| !before_paths.contains(p)) {
                out.push(Problem {
                    message: format!(
                        "[[ratchet]] {} {}: clippy path `{p}` was added to clippy_not_yet (the list may only shrink)",
                        rt.id, rt.app
                    ),
                    fix: "Add the path to the app's clippy.toml and justify its call sites instead.".into(),
                });
            }
        }
    }
    if let Some(tag) = release_tag {
        out.extend(ratchet_release_problems(r, tag));
    }
    out
}

/// Splits a release tag `debrief-v1.2.0-rc.1` into the app and the numeric core of its version (`[1, 2, 0]`); `None` when it has no
/// `<app>-v<numbers>` shape. A pre-release suffix (`-rc.1`, `+build`) is dropped: a pre-release of a release counts as that release.
fn parse_tag(tag: &str) -> Option<(String, Vec<u64>)> {
    let (app, v) = tag.rsplit_once("-v")?;
    let core = v.split(['-', '+']).next()?;
    let nums = core.split('.').map(|p| p.parse().ok()).collect::<Option<Vec<u64>>>()?;
    (!app.is_empty() && !nums.is_empty()).then(|| (app.to_string(), nums))
}

/// The problems a release tag has with the ratchets of its app: a tag of the target release or a later one is refused while a list
/// is not empty, and a tag or target that cannot be read is refused (fail closed). Other apps' ratchets are not looked at.
fn ratchet_release_problems(r: &Register, tag: &str) -> Vec<Problem> {
    ratchet_release_findings(r, tag)
        .into_iter()
        .filter_map(|(p, report_only)| (!report_only).then_some(p))
        .collect()
}

/// The findings a release tag has with the open lists of its app's ratchets, each with whether its ratchet is report-only (listed,
/// never refusing the tag). Unreadable tags and targets are always blocking.
fn ratchet_release_findings(r: &Register, tag: &str) -> Vec<(Problem, bool)> {
    let mut out = Vec::new();
    let Some((app, version)) = parse_tag(tag) else {
        return vec![(
            Problem {
                message: format!(
                    "tag {tag} is refused: it is not of the form <app>-v<major>.<minor>.<patch>, so it cannot be compared with a ratchet's target"
                ),
                fix: "Tag as <app>-vX.Y.Z (a pre-release suffix such as -rc.1 is allowed).".into(),
            },
            false,
        )];
    };
    for rt in r.ratchet.iter().filter(|rt| rt.app == app) {
        let Some((_, target)) = parse_tag(&rt.target) else {
            out.push((
                Problem {
                    message: format!(
                        "tag {tag} is refused: [[ratchet]] {} {} has a target `{}` that is not <app>-vX.Y.Z",
                        rt.id, rt.app, rt.target
                    ),
                    fix: "Fix the target in docs/gates.toml.".into(),
                },
                false,
            ));
            continue;
        };
        let open = !rt.not_yet.is_empty() || !rt.clippy_not_yet.is_empty();
        if open && version >= target {
            let mut left = rt.not_yet.clone();
            left.extend(rt.clippy_not_yet.iter().map(|p| format!("clippy path {p}")));
            let report_only = rt.blocks_tag == REPORT_ONLY;
            out.push((
                Problem {
                    message: format!(
                        "tag {tag} {}: [[ratchet]] {} {} still lists {} entries (target {}): {}",
                        if report_only {
                            "is not refused (report-only)"
                        } else {
                            "is refused"
                        },
                        rt.id,
                        rt.app,
                        left.len(),
                        rt.target,
                        left.join(", ")
                    ),
                    fix: "Opt the remaining members in (each clippy-clean under the preset) and clean up the listed clippy paths.".into(),
                },
                report_only,
            ));
        }
    }
    out
}

/// The open lists of the tag's app's report-only ratchets, as messages that do not refuse the tag (ADR 0060 amendment 2026-11-12).
pub fn release_reports(register: Option<&str>, tag: &str) -> Vec<String> {
    let Some(text) = register else { return Vec::new() };
    let (reg, errors) = parse(text);
    if !errors.is_empty() {
        return Vec::new();
    }
    ratchet_release_findings(&reg, tag)
        .into_iter()
        .filter_map(|(p, report_only)| report_only.then(|| format!("{} Fix: {}", p.message, p.fix)))
        .collect()
}

/// Whether the release tag is refused, from the register text and the register at the base (ADR 0060 3.2): by the ratchets of the
/// tag's app, and by a `[[report_only]]` entry past its expiry date that concerns the app (the sunset rule; ER-41: a time condition
/// blocks tags). Another expired entry, another app's gap or a docs finding never refuses a release (ER-31). Returns the reasons it
/// is refused; empty means the tag may be made.
pub fn release_refusals(register: Option<&str>, base: Option<&str>, tag: &str, today: Date) -> Vec<String> {
    let line = |p: Problem| format!("{} Fix: {}", p.message, p.fix);
    let Some(text) = register else {
        return vec![format!(
            "tag {tag} is refused: docs/gates.toml could not be read. Fix: restore the register."
        )];
    };
    let (reg, errors) = parse(text);
    if !errors.is_empty() {
        return errors
            .into_iter()
            .map(|e| format!("tag {tag} is refused: the register does not parse: {e}. Fix: repair docs/gates.toml."))
            .collect();
    }
    let app = parse_tag(tag).map(|(a, _)| a);
    let mut sunset_refusals: Vec<String> = sunset(&reg, today)
        .into_iter()
        .filter(|d| d.overdue && d.concerns(app.as_deref()))
        .map(|d| format!("tag {tag} is refused: {} Fix: {}", d.message(), d.fix()))
        .collect();
    let Some(app) = app else {
        sunset_refusals.extend(ratchet_release_problems(&reg, tag).into_iter().map(line));
        return sunset_refusals;
    };
    let mine = Register {
        ratchet: reg.ratchet.iter().filter(|r| r.app == app).cloned().collect(),
        ..Register::default()
    };
    if mine.ratchet.is_empty() {
        return sunset_refusals;
    }
    let Some(base_text) = base else {
        sunset_refusals.push(format!(
            "tag {tag} is refused: not measured, the register at the base revision could not be read, so a ratchet list cannot be compared. Fix: fetch the full history (fetch-depth: 0)."
        ));
        return sunset_refusals;
    };
    let base_reg = parse(base_text).0;
    let mut out = check_ratchets(&mine, Some(&base_reg), None);
    out.extend(ratchet_release_problems(&mine, tag));
    sunset_refusals.extend(out.into_iter().map(line));
    sunset_refusals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> Date {
        Date::parse("2026-11-20").unwrap()
    }

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
        let bad = GOOD.replace("owner = \"ADR 0060 WP-4\"\n", "");
        let (_, errs) = parse(&bad);
        assert!(errs.iter().any(|e| e.contains("lacks owner")), "{errs:?}");
        // The approvals are not this reader's: a register that lacks `approved` still parses whole (`paddock_approvals` judges it).
        let (r, errs) = parse(
            &GOOD
                .replace("approved = \"user\"\n", "")
                .replace("approved = \"coordinator\"\n", ""),
        );
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!((r.not_gated.len(), r.ignored.len(), r.ratchet.len()), (1, 1, 1));
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

    fn report_only(id: &str, apps: &str, since: &str, review_by: &str) -> String {
        format!(
            "version = 1\n\n[[report_only]]\nid = \"{id}\"\n{apps}reason = \"r\"\nowner = \"o\"\nsince = {since}\nreview_by = {review_by}\napproved = \"user\"\n"
        )
    }

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    /// The sunset rule's limit, with its positive control: a new entry may stay report-only 14 days, one day more is a problem; an
    /// entry made before the rule keeps the 60-day limit; a new entry cannot dodge it by backdating `since`.
    #[test]
    fn a_report_only_entry_made_after_the_sunset_start_expires_within_14_days() {
        let reg = |since: &str, by: &str| parse(&report_only("C900", "", since, by)).0;
        let fourteen = reg("2026-11-25", "2026-12-09");
        assert!(sunset_limits(&fourteen, None, d("2026-11-25")).is_empty());
        assert!(check_dates(&fourteen, d("2026-11-25")).is_empty());
        let fifteen = reg("2026-11-25", "2026-12-10");
        let p = sunset_limits(&fifteen, None, d("2026-11-25"));
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].message.contains("more than 14 days after since 2026-11-25"), "{p:?}");
        assert!(p[0].fix.contains("2026-12-09"), "the fix names the last allowed day: {p:?}");
        // made before the rule: only the 60-day limit of every entry
        assert!(sunset_limits(&reg("2026-11-24", "2027-01-20"), None, d("2026-11-25")).is_empty());
        // new on this branch (the base lacks it) with a backdated since: held to today + 14
        let backdated = reg("2026-11-20", "2027-01-10");
        let base = parse("version = 1\n").0;
        let p = sunset_limits(&backdated, Some(&base), d("2026-11-26"));
        assert!(p.iter().any(|x| x.message.contains("is new")), "{p:?}");
        assert!(
            sunset_limits(&backdated, Some(&backdated), d("2026-11-26")).is_empty(),
            "an entry the base has is not new"
        );
    }

    /// The sunset rule's dates: within a week of the expiry date is a reminder, after it overdue; a passed report-only date is the
    /// sunset rule's, not a plain problem (ER-41: it must not block every push); other kinds keep theirs.
    #[test]
    fn an_expired_report_only_entry_is_due_not_a_plain_problem() {
        let r = parse(&report_only("C900", "", "2026-11-25", "2026-12-08")).0;
        assert!(sunset(&r, d("2026-11-30")).is_empty(), "eight days before: nothing");
        let near = sunset(&r, d("2026-12-01"));
        assert_eq!(near.len(), 1);
        assert!(!near[0].overdue && near[0].message().contains("is near"), "{near:?}");
        assert!(!sunset(&r, d("2026-12-08"))[0].overdue, "the expiry date is the last day");
        let late = sunset(&r, d("2026-12-09"));
        assert!(
            late[0].overdue && late[0].message().contains("review_by 2026-12-08 has passed"),
            "{late:?}"
        );
        assert!(check_dates(&r, d("2026-12-09")).is_empty(), "not a plain problem");
        let ng = parse(&report_only("C900", "", "2026-11-25", "2026-12-08").replace("[[report_only]]", "[[not_gated]]")).0;
        assert!(check_dates(&ng, d("2026-12-09")).iter().any(|p| p.message.contains("has passed")));
    }

    /// A tag is refused by an overdue report-only entry that concerns its app, and by nothing else of the rule (positive control: the
    /// same register a day earlier lets it through).
    #[test]
    fn an_overdue_report_only_entry_refuses_a_tag_of_an_app_it_concerns() {
        let all = report_only("C900", "", "2026-11-25", "2026-12-08");
        assert!(release_refusals(Some(&all), Some(&all), "dyno-v1.2.0", d("2026-12-08")).is_empty());
        let r = release_refusals(Some(&all), Some(&all), "dyno-v1.2.0", d("2026-12-09"));
        assert_eq!(r.len(), 1, "{r:?}");
        assert!(
            r[0].starts_with("tag dyno-v1.2.0 is refused: [[report_only]] C900: review_by 2026-12-08 has passed"),
            "{r:?}"
        );
        let dyno = report_only("C900", "apps = [\"dyno\"]\n", "2026-11-25", "2026-12-08");
        assert!(release_refusals(Some(&dyno), Some(&dyno), "pitwall-v1.1.0", d("2026-12-09")).is_empty());
        assert_eq!(release_refusals(Some(&dyno), Some(&dyno), "dyno-v1.2.0", d("2026-12-09")).len(), 1);
    }

    /// A version whose commit message is `message`: its trailers are the `Gates-date:` lines after the subject, as git's own parser
    /// gives them for these simple messages (the real parser, prose and subject included, is exercised in `edge.rs`).
    fn version(sha: &str, message: &str, text: &str) -> Version {
        Version {
            sha: sha.into(),
            trailers: message
                .lines()
                .skip(1)
                .filter(|l| l.starts_with("Gates-date: "))
                .collect::<Vec<_>>()
                .join("\n"),
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
        let with_path = GOOD.replace("not_yet = [", "clippy_not_yet = [\"std::env::temp_dir\"]\nnot_yet = [");
        let (base_c, _) = parse(&with_path);
        let more = with_path.replace("[\"std::env::temp_dir\"]", "[\"std::env::temp_dir\", \"reqwest::Client::new\"]");
        assert!(
            check_ratchets(&parse(&more).0, Some(&base_c), None)[0]
                .message
                .contains("clippy path `reqwest::Client::new` was added")
        );
        assert!(check_ratchets(&base_c, Some(&base_c), None).is_empty());
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
        // the clippy list alone also keeps the tag refused
        let clippy_only = GOOD.replace(
            "not_yet = [\"xtask\", \"examples/x\"]",
            "not_yet = []\nclippy_not_yet = [\"std::env::temp_dir\"]",
        );
        assert!(!check_ratchets(&parse(&clippy_only).0, None, Some("debrief-v1.3.0")).is_empty());
        let empty = GOOD.replace("[\"xtask\", \"examples/x\"]", "[]");
        assert!(check_ratchets(&parse(&empty).0, None, Some("debrief-v1.3.0")).is_empty());
    }

    const RATCHET: &str = r#"
version = 1
[[not_gated]]
id = "C99"
reason = "r"
owner = "o"
since = 2026-01-01
review_by = 2026-02-01
approved = "coordinator"

[[ratchet]]
id = "C13"
app = "debrief"
not_yet = ["xtask", "crates/a"]
clippy_not_yet = ["std::env::temp_dir"]
owner = "Debrief 1.2"
target = "debrief-v1.2.0"
approved = "user"
"#;

    #[test]
    fn a_tag_is_refused_only_by_the_ratchets_of_its_app() {
        // an unrelated entry that expired long ago does not refuse a patch tag below the target
        assert!(release_refusals(Some(RATCHET), Some(RATCHET), "debrief-v1.1.5", today()).is_empty());
        // another app's tag is not looked at
        assert!(release_refusals(Some(RATCHET), None, "dyno-v9.0.0", today()).is_empty());
        // the target release and later ones are refused while a list is not empty, naming what is left
        for tag in ["debrief-v1.2.0", "debrief-v1.2.0-rc.1", "debrief-v1.2.1", "debrief-v2.0.0"] {
            let r = release_refusals(Some(RATCHET), Some(RATCHET), tag, today());
            assert_eq!(r.len(), 1, "{tag}: {r:?}");
            assert!(
                r[0].contains("xtask") && r[0].contains("crates/a") && r[0].contains("clippy path std::env::temp_dir"),
                "{r:?}"
            );
        }
        // good: both lists empty
        let empty = RATCHET
            .replace("not_yet = [\"xtask\", \"crates/a\"]", "not_yet = []")
            .replace("clippy_not_yet = [\"std::env::temp_dir\"]", "clippy_not_yet = []");
        assert!(release_refusals(Some(&empty), Some(RATCHET), "debrief-v1.2.0", today()).is_empty());
    }

    #[test]
    fn a_report_only_ratchet_lists_its_open_lists_and_never_refuses_the_tag() {
        let report_only = RATCHET.replace(
            "target = \"debrief-v1.2.0\"\n",
            "target = \"debrief-v1.2.0\"\nblocks_tag = \"report-only\"\n",
        );
        for tag in ["debrief-v1.2.0", "debrief-v1.2.1", "debrief-v2.0.0"] {
            assert!(
                release_refusals(Some(&report_only), Some(&report_only), tag, today()).is_empty(),
                "{tag}"
            );
            let rep = release_reports(Some(&report_only), tag);
            assert_eq!(rep.len(), 1, "{tag}: {rep:?}");
            assert!(rep[0].contains("report-only") && rep[0].contains("xtask"), "{rep:?}");
        }
        // below the target nothing is reported; without the value the tag is still refused and nothing is only reported
        assert!(release_reports(Some(&report_only), "debrief-v1.1.5").is_empty());
        assert!(!release_refusals(Some(RATCHET), Some(RATCHET), "debrief-v1.2.0", today()).is_empty());
        assert!(release_reports(Some(RATCHET), "debrief-v1.2.0").is_empty());
    }

    #[test]
    fn a_ratchet_target_moves_with_the_trailer_the_adr_names_and_not_without_it() {
        let moved = RATCHET.replace("debrief-v1.2.0", "debrief-v1.3.0");
        let bad = date_history(&[version("a1", "feat: add", RATCHET), version("b2", "fix: later", &moved)]);
        assert!(
            bad.iter().any(|p| p.message.contains("target of [[ratchet]] C13 debrief")
                && p.message
                    .contains("without a `Gates-date: C13 debrief debrief-v1.2.0 -> debrief-v1.3.0; approved: user`")),
            "{bad:?}"
        );
        let ok = date_history(&[
            version("a1", "feat: add", RATCHET),
            version(
                "b2",
                "fix: later\n\nGates-date: C13 debrief debrief-v1.2.0 -> debrief-v1.3.0; approved: user",
                &moved,
            ),
        ]);
        assert!(ok.is_empty(), "{ok:?}");
        // the coordinator cannot approve a target move
        let coord = date_history(&[
            version("a1", "feat: add", RATCHET),
            version(
                "b2",
                "x\n\nGates-date: C13 debrief debrief-v1.2.0 -> debrief-v1.3.0; approved: coordinator",
                &moved,
            ),
        ]);
        assert!(!coord.is_empty());
        // the base comparison alone no longer rejects the move
        let (base, _) = parse(RATCHET);
        assert!(check_ratchets(&parse(&moved).0, Some(&base), None).is_empty());
    }

    #[test]
    fn making_a_ratchet_report_only_needs_the_users_trailer() {
        let report_only = RATCHET.replace(
            "target = \"debrief-v1.2.0\"\n",
            "target = \"debrief-v1.2.0\"\nblocks_tag = \"report-only\"\n",
        );
        let bad = date_history(&[version("a1", "feat: add", RATCHET), version("b2", "fix: defer", &report_only)]);
        assert!(
            bad.iter().any(|p| p.message.contains("made [[ratchet]] C13 debrief report-only")),
            "{bad:?}"
        );
        let ok = date_history(&[
            version("a1", "feat: add", RATCHET),
            version(
                "b2",
                "fix: defer\n\nGates-date: C13 debrief blocks_tag blocking -> report-only; approved: user",
                &report_only,
            ),
        ]);
        assert!(ok.is_empty(), "{ok:?}");
        // reporting that a ratchet blocks again needs nothing
        let back = date_history(&[version("a1", "feat: add", &report_only), version("b2", "fix: undo", RATCHET)]);
        assert!(back.is_empty(), "{back:?}");
    }

    #[test]
    fn a_trailer_counts_only_with_its_exact_move_and_a_whole_approver_word() {
        let moved = RATCHET.replace("debrief-v1.2.0", "debrief-v1.3.0");
        let t = "Gates-date: C13 debrief debrief-v1.2.0 -> debrief-v1.3.0; approved: user";
        let flagged = |msg: &str| !date_history(&[version("a1", "feat: add", RATCHET), version("b2", msg, &moved)]).is_empty();
        assert!(!flagged(&format!("fix: move\n\nwhy\n\n{t}")));
        assert!(flagged(&format!("fix: move\n\n{t} and more")), "text after the approver");
        assert!(flagged(&format!("fix: move\n\n{t}x")), "text after the approver");
        // another app's ratchet of the same id does not share the trailer
        let other = t.replace("C13 debrief ", "C13 dyno ");
        assert!(flagged(&format!("fix: move\n\n{other}")));
        // the review_by trailer is exact as well
        let a = GOOD.replacen("review_by = 2026-12-06", "review_by = 2026-12-20", 1);
        let line = "Gates-date: C13 2026-12-06 -> 2026-12-20; approved: coordinator";
        assert!(date_history(&[version("a1", "x", GOOD), version("b2", &format!("x\n\n{line}"), &a)]).is_empty());
        assert!(!date_history(&[version("a1", "x", GOOD), version("b2", &format!("x\n\n{line} maybe"), &a)]).is_empty());
        assert!(
            !date_history(&[
                version("a1", "x", GOOD),
                version("b2", "x\n\nGates-date: C13 2026-12-06 -> 2026-12-20; approved: nobody", &a)
            ])
            .is_empty()
        );
    }

    #[test]
    fn a_blocks_tag_typo_is_a_register_error() {
        let typo = RATCHET.replace(
            "target = \"debrief-v1.2.0\"\n",
            "target = \"debrief-v1.2.0\"\nblocks_tag = \"report_only\"\n",
        );
        let (_, errs) = parse(&typo);
        assert!(
            errs.iter().any(|e| e.contains("blocks_tag") && e.contains("report_only")),
            "{errs:?}"
        );
        let good = RATCHET.replace(
            "target = \"debrief-v1.2.0\"\n",
            "target = \"debrief-v1.2.0\"\nblocks_tag = \"report-only\"\n",
        );
        assert!(parse(&good).1.is_empty());
    }

    #[test]
    fn a_broken_ratchet_is_named_with_its_app() {
        let broken = RATCHET.replace("target = \"debrief-v1.2.0\"\n", "");
        let r = release_refusals(Some(&broken), Some(RATCHET), "debrief-v1.1.5", today());
        assert!(
            r[0].contains("[[ratchet]] #1 (C13 debrief)") && r[0].contains("does not parse"),
            "{r:?}"
        );
    }

    #[test]
    fn what_cannot_be_read_refuses_the_tag() {
        assert!(release_refusals(None, Some(RATCHET), "debrief-v1.1.5", today())[0].contains("could not be read"));
        assert!(release_refusals(Some("not = toml = at all"), Some(RATCHET), "debrief-v1.1.5", today())[0].contains("does not parse"));
        // a ratchet of the app and no base: not measured
        assert!(release_refusals(Some(RATCHET), None, "debrief-v1.1.5", today())[0].contains("not measured"));
        // a tag that is not <app>-v<numbers>, or whose version is not numeric
        for tag in ["v1.2.0", "debrief-vx.y", "debrief"] {
            assert!(
                release_refusals(Some(RATCHET), Some(RATCHET), tag, today())[0].contains("not of the form"),
                "{tag}"
            );
        }
        // growth against the base refuses too
        let grown = RATCHET.replace("\"crates/a\"]", "\"crates/a\", \"crates/new\"]");
        assert!(release_refusals(Some(&grown), Some(RATCHET), "debrief-v1.1.5", today())[0].contains("was added to not_yet"));
    }

    #[test]
    fn the_committed_register_holds_debriefs_ratchet() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/gates.toml");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let (r, errors) = parse(&text);
        assert!(errors.is_empty(), "{errors:?}");
        let rt = r
            .ratchet
            .iter()
            .find(|x| x.id == "C13" && x.app == "debrief")
            .expect("the register has Debrief's C13 [[ratchet]]");
        assert_eq!(rt.target, "debrief-v1.2.0");
        // approved by the user; the lists only shrink, so they hold at most the 46 members and the two paths of 2026-11-09
        assert!(rt.not_yet.len() <= 46 && rt.clippy_not_yet.len() <= 2);
        // Debrief runs C13 in debrief.yml, so it must not also be waived by a [[not_gated]]
        assert!(
            r.not_gated
                .iter()
                .filter(|e| e.id == "C13")
                .all(|e| !e.apps.iter().any(|a| a == "debrief"))
        );
    }
}
