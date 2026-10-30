//! rdjson (reviewdog's diagnostic format) reader. Columns are code points; see `sarif.rs` for the conversion to our bytes.

use pacenotes_core::{Location, Position, RepoPath, Severity, Span};
use serde_json::Value;

use super::sarif::{choose_fix, to_byte_column};
use super::{Finding, FixSource, Imported, ReadStats, RuleId, SarifError, SarifLimits};

fn number(v: Option<&Value>) -> Option<u32> {
    v.and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0)
}

/// Reads an rdjson document (`{"source": {...}, "diagnostics": [...]}`). Bounded, never panics; results without a rule code or a usable
/// path are counted, not guessed.
pub fn read_rdjson(
    text: &str,
    limits: &SarifLimits,
    texts: &dyn Fn(&RepoPath) -> Option<String>,
) -> Result<(Vec<Imported>, ReadStats), SarifError> {
    if text.len() > limits.max_bytes {
        return Err(SarifError::TooBig {
            len: text.len(),
            max: limits.max_bytes,
        });
    }
    let doc: Value = serde_json::from_str(text).map_err(|e| SarifError::Json(e.to_string()))?;
    let diagnostics = doc
        .get("diagnostics")
        .and_then(Value::as_array)
        .ok_or(SarifError::NotSarif("`diagnostics` is missing (not rdjson)"))?;
    let tool = doc
        .pointer("/source/name")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty())
        .unwrap_or("rdjson");
    let mut out = Vec::new();
    let mut stats = ReadStats::default();
    for d in diagnostics {
        if out.len() >= limits.max_results {
            stats.truncated += 1;
            continue;
        }
        let Some(raw_id) = d.pointer("/code/value").and_then(Value::as_str) else {
            stats.no_rule += 1;
            continue;
        };
        let Ok(rule) = RuleId::external(tool, raw_id) else {
            stats.no_rule += 1;
            continue;
        };
        let severity = match d
            .get("severity")
            .and_then(Value::as_str)
            .or_else(|| doc.get("severity").and_then(Value::as_str))
        {
            Some("ERROR") => Severity::High,
            Some("INFO") => Severity::Info,
            _ => Severity::Medium,
        };
        let location = d
            .pointer("/location/path")
            .and_then(Value::as_str)
            .and_then(|p| match RepoPath::parse(p) {
                Ok(path) => Some(path),
                Err(_) => {
                    stats.bad_location += 1;
                    None
                }
            });
        let location = location.and_then(|path| {
            let start = d.pointer("/location/range/start");
            let end = d.pointer("/location/range/end");
            let line = number(start.and_then(|s| s.get("line")))?;
            let end_line = number(end.and_then(|e| e.get("line"))).filter(|l| *l >= line).unwrap_or(line);
            if let (Some(sc), Some(ec)) = (
                number(start.and_then(|s| s.get("column"))),
                number(end.and_then(|e| e.get("column"))),
            ) {
                match texts(&path) {
                    Some(t) => {
                        if let (Some(sb), Some(eb)) = (to_byte_column(&t, line, sc, false), to_byte_column(&t, end_line, ec, false))
                            && let (Some(a), Some(b)) = (Position::new(line, sb), Position::new(end_line, eb))
                            && let Ok(span) = Span::new(a, b)
                        {
                            return Some(Location { path, span });
                        }
                    }
                    None => stats.columns_dropped += 1,
                }
            }
            Span::lines(line, end_line).ok().map(|span| Location { path, span })
        });
        let (fix, fix_source) = choose_fix(None, FixSource::Tool, &[], tool, raw_id);
        out.push(Imported {
            finding: Finding {
                rule,
                severity,
                location,
                message: d
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(limits.max_message)
                    .collect(),
                fix,
                fix_source,
                explanation: None,
                evidence: Vec::new(),
                fix_diff: None,
                fingerprint: None,
                confidence: None,
            },
            suppressed_by: None,
        });
    }
    Ok((out, stats))
}
