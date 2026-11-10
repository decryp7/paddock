//! `docs/optional-measurements.toml` (ADR 0089 section 3): the reviewed list of measurements a place may skip, and the facts of
//! a run that decide whether an entry covers it. Pure: the text and the facts are passed in.

use super::Date;
use std::collections::HashSet;

/// What kind of run this is. A release run honours an entry only under ADR 0089 D14.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    /// Any run that is not a release run.
    Ordinary,
    /// A tag or `release/**` run, or a workflow that set `PADDOCK_RUN_KIND=release`.
    Release,
}

impl RunKind {
    /// The kind from the two facts the environment gives: `GITHUB_REF` and `PADDOCK_RUN_KIND`. The variable is empty (or
    /// unset) or exactly `release`; anything else (`relase`, `Release`, ` release`) is an error, because a misspelt variable
    /// would otherwise make a release run look ordinary and let an optional entry excuse a missing measurement.
    pub fn detect(github_ref: Option<&str>, run_kind_var: Option<&str>) -> Result<RunKind, String> {
        let by_ref = github_ref
            .is_some_and(|r| r.starts_with("refs/tags/") || r.starts_with("refs/heads/release/"));
        match run_kind_var {
            None | Some("") => Ok(if by_ref {
                RunKind::Release
            } else {
                RunKind::Ordinary
            }),
            Some("release") => Ok(RunKind::Release),
            Some(other) => Err(format!(
                "PADDOCK_RUN_KIND=\"{other}\" is not a run kind: leave it empty or write exactly release"
            )),
        }
    }
}

/// How much of the tree the run looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The whole tree (or the whole app): nothing is "not applicable".
    Root,
    /// `--files` or an app scope: a check that reads nothing in it has nothing to do.
    Scoped,
}

/// The facts of one run: its kind and scope, where it ran, today's date (`YYYY-MM-DD`) and which legs ran which test green.
#[derive(Debug, Clone)]
pub struct Run {
    /// Ordinary or release.
    pub kind: RunKind,
    /// Whole tree or scoped.
    pub scope: Scope,
    /// The CI leg (`PADDOCK_LEG`), when known.
    pub leg: Option<String>,
    /// The operating system (`std::env::consts::OS`).
    pub os: String,
    /// Today, for expiry.
    pub today: Date,
    green: Vec<(String, String)>,
}

impl Run {
    /// A run on `os` today; add the leg and green legs with the builders.
    pub fn new(kind: RunKind, scope: Scope, os: impl Into<String>, today: Date) -> Run {
        Run {
            kind,
            scope,
            leg: None,
            os: os.into(),
            today,
            green: Vec::new(),
        }
    }

    /// The leg this run is.
    pub fn on_leg(mut self, leg: impl Into<String>) -> Run {
        self.leg = Some(leg.into());
        self
    }

    /// Records that `leg` ran the measurement `id` green in this run (what a release run's final job reads from the legs' reports).
    pub fn with_green(mut self, id: impl Into<String>, leg: impl Into<String>) -> Run {
        self.green.push((id.into(), leg.into()));
        self
    }

    fn green_on(&self, id: &str, legs: &[String]) -> bool {
        self.green.iter().any(|(i, l)| i == id && legs.contains(l))
    }
}

/// One `[[optional]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The measurement it covers, `*` matching any text (`test:debrief-video::real_ffmpeg::*`).
    pub id: String,
    /// Legs it applies to; empty means every leg.
    pub legs: Vec<String>,
    /// Operating systems it applies to; empty means every OS.
    pub os: Vec<String>,
    /// Why, with a measurement date.
    pub reason: String,
    /// Last day it counts, `YYYY-MM-DD`.
    pub expires: Date,
    /// Legs that must have run the same measurement green for a release run to honour the entry (D14).
    pub measured_on: Vec<String>,
}

impl Entry {
    fn matches_id(&self, id: &str) -> bool {
        glob(&self.id, id)
    }

    fn applies_here(&self, run: &Run) -> bool {
        let leg_ok =
            self.legs.is_empty() || run.leg.as_ref().is_some_and(|l| self.legs.contains(l));
        let os_ok = self.os.is_empty() || self.os.contains(&run.os);
        leg_ok && os_ok
    }

