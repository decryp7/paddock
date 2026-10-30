//! SARIF 2.1.0 writer and reader (note section 4). Columns in our model are UTF-8 bytes; SARIF's are code points or UTF-16 code
//! units, so both directions convert, and they need the file's text to do it.

use pacenotes_core::{Location, Position, RepoPath, Severity, Span};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::str::FromStr;

use super::{BaselineState, Finding, FindingFingerprint, FixSentence, FixSource, HiddenFinding, Imported, Reason, Report, Rule, RuleId};

/// The key of our fingerprint in `partialFingerprints`.
pub const FINGERPRINT_KEY: &str = "paddockFindingHash/v1";

/// Bounds of a read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SarifLimits {
    /// Largest input in bytes.
    pub max_bytes: usize,
    /// Most results taken from one input.
    pub max_results: usize,
    /// Longest message kept, in bytes.
    pub max_message: usize,
}

impl Default for SarifLimits {
    fn default() -> Self {
        SarifLimits {
            max_bytes: 128 * 1024 * 1024,
            max_results: 200_000,
            max_message: 4096,
        }
    }
}

/// What a read left out, counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadStats {
    /// Results whose `kind` is not `fail`.
    pub skipped_kind: usize,
    /// Results without a usable rule id.
    pub no_rule: usize,
    /// Results whose location could not be used (an absolute or unsafe path, a missing line).
    pub bad_location: usize,
    /// Results whose columns were dropped because the file's text was not available to convert them.
    pub columns_dropped: usize,
    /// Results past the limit.
    pub truncated: usize,
}

/// Why an input was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SarifError {
    /// Over the byte limit.
    #[error("the file is {len} bytes; at most {max} are read")]
    TooBig {
        /// Its size.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// Not JSON.
    #[error("not valid JSON: {0}")]
    Json(String),
    /// JSON, but not SARIF 2.1.0.
    #[error("not a SARIF 2.1.0 file: {0}")]
    NotSarif(&'static str),
}

/// A file's text with the byte index of each line start, so a line is found by binary search instead of by scanning from the top.
pub(super) struct TextIndex {
    text: String,
    starts: Vec<usize>,
}

impl TextIndex {
    pub(super) fn new(text: String) -> TextIndex {
        let mut starts = vec![0usize];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        TextIndex { text, starts }
    }

    pub(super) fn line_start(&self, line: u32) -> Option<usize> {
        self.starts
            .get((line as usize).checked_sub(1)?)
            .copied()
            .filter(|s| *s <= self.text.len())
    }

    fn line(&self, line: u32) -> Option<&str> {
        let start = self.line_start(line)?;
        let end = self.starts.get(line as usize).map_or(self.text.len(), |e| e - 1);
        self.text.get(start..end.max(start))
    }

    /// 1-based byte column to 1-based code-point column on `line`.
    pub(super) fn byte_to_code_point(&self, line: u32, column: u32) -> Option<u32> {
        let row = self.line(line)?;
        let at = (column as usize).checked_sub(1)?;
        row.is_char_boundary(at).then(|| row[..at].chars().count() as u32 + 1)
    }

    /// The column to bytes, from code points (`utf16` false) or UTF-16 code units (`utf16` true).
    pub(super) fn to_byte_column(&self, line: u32, column: u32, utf16: bool) -> Option<u32> {
        let row = self.line(line)?;
        let mut want = (column as usize).checked_sub(1)?;
        let mut bytes = 0usize;
        for c in row.chars() {
            if want == 0 {
                break;
            }
            let units = if utf16 { c.len_utf16() } else { 1 };
            want = want.checked_sub(units)?;
            bytes += c.len_utf8();
        }
        (want == 0).then_some(bytes as u32 + 1)
    }

    /// True when the 1-based byte column is the start of a character (or the end of the line) on `line`.
    pub(super) fn on_boundary(&self, line: u32, column: u32) -> bool {
        self.line(line).is_some_and(|row| {
            (column as usize)
                .checked_sub(1)
                .is_some_and(|at| row.is_char_boundary(at) && at <= row.len())
        })
    }
}

/// Fetches each file's text once per run, whatever the number of results in it.
pub(super) struct Texts<'a> {
    fetch: &'a dyn Fn(&RepoPath) -> Option<String>,
    cache: std::cell::RefCell<BTreeMap<String, Option<std::rc::Rc<TextIndex>>>>,
}

