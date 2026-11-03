//! `llvm-cov export -format=text` JSON: `data[].files[]` with `filename` and `segments`, each `[line, col, count, hasCount, isRegionEntry,
//! isGap]`. Line counts are derived as llvm-cov does: a line that starts counted regions takes the largest of their counts, and a line
//! inside a region that began earlier takes that region's count; the segment that ends a region (`hasCount` false) closes it.

use serde_json::Value;

use super::{ImportContext, ImportError, check_cap, check_files, check_lines, line_no, malformed, resolve};
use crate::model::CoverageReport;

pub(super) fn read(text: &str, cx: &ImportContext<'_>, report: &mut CoverageReport) -> Result<(), ImportError> {
    let doc: Value = serde_json::from_str(text).map_err(|e| malformed("llvm-cov JSON", Some(e.line()), e.to_string()))?;
    let data = doc
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("llvm-cov JSON", None, "`data` is missing"))?;
    let mut total = 0usize;
    // the lines a report may describe grow with its size: a few hundred bytes of segments cannot reach millions of lines
    let cap = text
        .len()
        .saturating_mul(cx.limits.max_lines_per_input_byte)
        .saturating_add(100_000)
        .min(cx.limits.max_total_lines);
    for export in data {
        let Some(files) = export.get("files").and_then(Value::as_array) else {
            continue;
        };
        for f in files {
            let Some(name) = f.get("filename").and_then(Value::as_str) else {
                continue;
            };
            let Some(path) = resolve(cx, cx.mapper.map(name).into_iter().collect(), report) else {
                continue;
            };
            let segments = f.get("segments").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
            let mut parsed: Vec<(u64, u64, u64, bool, bool, bool)> = Vec::new();
            for s in segments {
                let shaped = s.as_array().and_then(|a| {
                    Some((
                        a.first()?.as_u64()?,
                        a.get(1)?.as_u64()?,
                        a.get(2)?.as_u64()?,
                        flag(a.get(3)?)?,
                        flag(a.get(4)?)?,
                        a.get(5).and_then(flag).unwrap_or(false),
                    ))
                });
                match shaped {
                    Some(seg) => parsed.push(seg),
                    None => report.skipped.dropped_segments = report.skipped.dropped_segments.saturating_add(1),
                }
            }
            // lines are 32-bit and segments are listed in order: anything else is a broken report, and refusing it here is what keeps the
            // gap fill below from walking billions of numbers
            for pair in parsed.windows(2) {
                if (pair[1].0, pair[1].1) < (pair[0].0, pair[0].1) {
                    return Err(malformed(
                        "llvm-cov JSON",
                        None,
                        format!(
                            "the segments of {name:?} are not in order (line {} after line {})",
                            pair[1].0, pair[0].0
                        ),
                    ));
                }
            }
            if let Some(big) = parsed.iter().find(|s| s.0 > u64::from(u32::MAX)) {
                return Err(malformed(
                    "llvm-cov JSON",
                    None,
                    format!("a segment of {name:?} has line {}, past the 32-bit line numbers", big.0),
                ));
            }
            let file = report.file_mut(path);
            let mut open: Option<u64> = None;
            let mut i = 0;
            while i < parsed.len() {
                let line = parsed[i].0;
                // everything that starts on this line
                let mut j = i;
                let mut best: Option<u64> = None;
                let mut last_open = open;
                // a region that ends at column 1 leaves nothing of this line counted
                let mut only_closing_at_start = true;
                while j < parsed.len() && parsed[j].0 == line {
                    let (_, col, count, has, entry, gap) = parsed[j];
                    if has && !gap {
                        last_open = Some(count);
                        if entry {
                            best = Some(best.map_or(count, |b| b.max(count)));
                        }
                    } else if !has {
                        last_open = None;
                        if col != 1 {
                            only_closing_at_start = false;
                        }
                    }
                    if has {
                        only_closing_at_start = false;
                    }
                    j += 1;
                }
                if let Some(l) = line_no(line) {
                    if let Some(c) = best.or(if only_closing_at_start { None } else { open }).or(last_open) {
                        file.hit(l, c);
                    }
                    check_lines(file.lines.len(), cx)?;
                    check_cap(total, file.lines.len(), cap)?;
                }
                // lines strictly between this segment group and the next take the open region's count; every one adds a line, so the
                // per-file and whole-report limits end the loop
                if let (Some(c), Some(next)) = (last_open, parsed.get(j)) {
                    let mut l = line + 1;
                    while l < next.0 {
                        if let Some(ln) = line_no(l) {
                            file.hit(ln, c);
                        }
                        check_lines(file.lines.len(), cx)?;
                        check_cap(total, file.lines.len(), cap)?;
                        l += 1;
                    }
                }
                open = last_open;
                i = j;
            }
            total += file.lines.len();
            check_files(report, cx)?;
        }
    }
    Ok(())
}

/// A JSON boolean, or a number read as one (some llvm versions write 0 and 1).
fn flag(v: &Value) -> Option<bool> {
    v.as_bool().or_else(|| v.as_u64().map(|n| n > 0))
}
