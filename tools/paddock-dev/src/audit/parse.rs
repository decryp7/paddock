//! The audit tools' output as typed findings (parse, don't validate): `cargo deny --format json check advisories` and
//! `npm audit --json`. A run that did not look (no summary, an error object, unparseable text) is [`Measure::NotMeasured`],
//! never an empty list: an empty list is the answer "looked, found nothing" (ER-33).

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// One advisory that hits one dependency of one lockfile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// The lockfile it was found in, with its tool: `cargo crates/Cargo.lock`, `npm package-lock.json`.
    pub source: String,
    /// The advisory: `RUSTSEC-...` or `GHSA-...` (a yanked crate: `yanked`).
    pub id: String,
    /// The vulnerable dependency.
    pub package: String,
    /// Its version (cargo) or the affected range (npm).
    pub version: String,
    /// How the workspace reaches it, root first: `app 0.1.0 > time 0.1.43`.
    pub path: String,
    /// The advisory's title.
    pub title: String,
}

impl Finding {
    /// What makes two findings the same advisory on the same dependency of the same lockfile, in this change and on main. The
    /// version is left out: the same advisory on the same dependency is already decided on main, whichever version it hits.
    pub fn key(&self) -> (String, String, String) {
        (self.source.clone(), self.id.clone(), self.package.clone())
    }
}

/// What one audit run answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Measure {
    /// The tool ran to its end: these are its findings (possibly none).
    Found(Vec<Finding>),
    /// The tool did not look; the reason is what to read before running it again.
    NotMeasured(String),
}

/// npm's severities, lowest first.
pub const NPM_LEVELS: [&str; 5] = ["info", "low", "moderate", "high", "critical"];

fn rank(level: &str) -> usize {
    NPM_LEVELS.iter().position(|l| *l == level).unwrap_or(0)
}

fn last_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines[lines.len().saturating_sub(2)..].join(" | ")
}

fn krate(v: &Value) -> Option<(String, String)> {
    let k = v.get("Krate")?;
    Some((k.get("name")?.as_str()?.to_string(), k.get("version")?.as_str()?.to_string()))
}

/// The first parent chain of a diagnostic's graph, root first.
fn cargo_path(graph: &Value) -> String {
    let mut chain = Vec::new();
    let mut at = Some(graph);
    while let Some(node) = at {
        if let Some((n, v)) = krate(node) {
            chain.push(format!("{n} {v}"));
        }
        at = node.get("parents").and_then(Value::as_array).and_then(|p| p.first());
    }
    chain.reverse();
    chain.join(" > ")
}