impl<'a> Texts<'a> {
    pub(super) fn new(fetch: &'a dyn Fn(&RepoPath) -> Option<String>) -> Texts<'a> {
        Texts {
            fetch,
            cache: std::cell::RefCell::new(BTreeMap::new()),
        }
    }

    pub(super) fn get(&self, path: &RepoPath) -> Option<std::rc::Rc<TextIndex>> {
        self.cache
            .borrow_mut()
            .entry(path.as_str().to_owned())
            .or_insert_with(|| (self.fetch)(path).map(|t| std::rc::Rc::new(TextIndex::new(t))))
            .clone()
    }
}

/// A finding in a report, with what hid it and its state against a baseline.
#[derive(Clone, Copy)]
struct Item<'a> {
    finding: &'a Finding,
    hidden: Option<&'a HiddenFinding>,
    baseline: Option<BaselineState>,
}

fn level(s: Severity) -> &'static str {
    match s {
        Severity::Info => "note",
        Severity::Low | Severity::Medium => "warning",
        Severity::High | Severity::Blocker => "error",
    }
}

fn region(span: &Span, text: Option<&TextIndex>) -> Value {
    let (s, e) = (span.start(), span.end());
    let mut r = json!({ "startLine": s.line(), "endLine": e.line() });
    if let Some(ix) = text
        && let (Some(sc), Some(ec)) = (
            ix.byte_to_code_point(s.line(), s.column()),
            ix.byte_to_code_point(e.line(), e.column()),
        )
    {
        r["startColumn"] = json!(sc);
        r["endColumn"] = json!(ec);
        if let (Some(start), Some(end_line)) = (ix.line_start(s.line()), ix.line_start(e.line())) {
            let from = start + s.column() as usize - 1;
            let to = end_line + e.column() as usize - 1;
            if to >= from && to <= ix.text.len() {
                r["byteOffset"] = json!(from);
                r["byteLength"] = json!(to - from);
            }
        }
    }
    r
}

fn rule_descriptor(id: &str, rule: Option<&Rule>, sample: Option<&Finding>) -> Value {
    let mut d = Map::new();
    d.insert("id".into(), json!(id));
    if let Some(r) = rule {
        d.insert("name".into(), json!(r.title));
        d.insert("shortDescription".into(), json!({ "text": r.title }));
        d.insert("defaultConfiguration".into(), json!({ "level": level(r.default_severity) }));
        d.insert("help".into(), json!({ "text": r.fix.as_str() }));
        let mut props = Map::new();
        if let Some(p) = &r.policy_ref {
            props.insert("policyRef".into(), json!(p));
        }
        props.insert("ruleVersion".into(), json!(r.version));
        d.insert("properties".into(), Value::Object(props));
    } else if let Some(f) = sample {
        d.insert("defaultConfiguration".into(), json!({ "level": level(f.severity) }));
        if f.fix_source != FixSource::Generated {
            d.insert("help".into(), json!({ "text": f.fix.as_str() }));
        }
    }
    Value::Object(d)
}

fn result(item: Item<'_>, rule_index: usize, rule_id: &str, texts: &Texts<'_>) -> Value {
    let f = item.finding;
    let mut r = Map::new();
    r.insert("ruleId".into(), json!(rule_id));
    r.insert("ruleIndex".into(), json!(rule_index));
    r.insert("level".into(), json!(level(f.severity)));
    r.insert("message".into(), json!({ "text": f.message }));
    if let Some(loc) = &f.location {
        let text = texts.get(&loc.path);
        r.insert(
            "locations".into(),
            json!([{ "physicalLocation": {
                "artifactLocation": { "uri": loc.path.as_str(), "uriBaseId": "%SRCROOT%" },
                "region": region(&loc.span, text.as_deref()),
            } }]),
        );
    }
    if let Some(fp) = &f.fingerprint {
        r.insert("partialFingerprints".into(), json!({ FINGERPRINT_KEY: fp.to_hex() }));
    }
    if let Some(state) = item.baseline {
        r.insert("baselineState".into(), json!(state.sarif_name()));
    }
    let mut props = Map::new();
    props.insert("severity".into(), json!(f.severity.name()));
    if f.fix_source != FixSource::Generated {
        props.insert("fix".into(), json!(f.fix.as_str()));
        props.insert(
            "fixSource".into(),
            json!(if f.fix_source == FixSource::Rule { "rule" } else { "tool" }),
        );
    }
    // a suggested change is a unified diff, which SARIF's `fixes` (artifact changes with replacements) cannot carry as it is
    if let Some(diff) = &f.fix_diff {
        props.insert("fixDiff".into(), json!(diff));
    }
    r.insert("properties".into(), Value::Object(props));
    if let Some(h) = item.hidden {
        let mut sup =
            json!({ "kind": if h.in_source { "inSource" } else { "external" }, "status": "accepted", "justification": h.reason.as_str() });
        let mut p = Map::new();
        if let Some(e) = h.expires {
            p.insert("expires".into(), json!(e.to_string()));
        }
        if let Some(o) = &h.owner {
            p.insert("owner".into(), json!(o));
        }
        if !p.is_empty() {
            sup["properties"] = Value::Object(p);
        }
        r.insert("suppressions".into(), json!([sup]));
    }
    Value::Object(r)
}