    /// Whether this entry covers a not-measured `id` in `run`: same id, live, same leg and OS, and in a release run measured elsewhere.
    pub(crate) fn covers(&self, id: &str, run: &Run) -> bool {
        self.matches_id(id)
            && self.expires >= run.today
            && self.applies_here(run)
            && (run.kind == RunKind::Ordinary
                || (!self.measured_on.is_empty() && run.green_on(id, &self.measured_on)))
    }
}

/// The reviewed optional measurements. Never makes a `CheckBroken` optional: [`super::Verdict`] does not ask.
#[derive(Debug, Clone, Default)]
pub struct Optional {
    entries: Vec<Entry>,
}

impl Optional {
    /// No entries: every missing measurement counts.
    pub fn none() -> Optional {
        Optional::default()
    }

    /// The entries.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Entries past their date today: each is an error (as the conformance waiver's W001).
    pub fn expired(&self, today: Date) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.expires < today).collect()
    }

    /// The first live entry that covers `id` in `run`.
    pub(crate) fn covering(&self, id: &str, run: &Run) -> Option<&Entry> {
        self.entries.iter().find(|e| e.covers(id, run))
    }

    /// Reads the file: `[[optional]]` tables with `id` (a kind prefix `test:`, `check:`, `gate:` or `probe:` and something
    /// narrower than `*`), `reason`, `expires` (a real `YYYY-MM-DD`), `where = { leg = [..], os = [..] }` (required, at least
    /// one non-empty list) and `measured_on = [..]`. This is the whole subset; anything else, a duplicate key and an entry
    /// that would excuse every place are errors naming the line, so a typo never silently drops or widens an entry.
    pub fn parse(text: &str) -> Result<Optional, String> {
        let mut entries: Vec<Entry> = Vec::new();
        let mut cur: Option<(usize, Entry, HashSet<String>)> = None;
        for (i, raw) in text.lines().enumerate() {
            let n = i + 1;
            let line = strip_comment(raw).trim().to_string();
            if line.is_empty() {
                continue;
            }
            if line == "[[optional]]" {
                if let Some((start, e, seen)) = cur.take() {
                    entries.push(finish(start, e, &seen)?);
                }
                let blank = Entry {
                    id: String::new(),
                    legs: vec![],
                    os: vec![],
                    reason: String::new(),
                    expires: Date::parse("1970-01-01")?,
                    measured_on: vec![],
                };
                cur = Some((n, blank, HashSet::new()));
                continue;
            }
            let Some((_, e, seen)) = cur.as_mut() else {
                return Err(format!(
                    "line {n}: \"{line}\" is outside an [[optional]] table"
                ));
            };
            let (key, value) = line
                .split_once('=')
                .map(|(k, v)| (k.trim(), v.trim()))
                .ok_or_else(|| {
                    format!("line {n}: cannot read \"{line}\" (expected key = value)")
                })?;
            let mut once = |name: &str| -> Result<(), String> {
                if seen.insert(name.to_string()) {
                    Ok(())
                } else {
                    Err(format!(
                        "line {n}: the key \"{name}\" appears twice in one entry"
                    ))
                }
            };
            match key {
                "id" => {
                    once("id")?;
                    e.id = string(value, n)?;
                }
                "reason" => {
                    once("reason")?;
                    e.reason = string(value, n)?;
                }
                "expires" => {
                    once("expires")?;
                    e.expires =
                        Date::parse(&string(value, n)?).map_err(|m| format!("line {n}: {m}"))?;
                }
                "measured_on" => {
                    once("measured_on")?;
                    e.measured_on = array(value, n)?;
                }
                "where" => {
                    once("where")?;
                    let inner = value
                        .strip_prefix('{')
                        .and_then(|v| v.strip_suffix('}'))
                        .ok_or_else(|| format!("line {n}: where must be an inline table {{ leg = [..], os = [..] }}"))?;
                    for part in split_top(inner) {
                        let (k, v) = part
                            .split_once('=')
                            .map(|(k, v)| (k.trim(), v.trim()))
                            .ok_or_else(|| format!("line {n}: cannot read \"{part}\" in where"))?;
                        let list = array(v, n)?;
                        if list.is_empty() {
                            return Err(format!(
                                "line {n}: where.{k} is an empty list, which would mean every {k}; name them or leave the key out"
                            ));
                        }
                        match k {
                            "leg" => once("where.leg").map(|_| e.legs = list)?,
                            "os" => once("where.os").map(|_| e.os = list)?,
                            other => {
                                return Err(format!(
                                    "line {n}: unknown key \"{other}\" in where (leg and os only)"
                                ));
                            }
                        }
                    }
                }
                other => return Err(format!("line {n}: unknown key \"{other}\"")),
            }
        }
        if let Some((start, e, seen)) = cur.take() {
            entries.push(finish(start, e, &seen)?);
        }
        Ok(Optional { entries })
    }
}