/// `cargo deny --format json check advisories`: the JSON lines on stderr (stdout when a version prints them there). Only
/// diagnostics of severity `error` count, as in the blocking `cargo deny check` (a `warn` setting stays a warning).
pub fn parse_deny(source: &str, text: &str) -> Measure {
    let mut found: Vec<Finding> = Vec::new();
    let mut summary_errors: Option<u64> = None;
    let mut last_error_log = String::new();
    for line in text.lines().filter(|l| l.trim_start().starts_with('{')) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let fields = v.get("fields");
        match v.get("type").and_then(Value::as_str) {
            Some("summary") => {
                summary_errors = fields.and_then(|f| f.pointer("/advisories/errors")).and_then(Value::as_u64);
            }
            Some("log") if fields.and_then(|f| f.get("level")).and_then(Value::as_str) == Some("ERROR") => {
                last_error_log = fields
                    .and_then(|f| f.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
            Some("diagnostic") => {
                let Some(f) = fields else { continue };
                if f.get("severity").and_then(Value::as_str) != Some("error") {
                    continue;
                }
                let code = f.get("code").and_then(Value::as_str).unwrap_or_default();
                let advisory = f.get("advisory");
                if advisory.is_none() && code != "yanked" {
                    continue;
                }
                let graph = f.pointer("/graphs/0");
                let (package, version) = graph
                    .and_then(krate)
                    .or_else(|| {
                        let p = advisory?.get("package")?.as_str()?;
                        Some((p.to_string(), String::new()))
                    })
                    .unwrap_or_default();
                let finding = Finding {
                    source: source.to_string(),
                    id: advisory
                        .and_then(|a| a.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or(code)
                        .to_string(),
                    package,
                    version,
                    path: graph.map(cargo_path).unwrap_or_default(),
                    title: f.get("message").and_then(Value::as_str).unwrap_or_default().to_string(),
                };
                if !found.contains(&finding) {
                    found.push(finding);
                }
            }
            _ => {}
        }
    }
    match summary_errors {
        None => {
            let why = if last_error_log.is_empty() {
                last_lines(text)
            } else {
                last_error_log
            };
            Measure::NotMeasured(format!("cargo deny printed no summary ({why})"))
        }
        Some(n) if n > 0 && found.is_empty() => {
            Measure::NotMeasured(format!("cargo deny counted {n} advisory errors but printed no advisory diagnostic"))
        }
        Some(_) => Measure::Found(found),
    }
}

/// The advisory id of an npm `via` url (`https://github.com/advisories/GHSA-xxxx`).
fn npm_id(via: &Value) -> String {
    let url = via.get("url").and_then(Value::as_str).unwrap_or_default();
    match url.rsplit('/').next() {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => format!("npm-{}", via.get("source").map(Value::to_string).unwrap_or_default()),
    }
}

/// The shortest chain from a direct dependency down to `package`, root first, along the report's `effects` edges.
fn npm_path(vulns: &BTreeMap<&str, &Value>, package: &str) -> String {
    let mut prev: BTreeMap<&str, &str> = BTreeMap::new();
    let mut seen: BTreeSet<&str> = BTreeSet::from([package]);
    let mut queue: VecDeque<&str> = VecDeque::from([package]);
    let mut root = package;
    while let Some(at) = queue.pop_front() {
        let entry = vulns.get(at);
        if entry.and_then(|e| e.get("isDirect")).and_then(Value::as_bool) == Some(true) {
            root = at;
            break;
        }
        for next in entry
            .and_then(|e| e.get("effects"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if seen.insert(next) {
                prev.insert(next, at);
                queue.push_back(next);
            }
        }
    }
    let mut chain = vec![root];
    let mut at = root;
    while let Some(p) = prev.get(at) {
        chain.push(p);
        at = p;
    }
    chain.join(" > ")
}

/// `npm audit --json` (npm 7 and later), the advisories at `level` or above.
pub fn parse_npm(source: &str, text: &str, level: &str) -> Measure {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Measure::NotMeasured(format!("npm audit printed no JSON ({})", last_lines(text)));
    };
    if let Some(e) = v.get("error") {
        let why = e
            .get("summary")
            .and_then(Value::as_str)
            .or_else(|| e.get("code").and_then(Value::as_str))
            .unwrap_or("an error");
        return Measure::NotMeasured(format!("npm audit answered with {why}"));
    }
    let Some(vulns) = v.get("vulnerabilities").and_then(Value::as_object) else {
        return Measure::NotMeasured("npm audit's answer has no `vulnerabilities` object".into());
    };
    let by_name: BTreeMap<&str, &Value> = vulns.iter().map(|(k, v)| (k.as_str(), v)).collect();
    let min = rank(level);
    let mut found: Vec<Finding> = Vec::new();
    for entry in vulns.values() {
        for via in entry
            .get("via")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|x| x.is_object())
        {
            let sev = via.get("severity").and_then(Value::as_str).unwrap_or("info");
            if rank(sev) < min {
                continue;
            }
            let package = via.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
            let finding = Finding {
                source: source.to_string(),
                id: npm_id(via),
                path: npm_path(&by_name, &package),
                version: via.get("range").and_then(Value::as_str).unwrap_or_default().to_string(),
                title: via.get("title").and_then(Value::as_str).unwrap_or_default().to_string(),
                package,
            };
            if !found.contains(&finding) {
                found.push(finding);
            }
        }
    }
    found.sort_by(|a, b| (&a.id, &a.package).cmp(&(&b.id, &b.package)));
    Measure::Found(found)
}