/// Writes a report as SARIF 2.1.0 (pretty JSON). Our findings go in our `run`; each imported tool's results go back in **its own
/// `run`** with its raw rule id. Suppressed results stay in `results` with `suppressions` set (`hidden` pairs a finding with the
/// reason that hid it and whether that was in the source). `texts` gives a file's text so byte columns can be written as code points; a
/// finding whose file has no text keeps its lines and loses its columns.
pub fn write_sarif(
    report: &Report,
    hidden: &[HiddenFinding],
    baseline: &[Option<BaselineState>],
    fetch: &dyn Fn(&RepoPath) -> Option<String>,
) -> String {
    let texts = Texts::new(fetch);
    let mut ours: Vec<Item<'_>> = Vec::new();
    let mut theirs: BTreeMap<&str, Vec<Item<'_>>> = BTreeMap::new();
    let all = report
        .findings
        .iter()
        .enumerate()
        .map(|(i, f)| Item {
            finding: f,
            hidden: None,
            baseline: baseline.get(i).copied().flatten(),
        })
        .chain(hidden.iter().map(|h| Item {
            finding: &h.finding,
            hidden: Some(h),
            baseline: None,
        }));
    for item in all {
        match &item.finding.rule {
            RuleId::Local(_) => ours.push(item),
            RuleId::External(e) => theirs.entry(e.tool()).or_default().push(item),
        }
    }
    let mut runs = Vec::new();
    // our run
    let mut ids: Vec<String> = report.rules.iter().map(|r| r.id.to_string()).collect();
    for item in &ours {
        let id = item.finding.rule.to_string();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let rules: Vec<Value> = ids
        .iter()
        .map(|id| {
            rule_descriptor(
                id,
                report.rules.iter().find(|r| &r.id.to_string() == id),
                ours.iter().map(|i| i.finding).find(|f| &f.rule.to_string() == id),
            )
        })
        .collect();
    let results: Vec<Value> = ours
        .iter()
        .map(|item| {
            let id = item.finding.rule.to_string();
            let index = ids.iter().position(|x| *x == id).unwrap_or(0);
            result(*item, index, &id, &texts)
        })
        .collect();
    runs.push(json!({
        "tool": { "driver": { "name": report.tool, "version": report.tool_version, "rules": rules } },
        "automationDetails": { "id": report.scope },
        "columnKind": "unicodeCodePoints",
        "originalUriBaseIds": { "%SRCROOT%": { "description": { "text": "The root of the repository" } } },
        "results": results,
    }));
    for (tool, items) in theirs {
        let mut raw: Vec<&str> = Vec::new();
        for item in &items {
            if let RuleId::External(e) = &item.finding.rule
                && !raw.contains(&e.id())
            {
                raw.push(e.id());
            }
        }
        let rules: Vec<Value> = raw
            .iter()
            .map(|id| {
                rule_descriptor(
                    id,
                    None,
                    items
                        .iter()
                        .map(|i| i.finding)
                        .find(|f| matches!(&f.rule, RuleId::External(e) if e.id() == *id)),
                )
            })
            .collect();
        let results: Vec<Value> = items
            .iter()
            .map(|item| {
                let id = match &item.finding.rule {
                    RuleId::External(e) => e.id(),
                    RuleId::Local(_) => "",
                };
                result(*item, raw.iter().position(|x| *x == id).unwrap_or(0), id, &texts)
            })
            .collect();
        runs.push(json!({
            "tool": { "driver": { "name": tool, "rules": rules } },
            "columnKind": "unicodeCodePoints",
            "originalUriBaseIds": { "%SRCROOT%": { "description": { "text": "The root of the repository" } } },
            "results": results,
        }));
    }
    let doc = json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": runs,
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default()
}

fn text_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.get("text")?.as_str()
}