/// The kinds of measurement an entry can name.
const KINDS: [&str; 4] = ["test:", "check:", "gate:", "probe:"];

fn finish(start: usize, e: Entry, seen: &HashSet<String>) -> Result<Entry, String> {
    let need = |what: &str| format!("the [[optional]] at line {start} needs {what}");
    let rest = KINDS.iter().find_map(|k| e.id.strip_prefix(k));
    match rest {
        None => {
            return Err(need(
                "an id that starts with test:, check:, gate: or probe: (a bare * would excuse everything)",
            ));
        }
        Some(r) if r.trim_matches('*').is_empty() => {
            return Err(need(
                "an id narrower than the whole kind (test:* would excuse every test)",
            ));
        }
        Some(_) => {}
    }
    if e.reason.trim().is_empty() {
        return Err(need("a reason"));
    }
    if !seen.contains("expires") {
        return Err(need("expires = \"YYYY-MM-DD\""));
    }
    if e.legs.is_empty() && e.os.is_empty() {
        return Err(need(
            "where = { leg = [..] and/or os = [..] }: an entry with no place would excuse every leg and OS",
        ));
    }
    Ok(e)
}

fn strip_comment(raw: &str) -> &str {
    let mut in_str = false;
    for (i, c) in raw.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &raw[..i],
            _ => {}
        }
    }
    raw
}

fn string(v: &str, n: usize) -> Result<String, String> {
    v.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .map(str::to_string)
        .ok_or_else(|| format!("line {n}: expected a quoted string, found {v}"))
}

fn array(v: &str, n: usize) -> Result<Vec<String>, String> {
    let inner = v
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| format!("line {n}: expected an array of strings, found {v}"))?;
    split_top(inner)
        .into_iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| string(p.trim(), n))
        .collect()
}

