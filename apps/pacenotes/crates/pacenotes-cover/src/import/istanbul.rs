//! istanbul's `coverage-final.json`: one object per file with `statementMap` (id to a location), `s` (id to hits), `branchMap` (id to
//! a branch with its `line`) and `b` (id to an array of hits per arm). A statement's line is where it starts.

use serde_json::Value;

use super::{ImportContext, ImportError, check_files, check_lines, check_total, line_no, malformed, resolve};
use crate::model::CoverageReport;

pub(super) fn read(text: &str, cx: &ImportContext<'_>, report: &mut CoverageReport) -> Result<(), ImportError> {
    let doc: Value = serde_json::from_str(text).map_err(|e| malformed("istanbul JSON", Some(e.line()), e.to_string()))?;
    let files = doc
        .as_object()
        .ok_or_else(|| malformed("istanbul JSON", None, "the top level is not an object of files"))?;
    let mut total = 0usize;
    for (key, f) in files {
        let name = f.get("path").and_then(Value::as_str).unwrap_or(key);
        let Some(path) = resolve(cx, cx.mapper.map(name).into_iter().collect(), report) else {
            continue;
        };
        let (mut bad, mut odd) = (0u32, 0u32);
        let file = report.file_mut(path);
        if let (Some(map), Some(hits)) = (
            f.get("statementMap").and_then(Value::as_object),
            f.get("s").and_then(Value::as_object),
        ) {
            for (id, loc) in map {
                let line = loc.pointer("/start/line").and_then(Value::as_u64);
                let h = match hits.get(id).map(Value::as_u64) {
                    Some(Some(h)) => h,
                    // a missing count is 0; one that is there but not a non-negative integer is counted
                    Some(None) => {
                        odd += 1;
                        0
                    }
                    None => 0,
                };
                match line.and_then(line_no) {
                    Some(l) => file.hit_max(l, h),
                    None => bad += 1,
                }
                check_lines(file.lines.len(), cx)?;
            }
        }
        if let (Some(map), Some(arms)) = (f.get("branchMap").and_then(Value::as_object), f.get("b").and_then(Value::as_object)) {
            for (id, b) in map {
                let line = b
                    .get("line")
                    .and_then(Value::as_u64)
                    .or_else(|| b.pointer("/loc/start/line").and_then(Value::as_u64));
                let hits: Vec<u64> = arms
                    .get(id)
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_u64).collect())
                    .unwrap_or_default();
                match line.and_then(line_no) {
                    Some(l) => file.branch(
                        l,
                        u32::try_from(hits.iter().filter(|h| **h > 0).count()).unwrap_or(u32::MAX),
                        u32::try_from(hits.len()).unwrap_or(u32::MAX),
                    ),
                    None => bad += 1,
                }
            }
        }
        total += file.lines.len();
        check_total(0, total, cx)?;
        report.skipped.bad_lines = report.skipped.bad_lines.saturating_add(bad);
        report.skipped.non_integer_hits = report.skipped.non_integer_hits.saturating_add(odd);
        check_files(report, cx)?;
    }
    Ok(())
}