pub(super) fn cut(s: &str, max: usize) -> String {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

fn generated(tool: &str, rule: &str) -> FixSentence {
    let sentence = format!("{} gives no fix for {}; see its documentation.", cut(tool, 100), cut(rule, 150));
    FixSentence::parse(&sentence).unwrap_or_else(|_| FixSentence::known("The tool gives no fix; see its documentation."))
}

/// Chooses a result's fix sentence (note section 3.4).
pub(super) fn choose_fix(
    own: Option<&str>,
    own_source: FixSource,
    from_tool: &[Option<&str>],
    tool: &str,
    rule: &str,
) -> (FixSentence, FixSource) {
    if let Some(s) = own.and_then(|t| FixSentence::parse(t).ok()) {
        return (s, own_source);
    }
    for text in from_tool.iter().flatten() {
        if let Some(s) = FixSentence::first_of(text) {
            return (s, FixSource::Tool);
        }
    }
    (generated(tool, rule), FixSource::Generated)
}

fn severity_of(level: Option<&str>, exact: Option<&str>) -> Severity {
    if let Some(s) = exact.and_then(|s| Severity::from_str(s).ok()) {
        return s;
    }
    match level {
        Some("note" | "none") => Severity::Info,
        Some("error") => Severity::High,
        _ => Severity::Medium,
    }
}

/// Reads SARIF 2.1.0. Bounded, never panics. Only results of `kind` `fail` (or no kind) are taken; the others are counted. A result
/// with `suppressions` comes back with the reason that hid it. The rule of an imported result is an [`super::ExternalRuleId`] with the
/// tool's name and the raw id, except in a run whose driver is `own_tool`, whose ids are ours.
pub fn read_sarif(
    text: &str,
    limits: &SarifLimits,
    own_tool: &str,
    fetch: &dyn Fn(&RepoPath) -> Option<String>,
) -> Result<(Vec<Imported>, ReadStats), SarifError> {
    let texts = Texts::new(fetch);
    if text.len() > limits.max_bytes {
        return Err(SarifError::TooBig {
            len: text.len(),
            max: limits.max_bytes,
        });
    }
    let doc: Value = serde_json::from_str(text).map_err(|e| SarifError::Json(e.to_string()))?;
    if doc.get("version").and_then(Value::as_str) != Some("2.1.0") {
        return Err(SarifError::NotSarif("`version` is not \"2.1.0\""));
    }
    let runs = doc
        .get("runs")
        .and_then(Value::as_array)
        .ok_or(SarifError::NotSarif("`runs` is missing"))?;
    let mut out = Vec::new();
    let mut stats = ReadStats::default();
    for run in runs {
        let driver = run.pointer("/tool/driver");
        let tool = driver.and_then(|d| d.get("name")).and_then(Value::as_str).unwrap_or("unknown");
        let utf16 = run.get("columnKind").and_then(Value::as_str) != Some("unicodeCodePoints");
        let descriptors: Vec<&Value> = driver
            .and_then(|d| d.get("rules"))
            .and_then(Value::as_array)
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        let Some(results) = run.get("results").and_then(Value::as_array) else {
            continue;
        };
        for r in results {
            if out.len() >= limits.max_results {
                stats.truncated += 1;
                continue;
            }
            if r.get("kind").and_then(Value::as_str).is_some_and(|k| k != "fail") {
                stats.skipped_kind += 1;
                continue;
            }
            let raw_id = r
                .get("ruleId")
                .and_then(Value::as_str)
                .or_else(|| r.pointer("/rule/id").and_then(Value::as_str))
                .or_else(|| {
                    descriptors
                        .get(r.get("ruleIndex").and_then(Value::as_u64).unwrap_or(u64::MAX) as usize)
                        .and_then(|d| d.get("id"))
                        .and_then(Value::as_str)
                });
            let Some(raw_id) = raw_id else {
                stats.no_rule += 1;
                continue;
            };
            let rule = if tool == own_tool {
                RuleId::local(raw_id).or_else(|_| RuleId::external(tool, raw_id))
            } else {
                RuleId::external(tool, raw_id)
            };
            let Ok(rule) = rule else {
                stats.no_rule += 1;
                continue;
            };
            let descriptor = descriptors
                .iter()
                .find(|d| d.get("id").and_then(Value::as_str) == Some(raw_id))
                .copied();
            let level = r
                .get("level")
                .and_then(Value::as_str)
                .or_else(|| {
                    descriptor
                        .and_then(|d| d.pointer("/defaultConfiguration/level"))
                        .and_then(Value::as_str)
                })
                .or(Some("warning"));
            let severity = severity_of(level, r.pointer("/properties/severity").and_then(Value::as_str));
            let location = location_of(r, utf16, &texts, &mut stats);
            let message = cut(text_of(r, "message").unwrap_or(""), limits.max_message);
            let own_fix = r.pointer("/properties/fix").and_then(Value::as_str);
            let own_source = if r.pointer("/properties/fixSource").and_then(Value::as_str) == Some("rule") {
                FixSource::Rule
            } else {
                FixSource::Tool
            };
            let (fix, fix_source) = choose_fix(
                own_fix,
                own_source,
                &[
                    r.pointer("/fixes/0/description/text").and_then(Value::as_str),
                    descriptor.and_then(|d| d.pointer("/help/text")).and_then(Value::as_str),
                    descriptor.and_then(|d| d.pointer("/fullDescription/text")).and_then(Value::as_str),
                ],
                tool,
                raw_id,
            );
            let fingerprint = r
                .pointer("/partialFingerprints")
                .and_then(|p| p.get(FINGERPRINT_KEY))
                .and_then(Value::as_str)
                .and_then(FindingFingerprint::from_hex);
            let fix_diff = r.pointer("/properties/fixDiff").and_then(Value::as_str).map(|d| cut(d, 64 * 1024));
            // only an accepted suppression (or one with no status) hides a result; `rejected` and `underReview` do not
            let suppressed_by = r
                .get("suppressions")
                .and_then(Value::as_array)
                .and_then(|a| {
                    a.iter()
                        .find(|s| s.get("status").and_then(Value::as_str).is_none_or(|st| st == "accepted"))
                })
                .map(|s| {
                    Reason::parse(s.get("justification").and_then(Value::as_str).unwrap_or(""))
                        .unwrap_or_else(|_| Reason::known("suppressed in the source tool (no reason given)"))
                });
            out.push(Imported {
                finding: Finding {
                    rule,
                    severity,
                    location,
                    message,
                    fix,
                    fix_source,
                    explanation: None,
                    evidence: Vec::new(),
                    fix_diff,
                    fingerprint,
                    confidence: None,
                },
                suppressed_by,
            });
        }
    }
    Ok((out, stats))
}

fn location_of(r: &Value, utf16: bool, texts: &Texts<'_>, stats: &mut ReadStats) -> Option<Location> {
    let phys = r.pointer("/locations/0/physicalLocation")?;
    let uri = phys.pointer("/artifactLocation/uri").and_then(Value::as_str)?;
    let Ok(path) = RepoPath::parse(uri) else {
        stats.bad_location += 1;
        return None;
    };
    let region = phys.get("region");
    let num = |key: &str| {
        region
            .and_then(|g| g.get(key))
            .and_then(Value::as_u64)
            .and_then(|l| u32::try_from(l).ok())
            .filter(|l| *l > 0)
    };
    let Some(start_line) = num("startLine") else {
        stats.bad_location += 1;
        return None;
    };
    let end_line = num("endLine").filter(|l| *l >= start_line).unwrap_or(start_line);
    match (num("startColumn"), num("endColumn")) {
        (Some(sc), Some(ec)) => match texts.get(&path) {
            Some(ix) => {
                if let (Some(sb), Some(eb)) = (ix.to_byte_column(start_line, sc, utf16), ix.to_byte_column(end_line, ec, utf16))
                    && let (Some(a), Some(b)) = (Position::new(start_line, sb), Position::new(end_line, eb))
                    && let Ok(span) = Span::new(a, b)
                {
                    return Some(Location { path, span });
                }
                stats.columns_dropped += 1;
            }
            None => stats.columns_dropped += 1,
        },
        // a start without an end cannot be turned into a span
        (Some(_), None) | (None, Some(_)) => stats.columns_dropped += 1,
        (None, None) => {}
    }
    Span::lines(start_line, end_line).ok().map(|span| Location { path, span })
}