/// Splits at commas outside brackets and quotes.
fn split_top(s: &str) -> Vec<&str> {
    let (mut parts, mut depth, mut in_str, mut from) = (Vec::new(), 0i32, false, 0);
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '[' if !in_str => depth += 1,
            ']' if !in_str => depth -= 1,
            ',' if !in_str && depth == 0 => {
                parts.push(&s[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[from..]);
    parts
}

/// Matches `text` against `pattern`, where `*` stands for any run of characters.
fn glob(pattern: &str, text: &str) -> bool {
    let pieces: Vec<&str> = pattern.split('*').collect();
    if pieces.len() == 1 {
        return pattern == text;
    }
    let (first, last) = (pieces[0], pieces[pieces.len() - 1]);
    if !text.starts_with(first) || text.len() < first.len() + last.len() || !text.ends_with(last) {
        return false;
    }
    let mut at = first.len();
    let end = text.len() - last.len();
    for mid in &pieces[1..pieces.len() - 1] {
        match text[at..end].find(mid) {
            Some(p) => at += p + mid.len(),
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"
# a comment
[[optional]]
id = "test:debrief-video::real_ffmpeg::*"    # trailing comment
where = { leg = ["linux-container"], os = ["linux"] }
reason = "paddock-ci:v2 has no ffmpeg (measured 2026-11-10); the image adds it in WP-5"
expires = "2027-01-31"
measured_on = ["linux-host", "macos"]

[[optional]]
id = "check:C99/graph"
where = { os = ["linux"] }
reason = "offline"
expires = "2027-02-01"
"#;

    fn d(text: &str) -> Date {
        Date::parse(text).unwrap()
    }

    fn run(kind: RunKind) -> Run {
        Run::new(kind, Scope::Root, "linux", d("2026-11-10")).on_leg("linux-container")
    }

    #[test]
    fn reads_the_register_shape() {
        let o = Optional::parse(FILE).unwrap();
        assert_eq!(o.entries().len(), 2);
        let e = &o.entries()[0];
        assert_eq!(
            (e.legs.as_slice(), e.os.as_slice()),
            (
                &["linux-container".to_string()][..],
                &["linux".to_string()][..]
            )
        );
        assert_eq!(e.measured_on, ["linux-host", "macos"]);
        assert!(o.entries()[1].legs.is_empty());
        assert!(Optional::parse("").unwrap().entries().is_empty());
        assert!(Optional::none().entries().is_empty());
    }

    #[test]
    fn every_malformed_file_is_an_error_naming_the_line() {
        let bad = [
            "id = \"x\"\n",
            "[[optional]]\nid = x\n",
            "[[optional]]\nid\n",
            "[[optional]]\nid = \"a\"\nreason = \"r\"\nexpires = \"soon\"\n",
            "[[optional]]\nreason = \"r\"\nexpires = \"2027-01-01\"\n",
            "[[optional]]\nid = \"a\"\nexpires = \"2027-01-01\"\n",
            "[[optional]]\nid = \"a\"\nreason = \" \"\nexpires = \"2027-01-01\"\n",
            "[[optional]]\ncolour = \"red\"\n",
            "[[optional]]\nwhere = leg\n",
            "[[optional]]\nwhere = { leg ]\n",
            "[[optional]]\nwhere = { arch = [\"x\"] }\n",
            "[[optional]]\nmeasured_on = \"a\"\n",
            "[[optional]]\nmeasured_on = [a]\n",
            // The register must not excuse too much.
            "[[optional]]\nid = \"*\"\nwhere = { os = [\"linux\"] }\nreason = \"r\"\nexpires = \"2027-01-01\"\n",
            "[[optional]]\nid = \"test:*\"\nwhere = { os = [\"linux\"] }\nreason = \"r\"\nexpires = \"2027-01-01\"\n",
            "[[optional]]\nid = \"thing\"\nwhere = { os = [\"linux\"] }\nreason = \"r\"\nexpires = \"2027-01-01\"\n",
            "[[optional]]\nid = \"test:a\"\nreason = \"r\"\nexpires = \"2027-01-01\"\n",
            "[[optional]]\nid = \"test:a\"\nwhere = {}\nreason = \"r\"\nexpires = \"2027-01-01\"\n",
            "[[optional]]\nid = \"test:a\"\nwhere = { leg = [], os = [\"linux\"] }\nreason = \"r\"\nexpires = \"2027-01-01\"\n",
            "[[optional]]\nid = \"test:a\"\nwhere = { leg = [] }\nreason = \"r\"\nexpires = \"2027-01-01\"\n",
            "[[optional]]\nid = \"test:a\"\nwhere = { os = [\"linux\"], os = [\"macos\"] }\nreason = \"r\"\nexpires = \"2027-01-01\"\n",
            "[[optional]]\nid = \"test:a\"\nid = \"test:b\"\nwhere = { os = [\"linux\"] }\nreason = \"r\"\nexpires = \"2027-01-01\"\n",
            "[[optional]]\nid = \"test:a\"\nwhere = { os = [\"linux\"] }\nreason = \"r\"\nexpires = \"2027-13-99\"\n",
            "[[optional]]\nid = \"test:a\"\nwhere = { os = [\"linux\"] }\nreason = \"r\"\nexpires = \"\"\n",
        ];
        for b in bad {
            let e = Optional::parse(b).unwrap_err();
            assert!(
                e.contains("line") || e.contains("[[optional]]"),
                "{b:?}: {e}"
            );
        }
    }

    #[test]
    fn covers_by_id_leg_os_expiry_and_in_a_release_run_by_a_green_leg() {
        let o = Optional::parse(FILE).unwrap();
        let id = "test:debrief-video::real_ffmpeg::probe";
        assert!(o.covering(id, &run(RunKind::Ordinary)).is_some());
        assert!(o.covering("test:other", &run(RunKind::Ordinary)).is_none());
        // Another leg or another OS is not covered: the entry names where it applies.
        let other_leg =
            Run::new(RunKind::Ordinary, Scope::Root, "linux", d("2026-11-10")).on_leg("macos");
        assert!(o.covering(id, &other_leg).is_none());
        let no_leg = Run::new(RunKind::Ordinary, Scope::Root, "linux", d("2026-11-10"));
        assert!(o.covering(id, &no_leg).is_none());
        let other_os = Run::new(RunKind::Ordinary, Scope::Root, "windows", d("2026-11-10"))
            .on_leg("linux-container");
        assert!(o.covering(id, &other_os).is_none());
        // Expired.
        let late = Run::new(RunKind::Ordinary, Scope::Root, "linux", d("2027-02-01"))
            .on_leg("linux-container");
        assert!(o.covering(id, &late).is_none());
        assert_eq!(o.expired(d("2027-02-01")).len(), 1);
        assert!(o.expired(d("2026-11-10")).is_empty());
        // A release run needs measured_on and a green leg for the same id.
        assert!(o.covering(id, &run(RunKind::Release)).is_none());
        assert!(
            o.covering(id, &run(RunKind::Release).with_green(id, "windows"))
                .is_none()
        );
        assert!(
            o.covering(id, &run(RunKind::Release).with_green("test:other", "macos"))
                .is_none()
        );
        assert!(
            o.covering(id, &run(RunKind::Release).with_green(id, "macos"))
                .is_some()
        );
        // An entry without measured_on is never honoured in a release run.
        let rel = Run::new(RunKind::Release, Scope::Root, "linux", d("2026-11-10"))
            .with_green("check:C99/graph", "macos");
        assert!(o.covering("check:C99/graph", &rel).is_none());
    }

    #[test]
    fn run_kind_comes_from_the_ref_or_the_variable_and_a_misspelling_is_an_error() {
        let k = RunKind::detect;
        assert_eq!(k(Some("refs/tags/dyno-v1.1.0"), None), Ok(RunKind::Release));
        assert_eq!(
            k(Some("refs/heads/release/x"), Some("")),
            Ok(RunKind::Release)
        );
        assert_eq!(
            k(Some("refs/heads/main"), Some("release")),
            Ok(RunKind::Release)
        );
        assert_eq!(k(Some("refs/heads/main"), Some("")), Ok(RunKind::Ordinary));
        assert_eq!(k(None, None), Ok(RunKind::Ordinary));
        for bad in [
            "relase", "Release", " release", "release ", "RELEASE", "1", "ordinary",
        ] {
            let e = k(Some("refs/heads/main"), Some(bad)).unwrap_err();
            assert!(
                e.contains("PADDOCK_RUN_KIND") && e.contains(bad),
                "{bad:?}: {e}"
            );
            // A bad variable is an error even on a tag: nobody guesses what it was meant to say.
            assert!(k(Some("refs/tags/x"), Some(bad)).is_err());
        }
    }

    #[test]
    fn a_hash_inside_a_quoted_reason_is_kept_and_a_trailing_comment_is_cut() {
        let o = Optional::parse("[[optional]]\nid = \"test:a\"\nwhere = { os = [\"linux\"] }\nreason = \"issue #12 # still\" # comment\nexpires = \"2027-01-01\"\n").unwrap();
        assert_eq!(o.entries()[0].reason, "issue #12 # still");
    }

    #[test]
    fn an_entry_is_live_on_its_expiry_day() {
        let o = Optional::parse(FILE).unwrap();
        let id = "test:debrief-video::real_ffmpeg::probe";
        let on = |day: &str| {
            Run::new(RunKind::Ordinary, Scope::Root, "linux", d(day)).on_leg("linux-container")
        };
        assert!(o.covering(id, &on("2027-01-31")).is_some());
        assert!(o.covering(id, &on("2027-02-01")).is_none());
        assert!(o.expired(d("2027-01-31")).is_empty());
    }

    #[test]
    fn glob_matches_stars_anywhere() {
        assert!(glob("a*c", "abc") && glob("a*c", "ac") && glob("*", "") && glob("a*b*c", "aXbYc"));
        assert!(glob("exact", "exact") && !glob("exact", "exactly"));
        assert!(!glob("a*c", "ab") && !glob("a*b*c", "acb") && !glob("ab*ba", "aba"));
    }
}
