//! SARIF 2.1.0 writer and reader (note section 4). Columns in our model are UTF-8 bytes; SARIF's are code points or UTF-16 code
//! units, so both directions convert, and they need the file's text to do it.

use pacenotes_core::{Location, Position, RepoPath, Severity, Span};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::str::FromStr;

use super::{Finding, FindingFingerprint, FixSentence, FixSource, Imported, Reason, Report, Rule, RuleId};

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

/// The byte index where line `line` (1-based) starts.
pub(super) fn line_start(text: &str, line: u32) -> Option<usize> {
    if line == 0 {
        return None;
    }
    let mut start = 0usize;
    for _ in 1..line {
        start += text[start..].find('\n')? + 1;
    }
    Some(start)
}

fn line_of(text: &str, line: u32) -> Option<&str> {
    let start = line_start(text, line)?;
    let rest = &text[start..];
    Some(rest.split('\n').next().unwrap_or(rest))
}

/// 1-based byte column to 1-based code-point column on `line`.
pub(super) fn byte_to_code_point(text: &str, line: u32, column: u32) -> Option<u32> {
    let row = line_of(text, line)?;
    let at = (column as usize).checked_sub(1)?;
    row.is_char_boundary(at).then(|| row[..at].chars().count() as u32 + 1)
}

/// The column to bytes, from code points (`utf16` false) or UTF-16 code units (`utf16` true).
pub(super) fn to_byte_column(text: &str, line: u32, column: u32, utf16: bool) -> Option<u32> {
    let row = line_of(text, line)?;
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

/// A finding with the reason that hid it and whether that was in the source, when something did.
type Entry<'a> = (&'a Finding, Option<(&'a Reason, bool)>);

fn level(s: Severity) -> &'static str {
    match s {
        Severity::Info => "note",
        Severity::Low | Severity::Medium => "warning",
        Severity::High | Severity::Blocker => "error",
    }
}

fn region(span: &Span, text: Option<&str>) -> Value {
    let (s, e) = (span.start(), span.end());
    let mut r = json!({ "startLine": s.line(), "endLine": e.line() });
    if let Some(text) = text
        && let (Some(sc), Some(ec)) = (
            byte_to_code_point(text, s.line(), s.column()),
            byte_to_code_point(text, e.line(), e.column()),
        )
    {
        r["startColumn"] = json!(sc);
        r["endColumn"] = json!(ec);
        if let Some(start) = line_start(text, s.line()) {
            let from = start + s.column() as usize - 1;
            if let Some(end_line) = line_start(text, e.line()) {
                let to = end_line + e.column() as usize - 1;
                if to >= from && to <= text.len() {
                    r["byteOffset"] = json!(from);
                    r["byteLength"] = json!(to - from);
                }
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

fn result(
    f: &Finding,
    rule_index: usize,
    rule_id: &str,
    texts: &dyn Fn(&RepoPath) -> Option<String>,
    suppression: Option<(&Reason, bool)>,
) -> Value {
    let mut r = Map::new();
    r.insert("ruleId".into(), json!(rule_id));
    r.insert("ruleIndex".into(), json!(rule_index));
    r.insert("level".into(), json!(level(f.severity)));
    r.insert("message".into(), json!({ "text": f.message }));
    if let Some(loc) = &f.location {
        let text = texts(&loc.path);
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
    if let Some(diff) = &f.fix_diff {
        r.insert(
            "fixes".into(),
            json!([{ "description": { "text": f.fix.as_str() }, "properties": { "diff": diff } }]),
        );
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
    r.insert("properties".into(), Value::Object(props));
    if let Some((reason, in_source)) = suppression {
        r.insert(
            "suppressions".into(),
            json!([{ "kind": if in_source { "inSource" } else { "external" }, "status": "accepted", "justification": reason.as_str() }]),
        );
    }
    Value::Object(r)
}

/// Writes a report as SARIF 2.1.0 (pretty JSON). Our findings go in our `run`; each imported tool's results go back in **its own
/// `run`** with its raw rule id. Suppressed results stay in `results` with `suppressions` set (`hidden` pairs a finding with the
/// reason that hid it and whether that was in the source). `texts` gives a file's text so byte columns can be written as code points; a
/// finding whose file has no text keeps its lines and loses its columns.
pub fn write_sarif(report: &Report, hidden: &[(Finding, Reason, bool)], texts: &dyn Fn(&RepoPath) -> Option<String>) -> String {
    let mut ours: Vec<Entry<'_>> = Vec::new();
    let mut theirs: BTreeMap<&str, Vec<Entry<'_>>> = BTreeMap::new();
    let all = report
        .findings
        .iter()
        .map(|f| (f, None))
        .chain(hidden.iter().map(|(f, r, s)| (f, Some((r, *s)))));
    for (f, sup) in all {
        match &f.rule {
            RuleId::Local(_) => ours.push((f, sup)),
            RuleId::External(e) => theirs.entry(e.tool()).or_default().push((f, sup)),
        }
    }
    let mut runs = Vec::new();
    // our run
    let mut ids: Vec<String> = report.rules.iter().map(|r| r.id.to_string()).collect();
    for (f, _) in &ours {
        let id = f.rule.to_string();
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
                ours.iter().map(|(f, _)| *f).find(|f| &f.rule.to_string() == id),
            )
        })
        .collect();
    let results: Vec<Value> = ours
        .iter()
        .map(|(f, sup)| {
            let id = f.rule.to_string();
            let index = ids.iter().position(|x| *x == id).unwrap_or(0);
            result(f, index, &id, texts, *sup)
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
        for (f, _) in &items {
            if let RuleId::External(e) = &f.rule
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
                        .map(|(f, _)| *f)
                        .find(|f| matches!(&f.rule, RuleId::External(e) if e.id() == *id)),
                )
            })
            .collect();
        let results: Vec<Value> = items
            .iter()
            .map(|(f, sup)| {
                let id = match &f.rule {
                    RuleId::External(e) => e.id(),
                    RuleId::Local(_) => "",
                };
                result(f, raw.iter().position(|x| *x == id).unwrap_or(0), id, texts, *sup)
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

fn cut(s: &str, max: usize) -> String {
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
    texts: &dyn Fn(&RepoPath) -> Option<String>,
) -> Result<(Vec<Imported>, ReadStats), SarifError> {
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
            let location = location_of(r, utf16, texts, &mut stats);
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
            let fix_diff = r
                .pointer("/fixes/0/properties/diff")
                .and_then(Value::as_str)
                .map(|d| cut(d, 64 * 1024));
            let suppressed_by = r.get("suppressions").and_then(Value::as_array).and_then(|a| a.first()).map(|s| {
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

fn location_of(r: &Value, utf16: bool, texts: &dyn Fn(&RepoPath) -> Option<String>, stats: &mut ReadStats) -> Option<Location> {
    let phys = r.pointer("/locations/0/physicalLocation")?;
    let uri = phys.pointer("/artifactLocation/uri").and_then(Value::as_str)?;
    let Ok(path) = RepoPath::parse(uri) else {
        stats.bad_location += 1;
        return None;
    };
    let region = phys.get("region");
    let line = region
        .and_then(|g| g.get("startLine"))
        .and_then(Value::as_u64)
        .and_then(|l| u32::try_from(l).ok())
        .filter(|l| *l > 0);
    let Some(start_line) = line else {
        stats.bad_location += 1;
        return None;
    };
    let end_line = region
        .and_then(|g| g.get("endLine"))
        .and_then(Value::as_u64)
        .and_then(|l| u32::try_from(l).ok())
        .filter(|l| *l >= start_line)
        .unwrap_or(start_line);
    let col = |key: &str| {
        region
            .and_then(|g| g.get(key))
            .and_then(Value::as_u64)
            .and_then(|c| u32::try_from(c).ok())
            .filter(|c| *c > 0)
    };
    let (sc, ec) = (col("startColumn"), col("endColumn"));
    if let (Some(sc), Some(ec)) = (sc, ec) {
        match texts(&path) {
            Some(text) => {
                if let (Some(sb), Some(eb)) = (
                    to_byte_column(&text, start_line, sc, utf16),
                    to_byte_column(&text, end_line, ec, utf16),
                ) && let (Some(a), Some(b)) = (Position::new(start_line, sb), Position::new(end_line, eb))
                    && let Ok(span) = Span::new(a, b)
                {
                    return Some(Location { path, span });
                }
            }
            None => stats.columns_dropped += 1,
        }
    }
    Span::lines(start_line, end_line).ok().map(|span| Location { path, span })
}
